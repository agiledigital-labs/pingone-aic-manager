import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
/** Reviewer failures exercised on the real IDM emulator and pinned JVM. */
import { afterAll, beforeAll, expect, it, vi } from "vitest";
import { HostCallError, RhinoRunner } from "../../src/runner.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import { create, emulator, js, send, simple } from "./fixtures.ts";

let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); }, 30_000);

it("R2 #1 missing escapes catches and makes standalone outcome fail", async () => {
    expectEmulationFailure("named _queryId: missing");
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
    expectEmulationFailure("require");
  const response = await runner.eval({ engine: "idm", bindings: { state: { caught: false } }, source: `
    try { require('unsupported'); } catch(e) { state.caught=true; } 'passed';
  ` });
  expect(response.outcome).not.toBe("ok");
  expect(response.missing).toContain("require");
  expect(response.bindings?.state).toEqual({ caught: false });
});

it("R2 new1 policy mapping retains missing and prevents catch continuation", async () => {
    expectEmulationFailure("policy helper");
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
    expectEmulationFailure(/rhino-local: concurrent job limit exceeded \(\d+\)/);
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
    expectEmulationFailure("IDM harness timeout");
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
    expectEmulationFailure(kind === "timeout" ? /IDM harness timeout/ : /rhino-local: concurrent job limit exceeded \(\d+\)/);
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
    expectEmulationFailure(/rhino-local: duplicate active job id: .+/);
  const result = await runner.eval({ id: "round2-duplicate", engine: "idm", source: "try{openidm.read('x')}catch(e){} 'passed'" }, {
    onCall: async () => {
      try { await runner.eval({ id: "round2-duplicate", source: "42" }); } catch { /* adapter swallowed it */ }
      return 42;
    },
  });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.kind).toBe("protocol");
});


it.each(["__proto__", "constructor", "toString", "prototype"])("R2 new5 unconfigured %s endpoint names are own lookups", async (name) => {
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [] }, endpoints: {}, complete: { endpoints: true } }), runner);
  for (const prefix of ["endpoint", "config/endpoint"]) {
    const result = await emu.request({ method: "GET", path: `${prefix}/${name}` });
    expect(result.status).toBe(404);
    expect(result.emulation).toBeUndefined();
  }
});

it.each(["__proto__", "constructor", "prototype"])("R2 new5 property %s hooks receive absent null and store own data", async (name) => {
  const emu = emulator(runner, { name: "widget", schema: { properties: {
    [name]: { type: "object", onStore: js("if(property!==null) throw 'not absent'; ({saved:true});") },
  } } });
  const created = await create(emu, {});
  expect(Object.hasOwn(created, name)).toBe(true);
  expect(created[name]).toEqual({ saved: true });
  expect((await send(emu, { method: "GET", path: "managed/widget/one" }))[name]).toEqual({ saved: true });
});

it("R2 new5 configured prototype-named routes work and action names stay own", async () => {
    expectEmulationFailure("managed object action: toString");
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [{ ...simple, actions: {} }] }, endpoints: {
    ["__proto__"]: js("({ok:true});"),
  } }), runner);
  expect((await emu.request({ method: "GET", path: "endpoint/__proto__" })).body).toMatchObject({ ok: true });
  await create(emu);
  const result = await emu.request({ method: "POST", path: "managed/widget/one", query: { _action: "toString" } });
  expect(JSON.stringify(result)).toContain("managed object action: toString");
});

it.each(["PATCH", "PUT", "DELETE"] as const)("R2 new6 %s cannot overwrite a recreated incarnation", async (method) => {
  let entered!: () => void;
  let release!: () => void;
  const started = new Promise<void>((resolve) => { entered = resolve; });
  const gate = new Promise<void>((resolve) => { release = resolve; });
  let pause = true;
  const hook = js("openidm.create('managed/gate',null,{pause:true});");
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [
    { ...simple, onUpdate: hook, onDelete: hook },
    { name: "gate", schema: { properties: { pause: { type: "boolean" } } } },
  ] } }), runner, { policy: async ({ object }) => {
    if (object.pause === true && pause) { pause = false; entered(); await gate; }
  } });
  await create(emu, { text: "original" });
  const pending = emu.request({ method, path: "managed/widget/one", headers: { "If-Match": "0" },
    ...(method === "DELETE" ? {} : { body: method === "PATCH" ? [{ operation: "replace", field: "/text", value: "stale" }] : { text: "stale" } }),
  });
  await started;
  let replacement;
  try {
    await send(emu, { method: "DELETE", path: "managed/widget/one" });
    replacement = await emu.request({ method: "PUT", path: "managed/widget/one", body: { text: "replacement" } });
  } finally { release(); }
  expect((await pending).status).toBe(412);
  expect(await send(emu, { method: "GET", path: "managed/widget/one" })).toMatchObject({ text: "replacement", _rev: "0" });
  expect(replacement.provenance).toContainEqual({ kind: "assumed", behavior: "recreated records restart revision at 0" });
});

it("R2 cookies reject trailing data", async () => {
  const emu = emulator(runner);
  for (const id of ["a", "b", "c"]) await create(emu, { text: id }, id);
  const query = { _queryFilter: "true", _pageSize: 1, _sortKeys: "_id" };
  const page = await send(emu, { method: "GET", path: "managed/widget", query });
  await send(emu, { method: "GET", path: "managed/widget", query: { ...query, _pagedResultsCookie: `${String(page.pagedResultsCookie)}.garbage` } }, 400);
});

it("R2 forks accept parent cookies and re-evaluate their own divergent rows", async () => {
  const emu = emulator(runner);
  for (const id of ["a", "b", "c"]) await create(emu, { text: id }, id);
  const query = { _queryFilter: "true", _pageSize: 1, _sortKeys: "_id" };
  const page = await send(emu, { method: "GET", path: "managed/widget", query });
  const child = emu.fork();
  const continuation = { ...query, _pagedResultsCookie: String(page.pagedResultsCookie) };
  expect((await send(child, { method: "GET", path: "managed/widget", query: continuation })).result).toMatchObject([{ _id: "b" }]);
  await create(child, { text: "d" }, "d");
  const rest = { ...continuation, _pageSize: 10 };
  expect((await send(child, { method: "GET", path: "managed/widget", query: rest })).result).toMatchObject([{ _id: "b" }, { _id: "c" }, { _id: "d" }]);
  expect((await send(emu, { method: "GET", path: "managed/widget", query: rest })).result).toMatchObject([{ _id: "b" }, { _id: "c" }]);
});


it("R2 new8 internal endpoint numbers survive while HTTP output stays measured", async () => {
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [] }, endpoints: {
    inner: js("({n:NaN,inf:Infinity,received:typeof request.content.n,receivedNan:request.content.n!==request.content.n,receivedInf:request.content.inf===Infinity});"),
    outer: js("var r=openidm.action('endpoint/inner','go',{n:NaN,inf:Infinity}); ({received:r.received,receivedNan:r.receivedNan,receivedInf:r.receivedInf,numeric:typeof r.n==='number',nan:r.n!==r.n,inf:r.inf===Infinity});"),
  } }), runner);
  const result = await emu.request({ method: "GET", path: "endpoint/outer" });
  expect(result.status).toBe(200);
  expect(result.body).toMatchObject({ received: "number", receivedNan: true, receivedInf: true, numeric: true, nan: true, inf: true });
  expect(result.provenance).toContainEqual({ kind: "assumed", behavior: "internal openidm special-number transport preserves numeric values; tenant behavior unmeasured" });
  const wire = await emu.request({ method: "POST", path: "endpoint/inner", query: { _action: "go" }, body: {} });
  expect(wire.body).toMatchObject({ n: "NaN", inf: Number("9223372036854775807") });
});

it.each(["source", "bindings", "policy"] as const)("R2 error-conversion audit: %s failures cannot become tenant success", async (boundary) => {
    expectEmulationFailure(boundary === "source" ? /java.lang.IllegalArgumentException: source must be a string/ : boundary === "bindings" ? /IDM protocol setup: unknown binding alias target: absent/ : /Error: policy adapter crashed/);
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [simple] } }), runner, { policy: () => { throw new Error("policy adapter crashed"); } });
  const result = await runner.eval({ engine: "idm", source: "try{openidm.read('audit')}catch(e){} 'passed'" }, {
    onCall: async (call) => {
      if (boundary === "policy") return emu.onCall({ ...call, method: "create", args: ["managed/widget", "audit", {}] });
      const nested = await runner.eval(boundary === "source"
        ? { engine: "idm", source: 7 as unknown as string }
        : { engine: "idm", source: "42", bindingAliases: { newObject: "absent" } });
      return nested.value;
    },
  });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.kind).toBe(boundary === "bindings" ? "protocol" : "internal");
});

it("R2 plain adapter exceptions are internal failures, not tenant CREST errors", async () => {
    expectEmulationFailure("adapter crashed");
  const result = await runner.eval({ engine: "idm", bindings: { state: { caught: false } }, source: "try{openidm.read('adapter')}catch(e){state.caught=true} 'passed'" }, {
    onCall: () => { throw new Error("adapter crashed"); },
  });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation).toMatchObject({ kind: "internal", message: "adapter crashed" });
  expect(result.bindings?.state).toEqual({ caught: false });
});
