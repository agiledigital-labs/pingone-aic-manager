import { describe, expect, it } from "vitest";
import { judge, validateCase } from "../../src/case/index.ts";
import type { JsonObject, JsonValue } from "../../src/case/types.ts";
import { carryGiven } from "../../src/harness/step.ts";
import { caseWith } from "../aic/helpers.ts";
import { runScript } from "./load-behaviour.ts";

const bag = "fr-idm-custom-attrs";
const record: JsonObject = { _id: "example", givenName: "Example", custom_example: "seeded", custom_flag: true };
const seed = (row: JsonObject = record) => ({ managed: { "managed/alpha_user": [row] } });
const read = 'var v = identity.getAttributeValues("fr-idm-custom-attrs"); nodeState.putShared("size", v.size()); if (v.size()) nodeState.putShared("bag", JSON.parse(String(v.get(0))));';
const identity = 'var identity = idRepository.getIdentity("example");';

describe("measured custom attribute bag (D10)", () => {
  it("derives a compact single JSON element preserving types and hides IDM custom names", () => {
    const effects = runScript(identity + read + 'nodeState.putShared("directSize", identity.getAttributeValues("custom_example").size());', seed());
    expect(effects.sharedState.final).toEqual({ size: 1, bag: { custom_example: "seeded", custom_flag: true }, directSize: 0 });
  });

  it("reads one empty object when a record has no custom properties", () => {
    const effects = runScript(identity + read, seed({ _id: "example" }));
    expect(effects.sharedState.final).toEqual({ size: 1, bag: {} });
  });

  it("reads persisted values before store, then the new bag on both wrappers", () => {
    const effects = runScript(identity + 'identity.setAttribute("fr-idm-custom-attrs", [\'{"custom_example":"written"}\']);' + read + 'identity.store(); nodeState.putShared("stored", JSON.parse(String(identity.getAttributeValues("fr-idm-custom-attrs").get(0)))); nodeState.putShared("fresh", JSON.parse(String(idRepository.getIdentity("example").getAttributeValues("fr-idm-custom-attrs").get(0))));', seed());
    expect(effects.sharedState.final.bag).toEqual({ custom_example: "seeded", custom_flag: true });
    expect(effects.sharedState.final.stored).toEqual({ custom_example: "written" });
    expect(effects.sharedState.final.fresh).toEqual({ custom_example: "written" });
  });

  it.each([
    { value: { custom_example: "written" }, size: 1 },
    { value: { custom_example: "both", custom_flag: false }, size: 1 },
    { value: { custom_flag: "false" }, size: 1 },
    { value: { custom_unknown: "x" }, size: 1 },
    { value: {}, size: 1 },
  ])("replaces the whole bag with $value without coercion or schema filtering", ({ value, size }) => {
    const values = [JSON.stringify(value)];
    const effects = runScript(identity + `identity.setAttribute(${JSON.stringify(bag)}, ${JSON.stringify(values)}); identity.store();` + read, seed());
    expect(effects.managedStore?.["managed/alpha_user"]?.[0]).toEqual({ _id: "example", givenName: "Example", ...value });
    expect(effects.sharedState.final).toEqual({ size, bag: value });
    expect(effects.identityWrites).toEqual([{ identity: "example", attribute: bag, values }]);
    expect(effects.openidm).toEqual([]);
    const kase = caseWith({ expect: { outcome: null, identityWrites: [{ identity: "example", attribute: bag, values }], allowUndeclared: { sharedState: true } } });
    expect(judge(kase, effects).pass).toBe(true);
    expect(judge(caseWith({ expect: { outcome: null, allowUndeclared: { sharedState: true } } }), effects).summary).toMatch(/identityWrites: undeclared write/);
  });

  it("[] removes all custom properties and retains size 0 across fresh wrappers and passes", () => {
    const given = seed();
    const effects = runScript(identity + 'identity.setAttribute("fr-idm-custom-attrs", []); identity.store();' + read + 'nodeState.putShared("freshSize", idRepository.getIdentity("example").getAttributeValues("fr-idm-custom-attrs").size());', given);
    expect(effects.sharedState.final).toEqual({ size: 0, freshSize: 0 });
    expect(effects.managedStore?.["managed/alpha_user"]?.[0]).toEqual({ _id: "example", givenName: "Example" });
    expect(effects.identityWrites).toEqual([{ identity: "example", attribute: bag, values: [] }]);
    const next = carryGiven(given, effects, []);
    expect(runScript(identity + read, next).sharedState.final.size).toBe(0);
    const restored = runScript(identity + 'identity.setAttribute("fr-idm-custom-attrs", ["{}"]); identity.store();' + read, next);
    expect(restored.sharedState.final.size).toBe(1);
    expect(restored.identityCustomAttrs).toEqual({ "managed/alpha_user/example": ["{}"] });
  });

  it("an IDM patch merges declared properties alongside unknown custom keys", () => {
    const effects = runScript(identity + 'identity.setAttribute("fr-idm-custom-attrs", [\'{"custom_unknown":"x"}\']); identity.store(); openidm.patch("managed/alpha_user/example", null, [{ operation: "replace", field: "custom_example", value: "patched" }, { operation: "replace", field: "custom_flag", value: true }]);' + read, seed());
    expect(effects.sharedState.final.bag).toEqual({ custom_unknown: "x", custom_example: "patched", custom_flag: true });
  });

  it.each([
    { values: ["{}", "{}"], code: 65 },
    { values: ["not json"], code: 21 },
    { values: ["not json", "{}"], code: 21 },
    { values: ["{}", "not json"], code: 21 },
  ])("throws errorcode $code and applies nothing for $values", ({ values, code }) => {
    const effects = runScript(identity + `identity.setAttribute("givenName", ["Changed"]); identity.setAttribute(${JSON.stringify(bag)}, ${JSON.stringify(values)}); try { identity.store(); } catch (e) { nodeState.putShared("error", String(e)); }` + read, seed());
    expect(effects.sharedState.final.error).toBe(`JavaException: org.forgerock.openam.scripting.api.identity.ScriptedIdentityScriptWrapper$IdentityUpdateException: Exception persisting attribute: Plug-in org.forgerock.openam.idrepo.ldap.DJLDAPv3Repo encountered a ldap exception.  ldap errorcode=${code}`);
    expect(effects.managedStore?.["managed/alpha_user"]?.[0]).toEqual(record);
    expect(effects.identityWrites).toEqual([]);
    expect(effects.sharedState.final.bag).toEqual({ custom_example: "seeded", custom_flag: true });
  });

  it.each(["null", "[]", '"text"', '{"plain":"x"}'])("retains measured JSON text %s", (value) => {
    const effects = runScript(identity + `identity.setAttribute(${JSON.stringify(bag)}, [${JSON.stringify(value)}]); identity.store(); nodeState.putShared("text", String(identity.getAttributeValues(${JSON.stringify(bag)}).get(0)));`, seed());
    expect(effects.sharedState.final.text).toBe(value);
  });

  it.each(["setAttribute", "addAttribute"])("refuses direct IDM custom name writes via %s", (method) => {
    const argument = method === "setAttribute" ? '["new"]' : '"new"';
    expect(() => runScript(identity + `identity.${method}("custom_example", ${argument});`, seed())).toThrow(/IDM field name; AM names this attribute "fr-idm-custom-attrs"/);
  });

  it("accepts an explicit bag layout which overrides the measured default", () => {
    const effects = runScript(identity + 'identity.setAttribute("fr-idm-custom-attrs", ["declared"]); identity.store(); nodeState.putShared("value", identity.getAttributeValues("fr-idm-custom-attrs").get(0));', {
      ...seed(), identityAttributes: { [bag]: { field: "elsewhere", cardinality: "single" } },
    });
    expect(effects.sharedState.final.value).toBe("declared");
    expect(effects.managedStore?.["managed/alpha_user"]?.[0]).toEqual({ ...record, elsewhere: "declared" });
    expect(effects.identityWrites?.[0]?.attribute).toBe(bag);
  });
});

describe("explicit bag workaround seed compatibility", () => {
  it.each([JSON.stringify({ custom_flag: true, custom_example: "seeded" }), [JSON.stringify({ custom_example: "seeded", custom_flag: true })]])("accepts agreeing JSON %j regardless of key order and canonicalises it", (explicit) => {
    const given = seed({ ...record, [bag]: explicit });
    const kase = validateCase(caseWith({ given }));
    expect(kase.given.managed?.["managed/alpha_user"]?.[0]).toEqual(record);
    expect(runScript(identity + read, given).sharedState.final.bag).toEqual({ custom_flag: true, custom_example: "seeded" });
    expect(given.managed["managed/alpha_user"]?.[0]?.[bag]).toEqual(explicit);
  });

  it("accepts the legacy empty-object seed", () => {
    expect(runScript(identity + read, seed({ _id: "example", [bag]: "{}" })).sharedState.final.bag).toEqual({});
  });

  it.each<JsonValue>(['{"custom_example":"different"}', "not json", [], ["{}", "{}"], '{}'])("rejects disagreeing or invalid workaround %j", (explicit) => {
    const given = seed({ ...record, [bag]: explicit });
    expect(() => validateCase(caseWith({ given }))).toThrow(/disagrees.*seed custom_\* properties/);
    expect(() => runScript(identity + read, given)).toThrow(/disagrees.*seed custom_\* properties/);
  });
});
