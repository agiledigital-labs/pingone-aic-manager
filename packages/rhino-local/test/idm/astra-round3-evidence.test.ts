import { afterAll, beforeAll, expect, it } from "vitest";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { reportHostEvidence } from "../../src/host-evidence.ts";
import type { BehaviorEvidence } from "../../src/protocol.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { js } from "./fixtures.ts";
let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); });
function make() { return new IdmEmulator(new IdmConfig({ managed: { objects: [{ name: "alpha_user", schema: { properties: { password: { type: "string" } } } }] }, endpoints: { nested: js("openidm.read('config/managed');({ok:true});") } }), runner); }
it("A3 #2 public onCall exposes password assumptions and measurement sources", async () => {
  const emu = make(), path = "managed/alpha_user/00000000-0000-0000-0000-000000000001";
  await emu.request({ method: "PUT", path, body: { password: "submitted-secret" } });
  const result = await runner.eval({ engine: "idm", source: `openidm.read('${path}').password` }, { onCall: emu.onCall });
  expect(result.outcome).toBe("ok");
  expect(result.assumed).toEqual(expect.arrayContaining([expect.stringContaining("opaque password body")]));
  expect(result.measured).toContain("alpha_user internal read/query exposes encoded password prefix");
  expect(result.provenance).toContainEqual({ kind: "measured", behavior: "alpha_user internal read/query exposes encoded password prefix", source: "orchestrator p22/p23, 2026-11-02; docs/api/10-managed-objects.md" });
});
it("A3 #2 caught endpoint query retains its host diagnostic assumption", async () => {
  const result = await runner.eval({ engine: "idm", source: "try {openidm.query('endpoint/absent',{_queryFilter:'true'})} catch(e) {e.javaException.getCode()}" }, { onCall: make().onCall });
  expect(result).toMatchObject({ outcome: "ok", value: 404 });
  expect(result.assumed).toContain("unconfigured endpoint query uses the managed NotFoundException diagnostic");
});
it("A3 #2 caught record absence error retains its host assumption", async () => {
  const result = await runner.eval({ engine: "idm", source: "try {openidm.delete('managed/alpha_user/absent',null)} catch(e) {e.javaException.getCode()}" }, { onCall: make().onCall });
  expect(result).toMatchObject({ outcome: "ok", value: 404 });
  expect(result.assumed).toContain("missing-record message omits tenant directory DN");
});
it("A3 #2 bound nested script host calls retain config-read evidence", async () => {
  const result = await runner.eval({ engine: "idm", source: "openidm.read('endpoint/nested').ok" }, { onCall: make().onCall });
  expect(result).toMatchObject({ outcome: "ok", value: true });
  expect(result.assumed).toContain("config snapshot reads are immediately consistent");
});
it.each(["hostRequest", "request"])("A3 #2 a %s adapter also publishes evidence into the owning eval", async (adapter) => {
  const emu = make(); let calls = 0;
  const result = await runner.eval({ engine: "idm", source: "openidm.read('config/managed').objects.length" }, { onCall: async (call) => {
    calls++;
    return (adapter === "hostRequest" ? await emu.hostRequest(call) : await emu.request({ method: "GET", path: "config/managed" })).body;
  } });
  expect(result).toMatchObject({ outcome: "ok", value: 1 });
  expect(result.assumed).toContain("config snapshot reads are immediately consistent");
  expect(calls).toBe(1); // Also pins exactly-once effects under both/shadow replay.
});
it("A3 #2 failed host calls retain evidence alongside the fatal outcome", async () => {
  expectEmulationFailure("named _queryId: unimplemented");
  const result = await runner.eval({ engine: "idm", source: "try {openidm.query('managed/alpha_user',{_queryId:'unimplemented'})} catch(e) {} 42;" }, { onCall: make().onCall });
  expect(result.outcome).not.toBe("ok");
  expect(result.measured).toContain("managed CRUD status and response shapes");
  expect(result.provenance).toContainEqual({ kind: "missing", behavior: "named _queryId: unimplemented" });
});
it("A3 #2 concurrent evaluation evidence stays with its own job", async () => {
  const emu = make();
  const [read, query] = await Promise.all([
    runner.eval({ engine: "idm", source: "openidm.read('config/managed')" }, { onCall: emu.onCall }),
    runner.eval({ engine: "idm", source: "try {openidm.query('endpoint/absent',{_queryFilter:'true'})} catch(e) {404}" }, { onCall: emu.onCall }),
  ]);
  expect(read.assumed).toContain("config snapshot reads are immediately consistent");
  expect(read.assumed).not.toContain("unconfigured endpoint query uses the managed NotFoundException diagnostic");
  expect(query.assumed).toContain("unconfigured endpoint query uses the managed NotFoundException diagnostic");
  expect(query.assumed).not.toContain("config snapshot reads are immediately consistent");
});
it("A3 #2 ignored nested runner results still retain runtime assumptions", async () => {
  const nested = await RhinoRunner.spawn();
  try {
    const result = await runner.eval({ engine: "idm", source: "openidm.read('nested');42;" }, { onCall: async () => {
      await nested.eval({ engine: "idm", source: "identityServer.getProperty('nested-evidence')" });
      return null;
    } });
    expect(result).toMatchObject({ outcome: "ok", value: 42 });
    expect(result.assumed).toContain("property nested-evidence absent");
  } finally { await nested.close(); }
});

it("A3 #2 reply consumption freezes evidence before an unanswered late reply", async () => {
  expectEmulationFailure("IDM harness timeout");
  const emu = make();
  const result = await runner.eval({ engine: "idm", timeoutMs: 150, source: "openidm.read('config/managed');openidm.read('config/endpoint/absent');42;" }, { onCall: async (call) => {
    const value = await emu.onCall(call);
    if (call.args[0] === "config/endpoint/absent") {
      const until = Date.now() + 350;
      while (Date.now() < until) { /* Deliberately block Node beyond the JVM deadline. */ }
    }
    return value;
  } });
  expect(result.outcome).toBe("timeout");
  expect(result.assumed).toContain("config snapshot reads are immediately consistent");
  expect(result.assumed).not.toContain("configured endpoint map also establishes config/endpoint snapshot absence");
  expect(result.unanswered).toContain(2);
});
it("A3 #2 invalid host evidence is an uncatchable protocol failure", async () => {
  expectEmulationFailure("rhino-local: invalid host evidence: expected evidence kind/behavior");
  const emu = make();
  const result = await runner.eval({ engine: "idm", source: "try {openidm.read('config/managed')} catch(e) {} 42;" }, { onCall: async (call) => {
    const value = await emu.onCall(call);
    reportHostEvidence([{ kind: "invalid", behavior: "adapter-bug" } as unknown as BehaviorEvidence]);
    return value;
  } });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.kind).toBe("protocol");
});
it("A3 #2 a hostRequest adapter cannot turn missing evidence into tenant data", async () => {
  expectEmulationFailure("named _queryId: unimplemented");
  const emu = make();
  const result = await runner.eval({ engine: "idm", source: "try {openidm.query('managed/alpha_user',{_queryId:'unimplemented'})} catch(e) {} 42;" }, { onCall: async (call) => (await emu.hostRequest(call)).body });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.missing).toContain("named _queryId: unimplemented");
});
