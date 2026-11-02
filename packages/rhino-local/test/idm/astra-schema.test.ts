import { afterAll, beforeAll, expect, it } from "vitest";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { IdmConfig, IdmEmulator } from "../../src/idm/index.ts";
import type { PropertyConfig } from "../../src/idm/types.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { js } from "./fixtures.ts";
let runner: RhinoRunner;
beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
afterAll(async () => { await runner?.close(); });
const cases: Array<[string, PropertyConfig]> = [
  ["type", { type: "unmeasured-shape" }],
  ["encryption", { encryption: { cipher: "AES/CBC/PKCS5Padding", key: "example" } }],
  ["secureHash", { secureHash: { algorithm: "SHA-256" } }],
  ["isVirtual", { isVirtual: true, onRetrieve: js("'derived'") }],
  ["returnByDefault", { returnByDefault: false }],
  ["viewable", { viewable: false }], ["userEditable", { userEditable: false }],
  ["scope", { scope: "private" }], ["customRuntimeOption", { customRuntimeOption: true }],
];
it.each(cases)("Astra #3 refuses reached %s schema instead of storing plausible values", async (flag, declaration) => {
  expectEmulationFailure(`property behavior ${flag}: widget/secret`);
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [{ name: "widget", schema: { properties: { secret: { type: "string", ...declaration } } } }] } }), runner);
  const result = await runner.eval({ engine: "idm", source: "try { openidm.create('managed/widget','a',{secret:'clear'}); } catch(e) {} 42;" }, { onCall: emu.onCall });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.missing).toContain(`property behavior ${flag}: widget/secret`);
});
it.each(["encryption", "secureHash"])("Astra #3 detects nested array item %s", async (flag) => {
  expectEmulationFailure(`property behavior ${flag}: widget/box/items/secret`);
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [{ name: "widget", schema: { properties: {
    box: { type: "array", items: { type: "object", properties: { secret: { type: "string", [flag]: {} } } } },
  } } }] } }), runner);
  const result = await runner.eval({ engine: "idm", source: "try { openidm.create('managed/widget','a',{box:[{secret:'clear'}]}); } catch(e) {} 42;" }, { onCall: emu.onCall });
  expect(result.outcome).not.toBe("ok");
});
it.each(["isVirtual", "returnByDefault", "viewable", "scope"])("Astra #3 read reaches declared %s even on empty collections", async (flag) => {
  expectEmulationFailure(`property behavior ${flag}: widget/secret`);
  const declaration = cases.find(([key]) => key === flag)![1];
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [{ name: "widget", schema: { properties: { secret: { type: "string", ...declaration } } } }] } }), runner);
  const result = await runner.eval({ engine: "idm", source: "try { openidm.query('managed/widget',{_queryFilter:'true'}); } catch(e) {} 42;" }, { onCall: emu.onCall });
  expect(result.outcome).not.toBe("ok");
});
it("Astra #3 alpha_user GET and query already omit write-only password", async () => {
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [{ name: "alpha_user", schema: { properties: { password: { type: "string" }, mail: { type: "string" } } } }] } }), runner);
  const result = await runner.eval({ engine: "idm", source: `
    var r=openidm.create('managed/alpha_user',null,{password:'secret',mail:'Mixed@Example.com'});
    var read=openidm.read('managed/alpha_user/'+r._id);
    var row=openidm.query('managed/alpha_user',{_queryFilter:'true'}).result[0];
    ({readHas:'password' in read,queryHas:'password' in row,mail:read.mail});
  ` }, { onCall: emu.onCall });
  expect(result).toMatchObject({ outcome: "ok", value: { readHas: false, queryHas: false, mail: "Mixed@Example.com" } });
});

it.each(["enum", "policies", "required"])("Astra #3 declared %s policy cannot silently pass without its evaluator", async (key) => {
  const schema = { properties: { secret: { type: "string", ...(key === "enum" ? { enum: ["allowed"] } : key === "policies" ? { policies: [{ policyId: "valid-type" }] } : {}) } }, ...(key === "required" ? { required: ["secret"] } : {}) };
  const cause = `schema policy ${key}: ${key === "required" ? "widget" : "widget/secret"}`;
  expectEmulationFailure(cause);
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [{ name: "widget", schema }] } }), runner);
  const result = await runner.eval({ engine: "idm", source: "openidm.create('managed/widget','a',{})" }, { onCall: emu.onCall });
  expect(result.outcome).not.toBe("ok");
  expect(result.emulation?.missing).toContain(cause);
});

it("Astra #3 an empty enum is a declaration, not permission to accept anything", async () => {
  expectEmulationFailure("schema policy enum: widget/secret");
  const emu = new IdmEmulator(new IdmConfig({ managed: { objects: [{ name: "widget", schema: { properties: { secret: { type: "string", enum: [] } } } }] } }), runner);
  const result = await runner.eval({ engine: "idm", source: "try {openidm.create('managed/widget','a',{secret:'not-allowed'})} catch(e) {} 42;" }, { onCall: emu.onCall });
  expect(result.outcome).not.toBe("ok");
});
