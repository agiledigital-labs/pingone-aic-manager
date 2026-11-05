import { afterAll, beforeAll, expect, it } from "vitest";
import { IdmConfig, IdmEmulator, type ConfigInput, type Document } from "../../src/idm/index.ts";
import { expectEmulationFailure, expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { js } from "./fixtures.ts";
let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); });
it.each(["endpoint/notSupplied", "config/endpoint/notSupplied"])("A3 #1 omitted endpoints are unknown for %s", async (path) => {
  expectEmulationFailure(`resource route: ${path}`);
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [] } }), runner);
  const result = await runner.eval({ engine: "idm", source: `try {openidm.read('${path}')} catch(e) {} 42;` }, { onCall: emu.onCall });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.missing).toContain(`resource route: ${path}`);
});
it.each([{}, { actual: js("({ok:true})") }])("A3 #1 an explicitly complete endpoint map establishes absence: %j", async (endpoints) => {
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [] }, endpoints, complete: { endpoints: true } }), runner);
  const result = await runner.eval({ engine: "idm", source: "({endpoint:openidm.read('endpoint/notSupplied'),config:openidm.read('config/endpoint/notSupplied')})" }, { onCall: emu.onCall });
  expect(result).toMatchObject({ outcome: "ok", value: { endpoint: null, config: null } });
});
it("A3 #1 forks retain unknown endpoint coverage", async () => {
  expectEmulationFailure("resource route: endpoint/notSupplied");
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [] } }), runner).fork();
  const result = await runner.eval({ engine: "idm", source: "openidm.read('endpoint/notSupplied')" }, { onCall: emu.onCall });
  expect(result.emulation?.missing).toContain("resource route: endpoint/notSupplied");
});
it("A3 #1 managed configuration is required rather than defaulted to authoritative empty", () => {
  expect(() => new IdmConfig({} as ConfigInput)).toThrow();
});
it.each(["patch", "update"])("A3 #3 stale/current %s both reach unsupported retrieval before revisions", async (method) => {
  // E15: a file reference is an IDM runtime script; with no provider it is the harness fault.
  expectHarnessFault("idm-runtime-script/unsupported.js");
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [{ name: "widget", schema: { properties: { text: { type: "string" } } }, onRetrieve: { type: "text/javascript", file: "unsupported.js" } }] } }), runner);
  await emu.request({ method: "PUT", path: "managed/widget/a", body: { text: "old" } });
  for (const revision of ["stale", "0"]) {
    const body = method === "patch" ? "[{operation:'replace',field:'/text',value:'new'}]" : "{text:'new'}";
    const result = await runner.eval({ engine: "idm", source: `try {openidm.${method}('managed/widget/a','${revision}',${body})} catch(e) {412;}` }, { onCall: emu.onCall });
    expect(result.outcome).not.toBe("ok");
    expect(result.emulation?.missing?.some((cause) => cause.startsWith("idm-runtime-script/unsupported.js: "))).toBe(true);
  }
});
it("A3 #3 stale order is assumed and a supported retrieval runs before 412", async () => {
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [{ name: "widget", schema: { properties: { text: { type: "string" } } }, onRetrieve: js("object.text='retrieved'") }] } }), runner);
  await emu.request({ method: "PUT", path: "managed/widget/a", body: {} });
  const result = await emu.request({ method: "PATCH", path: "managed/widget/a", headers: { "If-Match": "stale" }, body: [{ operation: "replace", field: "/text", value: "new" }] });
  expect(result.status).toBe(412);
  expect(result.provenance).toContainEqual({ kind: "assumed", behavior: "update pre-read and retrieval hooks run before revision validation, including stale revisions" });
});
it.each([
  { flag: "nope" }, { nested: { flag: "nope" } }, { list: [{ flag: "nope" }] },
])("A3 #4 recursive default mismatch is missing: %j", async (value) => {
  expectEmulationFailure("invalid default: widget/box");
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [{ name: "widget", schema: { properties: { box: { type: "object", properties: {
    flag: { type: "boolean" }, nested: { type: "object", properties: { flag: { type: "boolean" } } }, list: { type: "array", items: { type: "object", properties: { flag: { type: "boolean" } } } },
  }, default: value } } } }] } }), runner);
  const result = await runner.eval({ engine: "idm", source: "try {openidm.create('managed/widget','a',{})} catch(e) {} 42;" }, { onCall: emu.onCall });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.missing).toContain("invalid default: widget/box");
});
it("A3 #4 compatible recursive object defaults remain supported assumptions", async () => {
  const value = { nested: { flag: false }, list: [{ flag: true }] };
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [{ name: "widget", schema: { properties: { box: { type: "object", properties: {
    nested: { type: "object", properties: { flag: { type: "boolean" } } }, list: { type: "array", items: { type: "object", properties: { flag: { type: "boolean" } } } },
  }, default: value } } } }] } }), runner);
  const result = await runner.eval({ engine: "idm", source: "openidm.create('managed/widget','a',{}).box" }, { onCall: emu.onCall });
  expect(result).toMatchObject({ outcome: "ok", value });
  const rest = await emu.request({ method: "GET", path: "managed/widget/a" });
  expect((rest.body as Document).box).toEqual(value);
});
