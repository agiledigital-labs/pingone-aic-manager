import { afterAll, beforeAll, expect, it } from "vitest";
import { IdmConfig, IdmEmulator, type ManagedType } from "../../src/idm/index.ts";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { js } from "./fixtures.ts";
let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); });
function make(schema: ManagedType["schema"]) {
  return new IdmEmulator(new IdmConfig({ managed: { objects: [{ name: "widget", schema }] }, endpoints: { healthy: js("({ok:true});") } }), runner);
}
it.each([
  ["boolean", "nope"], ["string", 42], ["number", false], ["array", "not an array"], ["object", []],
] as const)("A2 #2 invalid %s default refuses before a write can succeed", async (type, value) => {
  expectEmulationFailure("invalid default: widget/flag");
  const emu = make({ properties: { flag: { type, default: value } } });
  const result = await runner.eval({ engine: "idm", source: "try {openidm.create('managed/widget','a',{})} catch(e) {} 42;" }, { onCall: emu.onCall });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.missing).toContain("invalid default: widget/flag");
});
it("A2 #2 invalid defaults are checked even for absent-record reads", async () => {
  expectEmulationFailure("invalid default: widget/flag");
  const result = await runner.eval({ engine: "idm", source: "openidm.read('managed/widget/absent')" }, { onCall: make({ properties: { flag: { type: "boolean", default: "nope" } } }).onCall });
  expect(result.outcome).not.toBe("ok");
});
const bad: Array<[string, unknown, string]> = [
  ["tuple encryption", { type: "array", items: [{ type: "string", encryption: {} }] }, "schema declaration shape: widget/box/items (expected object)"],
  ["tuple unknown type", { type: "array", items: [{ type: "unmeasured-shape" }] }, "schema declaration shape: widget/box/items (expected object)"],
  ["null items", { type: "array", items: null }, "schema declaration shape: widget/box/items (expected object)"],
  ["array properties", { type: "object", properties: [] }, "schema declaration shape: widget/box/properties (expected object)"],
  ["non-object child", { type: "object", properties: { secret: "ignored" } }, "schema declaration shape: widget/box/secret (expected object)"],
  ["non-array enum", { type: "string", enum: {} }, "schema declaration shape: widget/box/enum (expected array)"],
  ["non-script hook", { type: "string", onStore: "ignored" }, "schema declaration shape: widget/box/onStore (expected script object)"],
  ["non-string file", { type: "string", onStore: { type: "text/javascript", file: 7 } }, "schema declaration shape: widget/box/onStore/file (expected string)"],
  ...["onStore", "onValidate", "onRetrieve"].map((hook): [string, unknown, string] => [
    `nested ${hook}`, { type: "object", properties: { secret: { type: "string", [hook]: js("throw 'not measured'") } } }, `nested property behavior ${hook}: widget/box/secret`,
  ]),
  ["nested file", { type: "object", properties: { secret: { type: "string", onStore: { type: "text/javascript", file: "unimplemented-nested.js" } } } }, "nested script file reference: widget/box/secret/onStore: unimplemented-nested.js"],
  ["nested default", { type: "object", properties: { secret: { type: "string", default: "ignored" } } }, "nested property behavior default: widget/box/secret"],
  ["item default", { type: "array", items: { type: "string", default: "ignored" } }, "nested property behavior default: widget/box/items"],
];
it.each(bad)("A2 #3 %s declaration is missing before scripts can catch it", async (_label, box, cause) => {
  expectEmulationFailure(cause);
  const emu = make({ properties: { box } } as ManagedType["schema"]);
  const result = await runner.eval({ engine: "idm", source: "try {openidm.create('managed/widget','a',{box:{}})} catch(e) {} 42;" }, { onCall: emu.onCall });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.missing).toContain(cause);
});
it("A2 #3 malformed declarations precede even endpoint routing", async () => {
  expectEmulationFailure("schema declaration shape: widget/box/items (expected object)");
  const emu = make({ properties: { box: { type: "array", items: [] } } });
  const result = await emu.request({ method: "GET", path: "endpoint/healthy" });
  expect(result.emulation?.missing).toContain("schema declaration shape: widget/box/items (expected object)");
});
it.each(["enum", "policies", "encryption", "viewable", "id"])("A2 #4 unknown collection action rejects before %s storage refusal", async (key) => {
  const config = key === "enum" ? ["ok"] : key === "policies" ? [] : key === "viewable" ? false : {};
  const schema = key === "id" ? { id: "urn:example:Widget", properties: {} } : { properties: { text: { type: "string", [key]: config } } };
  const result = await runner.eval({ engine: "idm", source: "try {openidm.action('managed/widget','unknown',{})} catch(e) {e.javaException.getCode()}" }, { onCall: make(schema).onCall });
  expect(result).toMatchObject({ outcome: "ok", value: 400 });
});
it.each(["viewable", "userEditable", "id"])("A2 #4 known absent records precede %s storage refusal", async (key) => {
  const schema = key === "id" ? { id: "urn:example:Widget", properties: {} } : { properties: { text: { type: "string", [key]: false } } };
  const emu = make(schema);
  const read = await runner.eval({ engine: "idm", source: "openidm.read('managed/widget/absent')" }, { onCall: emu.onCall });
  expect(read).toMatchObject({ outcome: "ok", value: null });
  expect((await emu.request({ method: "DELETE", path: "managed/widget/absent" })).status).toBe(404);
});
it("A2 #4 malformed declarations still precede the measured unknown-action rejection", async () => {
  expectEmulationFailure("schema declaration shape: widget/box/items (expected object)");
  const emu = make({ properties: { box: { type: "array", items: [] } } });
  const result = await runner.eval({ engine: "idm", source: "try {openidm.action('managed/widget','unknown',{})} catch(e) {400}" }, { onCall: emu.onCall });
  expect(result.emulation?.missing).toContain("schema declaration shape: widget/box/items (expected object)");
});
it("A2 #4 schema id remains refused once storage is reached", async () => {
  expectEmulationFailure("property behavior id: widget");
  const result = await runner.eval({ engine: "idm", source: "openidm.query('managed/widget',{_queryFilter:'true'})" }, { onCall: make({ id: "urn:example:Widget", properties: {} }).onCall });
  expect(result.emulation?.missing).toContain("property behavior id: widget");
});
it.each(["{}", "{_queryExpression:'x'}"])("A2 #4 query rejection %s precedes storage capabilities", async (params) => {
  const result = await runner.eval({ engine: "idm", source: `try {openidm.query('managed/widget',${params})} catch(e) {e.javaException.getCode()}` }, { onCall: make({ properties: { text: { type: "string", viewable: false } } }).onCall });
  expect(result).toMatchObject({ outcome: "ok", value: params === "{}" ? 400 : 405 });
});
it("A2 #3 declaration validation precedes host argument normalization", async () => {
  expectEmulationFailure("schema declaration shape: widget/box/items (expected object)");
  const emu = make({ properties: { box: { type: "array", items: [] } } });
  const result = await runner.eval({ engine: "idm", source: "try {openidm.create('managed/widget','a',null)} catch(e) {} 42;" }, { onCall: emu.onCall });
  expect(result.emulation?.missing).toContain("schema declaration shape: widget/box/items (expected object)");
  expect(result.outcome).not.toBe("ok");
});
