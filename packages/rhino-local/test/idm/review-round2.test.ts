/** Reviewer failures exercised on the real IDM emulator and pinned JVM. */
import { afterAll, beforeAll, expect, it, vi } from "vitest";
import { HostCallError, RhinoRunner } from "../../src/runner.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import { create, emulator, js, simple } from "./fixtures.ts";

let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); }, 30_000);

it("R2 #1 missing escapes catches and makes standalone outcome fail", async () => {
  const emu = emulator(runner);
  const response = await runner.eval({ engine: "idm", bindings: { state: { caught: false } }, source: `
    try { openidm.query('managed/widget',{_queryId:'missing'}); } catch(e) { state.caught=true; }
    'passed';
  ` }, { onCall: emu.onCall });
  expect(response.outcome).not.toBe("ok");
  expect(response.missing).toContain("named _queryId: missing");
  expect(response.bindings?.state).toEqual({ caught: false });
});

it("R2 #1 runtime missing helpers also escape catches", async () => {
  const response = await runner.eval({ engine: "idm", bindings: { state: { caught: false } }, source: `
    try { require('unsupported'); } catch(e) { state.caught=true; } 'passed';
  ` });
  expect(response.outcome).not.toBe("ok");
  expect(response.missing).toContain("require");
  expect(response.bindings?.state).toEqual({ caught: false });
});

it("R2 new1 policy mapping retains missing and prevents catch continuation", async () => {
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [simple] } }), runner, { policy: () => {
    throw new HostCallError({ class: "example.Policy", code: 501, message: "policy unavailable", detail: {}, missing: ["policy helper"] });
  } });
  const response = await runner.eval({ engine: "idm", source: "try {openidm.create('managed/widget','one',{});} catch(e) {} 'passed';" }, { onCall: emu.onCall });
  expect(response.outcome).not.toBe("ok");
  expect(response.missing).toContain("policy helper");
  const result = await emu.request({ method: "PUT", path: "managed/widget/one", body: {} });
  expect(result.status).toBe(501);
  expect(JSON.stringify(result)).toContain("policy helper");
});

it("R2 new2 admission refusal remains fatal through hook and endpoint mappings", async () => {
  const limited = await RhinoRunner.spawn({ maxConcurrentJobs: 1 });
  try {
    const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [{ ...simple, onRetrieve: js("object.text='hook';") }] }, endpoints: {
      example: js("try {openidm.read('managed/widget/one');} catch(e) {({passed:true});}"),
    } }), limited);
    await create(emu);
    const result = await emu.request({ method: "GET", path: "endpoint/example" });
    expect(result.status).toBeGreaterThanOrEqual(500);
    expect(JSON.stringify(result)).toContain("concurrent job limit");
  } finally { await limited.close(); }
}, 30_000);

it("R2 harness timeout remains fatal through hook and endpoint mappings", async () => {
  const evaluate = runner.eval.bind(runner);
  const spy = vi.spyOn(runner, "eval").mockImplementation((job, options) => evaluate({ ...job,
    ...(job.source.includes("round2-timeout") ? { timeoutMs: 80 } : {}),
  }, options));
  try {
    const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [{ ...simple, onRetrieve: js("while(true){} /* round2-timeout */") }] }, endpoints: {
      example: js("try {openidm.read('managed/widget/one');} catch(e) {({passed:true});}"),
    } }), runner);
    await create(emu);
    const result = await emu.request({ method: "GET", path: "endpoint/example" });
    expect(result.status).toBeGreaterThanOrEqual(500);
    expect(JSON.stringify(result).toLowerCase()).toContain("timeout");
  } finally { spy.mockRestore(); }
});

it.each(["protocol", "timeout"] as const)("R2 nested %s failure cannot be ignored by an onCall adapter", async (kind) => {
  const nestedRunner = await RhinoRunner.spawn({ maxConcurrentJobs: kind === "protocol" ? 1 : 2 });
  try {
    const result = await nestedRunner.eval({ engine: "idm", source: "try {openidm.read('outer');} catch(e) {} 'passed';" }, {
      onCall: async () => {
        await nestedRunner.eval({ engine: "idm", timeoutMs: 80, source: kind === "timeout" ? "while(true){}" : "42" });
        return 42;
      },
    });
    expect(result.outcome).not.toBe("ok");
    expect(result.emulation?.kind).toBe(kind);
  } finally { await nestedRunner.close(); }
}, 30_000);

it("R2 an adapter cannot swallow a nested protocol rejection", async () => {
  const result = await runner.eval({ id: "round2-duplicate", engine: "idm", source: "try{openidm.read('x')}catch(e){} 'passed'" }, {
    onCall: async () => {
      try { await runner.eval({ id: "round2-duplicate", source: "42" }); } catch { /* adapter swallowed it */ }
      return 42;
    },
  });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.kind).toBe("protocol");
});
