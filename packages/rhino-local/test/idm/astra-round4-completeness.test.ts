import { afterAll, beforeAll, expect, it } from "vitest";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { IdmConfig, IdmEmulator, type ConfigInput } from "../../src/idm/index.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { js } from "./fixtures.ts";
let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); });
function make(complete = false) {
  return new IdmEmulator(new IdmConfig({ sync: { mappings: [] },
    managed: { objects: [{ name: "widget", schema: { properties: {} } }] },
    endpoints: {
      first: js("({absent:openidm.read('endpoint/fourth') === null});"),
      second: js("({ok:2});"), third: js("({ok:3});"),
    },
    ...(complete ? { complete: { managed: true, endpoints: true } } : {}),
  }), runner);
}
it("A4 #2 three supplied endpoints cannot prove a fourth absent, even inside a hook", async () => {
  expectEmulationFailure("resource route: endpoint/fourth");
  const result = await runner.eval({ engine: "idm", source: "try {openidm.read('endpoint/first')} catch(e) {} 42;" }, { onCall: make().onCall });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.missing).toContain("resource route: endpoint/fourth");
});
it("A4 #2 explicit endpoint completeness permits the nested null branch and transports evidence", async () => {
  const result = await runner.eval({ engine: "idm", source: "openidm.read('endpoint/first')" }, { onCall: make(true).onCall });
  expect(result).toMatchObject({ outcome: "ok", value: { absent: true } });
  expect(result.provenance).toContainEqual({ kind: "assumed", behavior: "caller declared endpoints namespace complete" });
  expect(result.provenance).toContainEqual({ kind: "measured", behavior: "unconfigured managed/endpoint names read as null internally", source: "orchestrator p24, 2026-11-02" });
});
for (const [path, namespace] of [["endpoint/fourth", "endpoints"], ["config/endpoint/fourth", "endpoints"], ["managed/fourth/a", "managed"]] as const) {
  it(`A4 #2 partial ${namespace} map leaves ${path} unknown in a fork and REST`, async () => {
    expectEmulationFailure(`resource route: ${path}`);
    const emu = make().fork();
    expect(await emu.request({ method: "GET", path })).toMatchObject({ status: 501, emulation: { kind: "missing", missing: [`resource route: ${path}`] } });
    const result = await runner.eval({ engine: "idm", source: `try {openidm.read('${path}')} catch(e) {} 42;` }, { onCall: emu.onCall });
    expect(result.outcome).not.toBe("ok");
    expect(result.emulation?.missing).toContain(`resource route: ${path}`);
  });
  it(`A4 #2 explicit ${namespace} completeness labels every absence for ${path}`, async () => {
    const emu = make(true).fork();
    const result = await runner.eval({ engine: "idm", source: `openidm.read('${path}')` }, { onCall: emu.onCall });
    expect(result).toMatchObject({ outcome: "ok", value: null });
    const label = { kind: "assumed", behavior: `caller declared ${namespace} namespace complete` };
    expect(result.provenance).toContainEqual(label);
    const rest = await emu.request({ method: "GET", path });
    expect(rest.status).toBe(404);
    expect(rest.provenance).toContainEqual(label);
  });
}
it.each(["managed", "endpoints"] as const)("A4 #2 unknown %s query is missing until completeness is declared", async (namespace) => {
  const path = `${namespace === "managed" ? "managed" : "endpoint"}/fourth`;
  expectEmulationFailure(`resource route: ${path}`);
  const source = `try {openidm.query('${path}',{_queryFilter:'true'})} catch(e) {e.javaException.getCode()}`;
  const unknown = await runner.eval({ engine: "idm", source }, { onCall: make().onCall });
  expect(unknown.outcome).not.toBe("ok");
  const absent = await runner.eval({ engine: "idm", source }, { onCall: make(true).onCall });
  expect(absent).toMatchObject({ outcome: "ok", value: 404 });
  expect(absent.provenance).toContainEqual({ kind: "assumed", behavior: `caller declared ${namespace} namespace complete` });
});
it("A4 #2 completeness is per namespace and never changes missing ids in an implemented collection", async () => {
  expectEmulationFailure("resource route: endpoint/fourth");
  const emu = new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [{ name: "widget", schema: { properties: {} } }] }, endpoints: {}, complete: { managed: true } }), runner);
  expect(await runner.eval({ engine: "idm", source: "openidm.read('managed/widget/absent')" }, { onCall: emu.onCall })).toMatchObject({ outcome: "ok", value: null });
  expect(await runner.eval({ engine: "idm", source: "openidm.read('managed/fourth/a')" }, { onCall: emu.onCall })).toMatchObject({ outcome: "ok", value: null });
  const unknown = await runner.eval({ engine: "idm", source: "openidm.read('endpoint/fourth')" }, { onCall: emu.onCall });
  expect(unknown.outcome).not.toBe("ok");
});
it.each([null, [], Object.setPrototypeOf([], null) as unknown, false, 0].map((value) => [value]))("A4 #2 malformed completeness container fails construction: %j", (complete) => {
  expect(() => new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] }, complete } as unknown as ConfigInput), runner)).toThrow("Invalid IDM configuration: complete must be a plain object");
});
it.each([{ endpoints: "true" }, { managed: 1 }, { system: true }])("A4 #2 malformed completeness flags fail construction: %j", (complete) => {
  expect(() => new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] }, endpoints: {}, complete } as unknown as ConfigInput), runner)).toThrow("Invalid IDM configuration: complete supports only boolean managed/endpoints flags");
});
it("A4 #2 endpoint completeness requires an explicit map, including an empty one", () => {
  expect(() => new IdmEmulator(new IdmConfig({ sync: { mappings: [] }, managed: { objects: [] }, complete: { endpoints: true } }), runner)).toThrow("Invalid IDM configuration: complete.endpoints requires an endpoints map");
});
