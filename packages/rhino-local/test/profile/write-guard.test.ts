/**
 * E15b: script-tester writes on a type whose profile declares hooks or policies fault, because
 * the lane runs neither. The profile records hook names without bodies. Every type is invented.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/run.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { normaliseManagedConfig } from "../../src/profile/normalise.ts";
import { type EnvProfile, type ObjectSchema, PROFILE_VERSION, type PropertySchema } from "../../src/profile/types.ts";
import { hooksRun, managedInvalidDefault, managedPolicy, managedWriteFaults, type WriteMethod, writeFault } from "../../src/profile/write-guard.ts";
import { parseProfile } from "../../src/profile/store.ts";
import { IdmConfig } from "../../src/idm/config.ts";
import { validateDeclarations } from "../../src/idm/declarations.ts";
import { Evidence } from "../../src/idm/provenance.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { caseWith } from "../aic/helpers.ts";
import { runScriptWithFaults } from "../bindings/load-behaviour.ts";

const META = { tenant: "sandbox", pulledAt: "2026-11-06T00:00:00.000Z", endpoint: "/openidm/config/managed" };
const body = (marker: string) => ({ type: "text/javascript", source: `/* ${marker} */ logger.info('x');` });

describe("the profile records hook names, never bodies", () => {
  const profile = normaliseManagedConfig({ objects: [{
    name: "gizmo",
    onCreate: body("BODY-ONE"), postUpdate: body("BODY-TWO"), onFrobnicate: body("BODY-THREE"),
    onNothing: "not a script", actions: { go: body("BODY-FOUR") },
    schema: { properties: {
      secret: { type: "string", onStore: body("BODY-FIVE"), onValidate: body("BODY-SIX") },
      code: { type: "string", policies: [{ policyId: "regexpMatches", params: { regexp: "^x" } }, { bare: true }] },
      plain: { type: "string" },
      count: { type: "number", default: 0 },
      cleared: { type: "string", default: null },
      gated: { type: "string",
        conditionalPolicies: [{ condition: { type: "text/javascript", source: "BODY-SEVEN" }, dependencies: ["plain"],
          policies: [{ policyId: "minimum-length" }] }, { condition: {} }],
        fallbackPolicies: [{ policyId: "maximum-length" }] },
    } },
  }, { name: "bare", schema: { required: [], properties: {} } }] }, META);

  it("stamps the format version, and records a missing required array", () => {
    expect(profile.profileVersion).toBe(PROFILE_VERSION);
    expect(profile.objects.gizmo?.noRequiredArray).toBe(true);
    expect(profile.objects.bare?.noRequiredArray).toBeUndefined();
  });

  it("records a default, null included, and conditional/fallback policy ids without conditions", () => {
    expect(profile.objects.gizmo?.properties.count).toEqual({ type: "number", default: 0 });
    expect(profile.objects.gizmo?.properties.cleared).toEqual({ type: "string", default: null });
    expect(profile.objects.gizmo?.properties.gated).toEqual({ type: "string",
      conditionalPolicies: ["minimum-length", "?"], fallbackPolicies: ["maximum-length"] });
  });

  it("lists object hooks sorted, including an unknown on…/post… name, and skips non-scripts", () => {
    expect(profile.objects.gizmo?.hooks).toEqual(["onCreate", "onFrobnicate", "postUpdate"]);
    expect(profile.objects.bare?.hooks).toEqual([]);
  });

  it("lists property hooks and policy ids, and leaves a plain property unchanged", () => {
    expect(profile.objects.gizmo?.properties.secret).toEqual({ type: "string", hooks: ["onStore", "onValidate"] });
    expect(profile.objects.gizmo?.properties.code).toEqual({ type: "string", policies: ["regexpMatches", "?"] });
    expect(profile.objects.gizmo?.properties.plain).toEqual({ type: "string" });
  });

  it("carries no script source", () => {
    expect(JSON.stringify(profile)).not.toMatch(/BODY-|logger|text\/javascript/);
  });
});

describe("staleness is the profile's format version (P3-5)", () => {
  const quiet: ObjectSchema = { name: "quiet", properties: { n: { type: "number", default: 0 } }, required: [], hooks: [] };
  // P3-4 (fix round 2): the discriminating rows are the strings "2" and "two" and the fraction 2.5,
  // which a numeric `< PROFILE_VERSION` comparison read as current.
  it.each([["absent", undefined], ["1", 1], ['"2"', "2"], ['"two"', "two"], ["2.5", 2.5], ["null", null]])("version %s faults every write on every type, with a hand-written remedy", (_name, version) => {
    const profile = { ...META, ...(version === undefined ? {} : { profileVersion: version }), sources: [], objects: { quiet } } as unknown as EnvProfile;
    const table = managedWriteFaults(profile);
    expect(Object.keys(table["managed/quiet"] ?? {})).toEqual(["create", "update", "patch", "delete"]);
    const message = table["managed/quiet"]?.create?.message ?? "";
    expect(table["managed/quiet"]?.create?.id).toBe("openidm-managed-profile-stale");
    expect(message).toContain("aic-script-tester-pull-profile");
    expect(message).toContain(`"profileVersion": ${PROFILE_VERSION}`);
    expect(message).toContain("noRequiredArray");
    expect(managedPolicy(profile)).toEqual({});
  });
  it.each([["2", 2], ["3", 3]])("integer version %s is current", (_name, version) => {
    expect(managedWriteFaults({ ...META, profileVersion: version, sources: [], objects: { quiet } })).toEqual({});
  });
  // Discriminating: the old sniff read an absent `hooks` as stale; a current profile that omits it declares none.
  it("a current profile whose type omits hooks declares none", () => {
    const { hooks: _hooks, ...bare } = quiet;
    expect(managedWriteFaults({ ...META, profileVersion: PROFILE_VERSION, sources: [], objects: { quiet: bare } })).toEqual({});
  });
});

describe("managedPolicy gives the lane plain lists (P2-2, sol-r1b)", () => {
  const object = (extra: Partial<ObjectSchema>): ObjectSchema => ({ name: "gizmo", required: [], hooks: [], properties: {
    n: { type: "number", default: 0 }, b: { type: "boolean", default: false }, cleared: { type: "string", default: null }, plain: { type: "string" },
    maybe: { type: "string", nullable: true }, tags: { type: "array", items: { type: "string" } }, box: { type: "object" },
    owner: { type: "relationship" },
  }, ...extra });
  const rule = (extra: Partial<ObjectSchema>) =>
    managedPolicy({ ...META, profileVersion: PROFILE_VERSION, sources: [], objects: { gizmo: object(extra) } })["managed/gizmo"];
  it("with a required array: only non-null defaults refuse a null; types split measured from unmeasured", () => {
    expect(rule({})).toEqual({ defaults: { n: 0, b: false, cleared: null }, notNull: ["n", "b"], nullRefused: ["n", "b"], nullUnmeasured: ["maybe"],
      checked: { cleared: "string", plain: "string", tags: "array", box: "object" }, unmeasured: { n: "number", b: "boolean" },
      itemTypes: { tags: "string" }, defaulted: ["n", "b", "cleared"] });
  });
  // Discriminating (measured 2026-11-08): the same type with no required key refuses every typed null.
  // A null default without a required array was not probed (the null-default probe used required: []).
  it("with no required array: every typed property refuses a null, a null default's and an unknown type's are unmeasured", () => {
    const loose = rule({ noRequiredArray: true });
    expect(loose?.nullRefused).toEqual(["n", "b", "plain", "tags", "box"]);
    expect(loose?.nullUnmeasured).toEqual(["cleared", "maybe", "owner"]);
  });
  // sol-r1c, zzemu_def probe D1/D2: `default: null` is applied (stored as null) and gives no NOT_NULL.
  // P3-1 (fix round 2): it is still a default to stock policy.js, so the lane faults a PATCH remove of it.
  it("a null default is applied as null, listed as defaulted, and is not a NOT_NULL", () => {
    const profile: EnvProfile = { ...META, profileVersion: PROFILE_VERSION, sources: [], objects: { gizmo: { name: "gizmo", required: [], hooks: [],
      properties: { cleared: { type: "string", default: null }, both: { type: "string", nullable: true, default: null } } } } };
    expect(managedPolicy(profile)["managed/gizmo"]).toEqual({ defaults: { cleared: null, both: null }, notNull: [], nullRefused: [],
      nullUnmeasured: [], checked: { cleared: "string", both: "string" }, unmeasured: {}, itemTypes: {}, defaulted: ["cleared", "both"] });
  });
  it("a type with nothing defaulted still gets a rule", () => {
    expect(managedPolicy({ ...META, profileVersion: PROFILE_VERSION, sources: [], objects: { gizmo: { name: "gizmo", required: [], hooks: [],
      properties: { plain: { type: "string" } } } } })["managed/gizmo"]?.checked).toEqual({ plain: "string" });
  });
});

describe("writeFault decides per type and method", () => {
  const object = (extra: Partial<ObjectSchema>): ObjectSchema => ({ name: "gizmo", properties: {}, required: [], hooks: [], ...extra });
  const cases: [string, ObjectSchema, WriteMethod, string | undefined][] = [
    ["nothing declared stays quiet", object({}), "create", undefined],
    ["onCreate on create", object({ hooks: ["onCreate"] }), "create", "openidm-managed-hook/gizmo.onCreate"],
    // Discriminating: a guard that faulted every hooked type for every method would fault here.
    ["onCreate does not run on update", object({ hooks: ["onCreate"] }), "update", undefined],
    ["onDelete does not run on create", object({ hooks: ["onDelete"] }), "create", undefined],
    ["onDelete on delete", object({ hooks: ["onDelete"] }), "delete", "openidm-managed-hook/gizmo.onDelete"],
    ["patch runs the update chain", object({ hooks: ["postUpdate"] }), "patch", "openidm-managed-hook/gizmo.postUpdate"],
    ["onSync runs on delete", object({ hooks: ["onSync"] }), "delete", "openidm-managed-hook/gizmo.onSync"],
    ["the chain's first hook names the fault", object({ hooks: ["onCreate", "postCreate"] }), "create", "openidm-managed-hook/gizmo.onCreate"],
    ["property onStore on create", object({ properties: { secret: { type: "string", hooks: ["onStore"] } } }), "create", "openidm-managed-hook/gizmo.secret.onStore"],
    ["property onStore not on delete", object({ properties: { secret: { type: "string", hooks: ["onStore"] } } }), "delete", undefined],
    // P3-1: an update or PATCH reads the record first; a create and a delete run no read hooks (doc 24).
    ["read hooks run on update", object({ hooks: ["onRetrieve"] }), "update", "openidm-managed-hook/gizmo.onRetrieve"],
    ["onRead runs on patch", object({ hooks: ["onRead"] }), "patch", "openidm-managed-hook/gizmo.onRead"],
    ["read hooks do not run on create", object({ hooks: ["onRetrieve", "onRead"] }), "create", undefined],
    ["read hooks do not run on delete", object({ hooks: ["onRetrieve"] }), "delete", undefined],
    ["a property onRetrieve runs on update", object({ properties: { p: { type: "string", hooks: ["onRetrieve"] } } }), "update", "openidm-managed-hook/gizmo.p.onRetrieve"],
    ["a property onRetrieve does not run on create", object({ properties: { p: { type: "string", hooks: ["onRetrieve"] } } }), "create", undefined],
    ["an unknown property hook counts everywhere", object({ properties: { p: { type: "string", hooks: ["onFrob"] } } }), "delete", "openidm-managed-hook/gizmo.p.onFrob"],
    ["an unknown hook counts everywhere", object({ hooks: ["onFrobnicate"] }), "delete", "openidm-managed-hook/gizmo.onFrobnicate"],
    ["required is a policy", object({ required: ["mail"] }), "create", "openidm-managed-policy/gizmo"],
    ["an enum is a policy", object({ properties: { state: { type: "string", enum: ["a"] } } }), "update", "openidm-managed-policy/gizmo"],
    ["an items enum is a policy", object({ properties: { tags: { type: "array", items: { type: "string", enum: ["a"] } } } }), "patch", "openidm-managed-policy/gizmo"],
    ["property policies", object({ properties: { code: { type: "string", policies: ["regexpMatches"] } } }), "create", "openidm-managed-policy/gizmo"],
    ["conditional policies (P3-6)", object({ properties: { code: { type: "string", conditionalPolicies: ["minimum-length"] } } }), "update", "openidm-managed-policy/gizmo"],
    ["fallback policies (P3-6)", object({ properties: { code: { type: "string", fallbackPolicies: ["maximum-length"] } } }), "create", "openidm-managed-policy/gizmo"],
    ["a default alone is not a write fault: the lane answers it", object({ properties: { n: { type: "number", default: 0 } } }), "create", undefined],
    ["policy does not judge a delete", object({ required: ["mail"] }), "delete", undefined],
    ["a hook outranks a policy", object({ hooks: ["onUpdate"], required: ["mail"] }), "update", "openidm-managed-hook/gizmo.onUpdate"],
  ];
  it.each(cases)("%s", (_name, schema, method, id) => {
    expect(writeFault(schema, method)?.id).toBe(id);
  });

  it("the message names every hook and the remedy", () => {
    const fault = writeFault(object({ hooks: ["onCreate", "postCreate"], properties: { secret: { type: "string", hooks: ["onStore"] } } }), "create");
    // Measured order (doc 24): property onStore runs before postCreate.
    expect(fault?.message).toContain("runs onCreate, secret.onStore, postCreate");
    expect(fault?.message).toContain("given.openidmFailures");
  });

  it("the lane table holds only types that fault", () => {
    const table = managedWriteFaults({ ...META, profileVersion: PROFILE_VERSION, sources: [], objects: { gizmo: object({ hooks: ["onDelete"] }), quiet: { ...object({}), name: "quiet" } } });
    expect(Object.keys(table)).toEqual(["managed/gizmo"]);
    expect(Object.keys(table["managed/gizmo"] ?? {})).toEqual(["delete"]);
  });
});

const lane: EnvProfile = { ...META, profileVersion: PROFILE_VERSION, sources: [], objects: {
  hooked: { name: "hooked", properties: { mail: { type: "string" } }, required: [], hooks: ["onCreate", "onUpdate", "onDelete"] },
  quiet: { name: "quiet", properties: { mail: { type: "string" } }, required: [], hooks: [] },
  defaulted: { name: "defaulted", properties: { mail: { type: "string" }, count: { type: "number", default: 0 },
    flag: { type: "boolean", default: false }, tags: { type: "array", items: { type: "string" }, default: ["a"] } }, required: [], hooks: [] },
  loose: { name: "loose", noRequiredArray: true, properties: { count: { type: "number", default: 0 } }, required: [], hooks: [] },
  nulldef: { name: "nulldef", properties: { cleared: { type: "string", default: null }, note: { type: "string" } }, required: [], hooks: [] },
  // sol-r1b: quiet's twin with no required key, and the probe's nested shapes (as a pull normalises them).
  bare: { name: "bare", noRequiredArray: true, properties: { mail: { type: "string" } }, required: [], hooks: [] },
  nested: { name: "nested", properties: { box: { type: "object" }, rows: { type: "array", items: { type: "object" } } }, required: [], hooks: [] },
} };
const seeded = { managed: { "managed/hooked": [{ _id: "h1", _rev: "1", mail: "a" }], "managed/quiet": [{ _id: "q1", _rev: "1", mail: "a" }],
  "managed/defaulted": [{ _id: "d1", _rev: "1", count: 1, flag: true, tags: [] }], "managed/loose": [],
  "managed/nulldef": [{ _id: "n1", _rev: "1", cleared: "x", note: "y" }] } };
const catching = (call: string) => `try { ${call} } catch (e) { nodeState.putShared("error", String(e)); }`;

describe("the lane faults writes that reach its store", () => {
  it.each([
    ["create", "openidm.create('managed/hooked', 'h2', {mail: 'b'})", "openidm-managed-hook/hooked.onCreate"],
    ["update", "openidm.update('managed/hooked/h1', null, {mail: 'b'})", "openidm-managed-hook/hooked.onUpdate"],
    ["patch", "openidm.patch('managed/hooked/h1', null, [{operation: 'replace', field: '/mail', value: 'b'}])", "openidm-managed-hook/hooked.onUpdate"],
    ["delete", "openidm.delete('managed/hooked/h1', null)", "openidm-managed-hook/hooked.onDelete"],
  ])("%s on a hooked type faults and leaves the store alone", (_method, call, id) => {
    const run = runScriptWithFaults(catching(call), seeded, { profile: lane });
    expect(run.faults.map((fault) => fault.id)).toEqual([id]);
    expect(run.effects.managedStore?.["managed/hooked"]).toEqual([{ _id: "h1", _rev: "1", mail: "a" }]);
  });

  it("a hookless type still writes", () => {
    const run = runScriptWithFaults("openidm.create('managed/quiet', 'q2', {mail: 'b'});", seeded, { profile: lane });
    expect(run.faults).toEqual([]);
    expect(run.effects.managedStore?.["managed/quiet"]).toHaveLength(2);
  });

  it("a write answered by a failure stub is the test's own claim and does not fault", () => {
    const run = runScriptWithFaults(catching("openidm.create('managed/hooked', 'h2', {mail: 'b'})"),
      { ...seeded, openidmFailures: [{ match: { method: "create", resource: "managed/hooked/h2", ordinal: 1 }, reply: { code: 403 } }] },
      { profile: lane });
    expect(run.faults).toEqual([]);
    expect(String(run.effects.sharedState.final.error)).toMatch(/JavaException:.*403/);
  });

  it("without a profile the lane cannot know, and writes as before", () => {
    const run = runScriptWithFaults("openidm.create('managed/hooked', 'h2', {mail: 'b'});", seeded);
    expect(run.faults).toEqual([]);
  });

  it("a create that would collide still faults first: AIC runs the create chain before the collision", () => {
    const run = runScriptWithFaults(catching("openidm.create('managed/hooked', 'h1', {mail: 'b'})"), seeded, { profile: lane });
    expect(run.faults.map((fault) => fault.id)).toEqual(["openidm-managed-hook/hooked.onCreate"]);
  });
});

describe("the lane answers defaulted properties (P2-2)", () => {
  const store = (run: ReturnType<typeof runScriptWithFaults>, type: string) => run.effects.managedStore?.[`managed/${type}`];
  it("a create omitting defaulted properties stores their defaults", () => {
    const run = runScriptWithFaults("openidm.create('managed/defaulted', 'd2', {mail: 'b', flag: true});", seeded, { profile: lane });
    expect(run.faults).toEqual([]);
    expect(store(run, "defaulted")?.[1]).toEqual({ _id: "d2", _rev: "local-rev", mail: "b", flag: true, count: 0, tags: ["a"] });
  });

  it.each([["null", "null"], ["undefined", "undefined"]])(
    "a create of a new record sending %s for one is the measured catchable 403, and stores nothing", (_name, literal) => {
      const run = runScriptWithFaults(
        `try { openidm.create('managed/defaulted', 'd2', {mail: 'b', count: ${literal}}); } ` +
        "catch (e) { nodeState.putShared('error', String(e)); nodeState.putShared('code', e.javaException.getCode()); }",
        seeded, { profile: lane });
      expect(run.faults).toEqual([]);
      expect(String(run.effects.sharedState.final.error)).toMatch(/^JavaException: .*ResourceExceptionScriptAdapter: Policy validation failed$/);
      expect(run.effects.sharedState.final.code).toBe(403);
      expect(store(run, "defaulted")).toHaveLength(1);
    });

  it("reading the detail is a fault: only that it names NOT_NULL is measured", () => {
    const run = runScriptWithFaults(catching("try { openidm.create('managed/defaulted', 'd2', {count: null}); } catch (e) { e.javaException.getDetail(); }"),
      seeded, { profile: lane });
    expect(run.faults.map((fault) => fault.id)).toEqual(["openidm-policy-detail"]);
  });

  it.each([
    ["an existing id", "openidm.create('managed/defaulted', 'd1', {count: null})"],
    ["an update", "openidm.update('managed/defaulted/d1', null, {count: null})"],
  ])("a defaulted null on %s is unmeasured and faults", (_name, call) => {
    const run = runScriptWithFaults(catching(call), seeded, { profile: lane });
    expect(run.faults.map((fault) => fault.id)).toEqual(["openidm-managed-policy/defaulted"]);
  });
  // sol-r1b: these faulted before. Measured 2026-11-08: with no required key a defaulted null is
  // NOT_NULL + VALID_TYPE, and any failing requirement refuses the write. The detail (and so the
  // order of two properties' entries) is behind getDetail(), which faults.
  it.each([
    ["two nulls", "defaulted", "openidm.create('managed/defaulted', 'd2', {count: null, flag: null})"],
    ["no required array", "loose", "openidm.create('managed/loose', 'l1', {count: null})"],
  ])("a defaulted null on %s is the measured catchable 403", (_name, type, call) => {
    const run = runScriptWithFaults(`try { ${call} } catch (e) { nodeState.putShared('code', e.javaException.getCode()); }`, seeded, { profile: lane });
    expect(run.faults).toEqual([]);
    expect(run.effects.sharedState.final.code).toBe(403);
    expect(store(run, type)).toEqual(seeded.managed[`managed/${type}` as keyof typeof seeded.managed]);
  });

  it("an explicit value is stored and an undefaulted null still writes", () => {
    const run = runScriptWithFaults("openidm.create('managed/defaulted', 'd2', {count: 7, mail: null});", seeded, { profile: lane });
    expect(run.faults).toEqual([]);
    expect(store(run, "defaulted")?.[1]).toMatchObject({ count: 7, mail: null, flag: false });
  });
});

describe("the lane answers the 2026-11-08 policy probe (sol-r1b P2-1, P2-3)", () => {
  const store = (run: ReturnType<typeof runScriptWithFaults>, type: string) => run.effects.managedStore?.[`managed/${type}`];
  const refused = (call: string) => runScriptWithFaults(`try { ${call} } catch (e) { nodeState.putShared('error', String(e)); ` +
    "nodeState.putShared('code', e.javaException.getCode()); }", seeded, { profile: lane });
  // P2-3, discriminating: the same null body against required: [] and against no required key.
  it("an undefaulted null with required: [] is stored as null", () => {
    const run = runScriptWithFaults("openidm.create('managed/quiet', 'q2', {mail: null});", seeded, { profile: lane });
    expect(run.faults).toEqual([]);
    expect(store(run, "quiet")?.[1]).toEqual({ _id: "q2", _rev: "local-rev", mail: null });
  });
  it("the same null with no required key is the catchable 403 and stores nothing", () => {
    const run = refused("openidm.create('managed/bare', 'b1', {mail: null});");
    expect(run.faults).toEqual([]);
    expect(String(run.effects.sharedState.final.error)).toMatch(/ResourceExceptionScriptAdapter: Policy validation failed$/);
    expect(run.effects.sharedState.final.code).toBe(403);
    expect(store(run, "bare") ?? []).toEqual([]);
  });
  it("reading that refusal's detail faults", () => {
    const run = runScriptWithFaults(catching("try { openidm.create('managed/bare', 'b1', {mail: null}); } catch (e) { e.javaException.getDetail(); }"),
      seeded, { profile: lane });
    expect(run.faults.map((fault) => fault.id)).toEqual(["openidm-policy-detail"]);
  });
  it("an optional remove writes and drops the key (measured 200)", () => {
    const run = runScriptWithFaults("openidm.patch('managed/quiet/q1', null, [{operation: 'remove', field: '/mail'}]);", seeded, { profile: lane });
    expect(run.faults).toEqual([]);
    expect(store(run, "quiet")).toEqual([{ _id: "q1", _rev: "1" }]);
  });
  it("an undefaulted null outside a create writes as on create (assumed)", () => {
    const run = runScriptWithFaults("openidm.update('managed/quiet/q1', null, {mail: null});", seeded, { profile: lane });
    expect(run.faults).toEqual([]);
    expect(store(run, "quiet")?.[0]).toMatchObject({ mail: null });
  });
  // P2-1: only the top-level type is enforced.
  it.each([
    ["a string sent as a number", "openidm.create('managed/quiet', 'q2', {mail: 5})", "quiet"],
    ["an object sent as a string", "openidm.create('managed/nested', 'x1', {box: 's'})", "nested"],
    ["an array sent as a string", "openidm.create('managed/nested', 'x1', {rows: 's'})", "nested"],
  ])("%s is the catchable 403 and stores nothing", (_name, call, type) => {
    const run = refused(call);
    expect(run.faults).toEqual([]);
    expect(run.effects.sharedState.final.code).toBe(403);
    expect(store(run, type)).toEqual(seeded.managed[`managed/${type}` as keyof typeof seeded.managed] ?? []);
  });
  // Sol's two rows (measured 201, stored as written).
  it.each([
    ["a number in a string[] nested in an object", { box: { tags: [2] } }],
    ["a string where items.properties.inner is a number", { rows: [{ inner: "bad" }] }],
  ])("%s is stored as written", (_name, content) => {
    const run = runScriptWithFaults(`openidm.create('managed/nested', 'x1', ${JSON.stringify(content)});`, seeded, { profile: lane });
    expect(run.faults).toEqual([]);
    expect(store(run, "nested")?.[0]).toEqual({ _id: "x1", _rev: "local-rev", ...content });
  });
  it.each([
    ["a top-level array item of the wrong type (only nested items were measured)", "openidm.create('managed/defaulted', 'd2', {tags: ['a', 2]})"],
    ["a mismatch against a declared number", "openidm.create('managed/defaulted', 'd2', {count: '7'})"],
    ["a mismatch on an update", "openidm.update('managed/quiet/q1', null, {mail: 5})"],
    ["a mismatch a PATCH writes", "openidm.patch('managed/quiet/q1', null, [{operation: 'replace', field: '/mail', value: 5}])"],
    ["a mismatch on a create over an existing id", "openidm.create('managed/quiet', 'q1', {mail: 5})"],
  ])("%s is unmeasured and faults", (_name, call) => {
    const run = runScriptWithFaults(catching(call), seeded, { profile: lane });
    expect(run.faults.map((fault) => fault.id)).toEqual([`openidm-managed-policy/${call.includes("defaulted") ? "defaulted" : "quiet"}`]);
  });
  it("a PATCH writing inside a property is not checked", () => {
    const run = runScriptWithFaults("openidm.patch('managed/nested/x1', null, [{operation: 'add', field: '/box/tags', value: [2]}]);",
      { managed: { ...seeded.managed, "managed/nested": [{ _id: "x1", _rev: "1", box: {} }] } }, { profile: lane });
    expect(run.faults).toEqual([]);
  });
});

describe("the lane's PATCH null refusals (P2-1)", () => {
  const replaceNull = (type: string, id: string) =>
    catching(`openidm.patch('managed/${type}/${id}', null, [{operation: 'replace', field: '/mail', value: null}])`);
  it.each([
    // Discriminating: a hookless type used to store the null and pass.
    ["a hookless type", "quiet", "q1"],
    // Parse-time, so it answers before the hooked type's write fault.
    ["a hooked type", "hooked", "h1"],
  ])("replace with null on %s is AIC's catchable invalid-patch refusal and writes nothing", (_name, type, id) => {
    const run = runScriptWithFaults(replaceNull(type, id), seeded, { profile: lane });
    expect(run.faults).toEqual([]);
    expect(String(run.effects.sharedState.final.error)).toContain("not a valid JSON patch");
    expect(run.effects.managedStore?.[`managed/${type}`]).toEqual([{ _id: id, _rev: "1", mail: "a" }]);
  });

  it("add with null is unmeasured and faults", () => {
    const run = runScriptWithFaults(catching("openidm.patch('managed/quiet/q1', null, [{operation: 'add', field: '/mail', value: null}])"),
      seeded, { profile: lane });
    expect(run.faults.map((fault) => fault.id)).toEqual(["openidm-patch-add-null"]);
  });

  // Fix round 2, P2-1. Discriminating: each of these passed and deleted `mail` at a825992. AIC sends
  // a script's undefined as null (docs/api/12), and an operation with no `value` key is the same null
  // to the patch parser (inference; the emulator's REST path answers it 400 too).
  it.each([
    ["value: undefined", "{operation: 'replace', field: '/mail', value: undefined}"],
    ["no value key", "{operation: 'replace', field: '/mail'}"],
  ])("replace with %s is the same catchable invalid-patch refusal and writes nothing", (_name, op) => {
    const run = runScriptWithFaults(catching(`openidm.patch('managed/quiet/q1', null, [${op}])`), seeded, { profile: lane });
    expect(run.faults).toEqual([]);
    expect(String(run.effects.sharedState.final.error)).toContain("not a valid JSON patch");
    expect(run.effects.managedStore?.["managed/quiet"]).toEqual([{ _id: "q1", _rev: "1", mail: "a" }]);
  });
  it.each([
    ["value: undefined", "{operation: 'add', field: '/mail', value: undefined}"],
    ["no value key", "{operation: 'add', field: '/mail'}"],
  ])("add with %s faults like add with null", (_name, op) => {
    const run = runScriptWithFaults(catching(`openidm.patch('managed/quiet/q1', null, [${op}])`), seeded, { profile: lane });
    expect(run.faults.map((fault) => fault.id)).toEqual(["openidm-patch-add-null"]);
    expect(run.effects.managedStore?.["managed/quiet"]).toEqual([{ _id: "q1", _rev: "1", mail: "a" }]);
  });

  // Fix round 2, P3-2. Discriminating: at a825992 the record lookup and revision check ran first, so
  // these answered "No Such Entry" and the version error.
  it.each([
    ["an absent record", "openidm.patch('managed/quiet/absent', null, [{operation: 'replace', field: '/mail', value: null}])"],
    ["a stale revision", "openidm.patch('managed/quiet/q1', '9', [{operation: 'replace', field: '/mail', value: null}])"],
    ["an unknown operation on an absent record", "openidm.patch('managed/quiet/absent', null, [{operation: 'move', field: '/mail'}])"],
  ])("the patch is judged before %s", (_name, call) => {
    const run = runScriptWithFaults(catching(call), seeded, { profile: lane });
    expect(run.faults).toEqual([]);
    expect(String(run.effects.sharedState.final.error)).toContain("not a valid JSON patch");
  });
  it("a well-formed patch on an absent record is still No Such Entry (positive control)", () => {
    const run = runScriptWithFaults(catching("openidm.patch('managed/quiet/absent', null, [{operation: 'replace', field: '/mail', value: 'b'}])"),
      seeded, { profile: lane });
    expect(String(run.effects.sharedState.final.error)).toContain("No Such Entry");
  });
});

describe("the lane's PATCH remove of a defaulted property (fix round 2, P3-1)", () => {
  // Stock policy.js adds an implicit required for any default; AIC's 403 REQUIRED was measured only
  // with the property also in schema.required. Unmeasured either way, so it faults.
  // sol-r1c: `default: null` is a valid declaration (zzemu_def probe D1), so it reaches this check
  // again, as before sol-r1. Removing it is unmeasured for a null default as for a non-null one.
  it.each([
    ["a non-null default", "defaulted", "d1", "/count"],
    // Discriminating: a null default gives the type no NOT_NULL, and is still a default.
    ["a null default", "nulldef", "n1", "/cleared"],
  ])("removing a property with %s faults and writes nothing", (_name, type, id, field) => {
    const run = runScriptWithFaults(catching(`openidm.patch('managed/${type}/${id}', null, [{operation: 'remove', field: '${field}'}])`),
      seeded, { profile: lane });
    expect(run.faults.map((fault) => fault.id)).toEqual([`openidm-managed-policy/${type}`]);
    expect(run.effects.managedStore?.[`managed/${type}`]).toEqual(seeded.managed[`managed/${type}` as keyof typeof seeded.managed]);
  });
  it("removing an undefaulted property of a null-defaulted type still writes", () => {
    const run = runScriptWithFaults("openidm.patch('managed/nulldef/n1', null, [{operation: 'remove', field: '/note'}]);", seeded, { profile: lane });
    expect(run.faults).toEqual([]);
    expect(run.effects.managedStore?.["managed/nulldef"]).toEqual([{ _id: "n1", _rev: "1", cleared: "x" }]);
  });
  it("removing an undefaulted property of the same type still writes", () => {
    const run = runScriptWithFaults("openidm.patch('managed/defaulted/d1', null, [{operation: 'add', field: '/mail', value: 'm'}, " +
      "{operation: 'remove', field: '/mail'}]);", seeded, { profile: lane });
    expect(run.faults).toEqual([]);
    expect(run.effects.managedStore?.["managed/defaulted"]).toEqual([{ _id: "d1", _rev: "1", count: 1, flag: true, tags: [] }]);
  });
});

describe("hooksRun follows the measured phase order (sol-r1 P3-1)", () => {
  // Doc 24 "Managed-object hooks / Order". Discriminating: the reviewer's mixed type. The old list put
  // every write hook before every read hook, so its first hook (the fault id) was onUpdate.
  const reviewed: ObjectSchema = { name: "gizmo", required: [], hooks: ["onRead", "onRetrieve", "onUpdate", "postUpdate"],
    properties: { p: { type: "string", hooks: ["onRetrieve", "onStore"] } } };
  it("an update reads first: object and property retrieval, then onRead, then the write chain", () => {
    expect(hooksRun(reviewed, "update")).toEqual(["onRetrieve", "p.onRetrieve", "onRead", "onUpdate", "p.onStore", "postUpdate"]);
    expect(writeFault(reviewed, "patch")?.id).toBe("openidm-managed-hook/gizmo.onRetrieve");
  });
  // Every property's onValidate before the object's, every property's onStore before the object's.
  const full: ObjectSchema = { name: "gizmo", required: [],
    hooks: ["onCreate", "onDelete", "onRead", "onRetrieve", "onStore", "onSync", "onUpdate", "onValidate", "postCreate", "postDelete", "postUpdate"],
    properties: { a: { type: "string", hooks: ["onRetrieve", "onStore", "onValidate"] }, b: { type: "string", hooks: ["onValidate"] } } };
  it.each([
    ["create", ["onCreate", "a.onValidate", "b.onValidate", "onValidate", "a.onStore", "onStore", "postCreate", "onSync"]],
    ["update", ["onRetrieve", "a.onRetrieve", "onRead", "onUpdate", "a.onValidate", "b.onValidate", "onValidate", "a.onStore", "onStore", "postUpdate", "onSync"]],
    ["patch", ["onRetrieve", "a.onRetrieve", "onRead", "onUpdate", "a.onValidate", "b.onValidate", "onValidate", "a.onStore", "onStore", "postUpdate", "onSync"]],
    ["delete", ["onDelete", "postDelete", "onSync"]],
  ] as const)("%s", (method, order) => {
    expect(hooksRun(full, method)).toEqual(order);
  });
});

describe("one name per type: map key and object.name must agree (sol-r1 P1-2)", () => {
  // The reviewer's profile: existence says managed/gizmo, the object calls itself "other".
  const aliased = (version?: number): EnvProfile => ({ tenant: "sandbox", pulledAt: META.pulledAt, sources: [],
    ...(version === undefined ? {} : { profileVersion: version }),
    objects: { gizmo: { name: "other", required: [], properties: { n: { type: "number", default: 0 } }, hooks: ["onCreate", "onUpdate", "onDelete"] } } });
  const writes = "openidm.create('managed/gizmo', 'g1', {}); openidm.update('managed/gizmo/g1', null, {}); " +
    "openidm.patch('managed/gizmo/g1', null, []); openidm.delete('managed/gizmo/g1', null);";
  it.each([["absent (stale)", undefined], ["current", PROFILE_VERSION]])("parseProfile refuses version %s", (_name, version) => {
    expect(() => parseProfile(JSON.stringify(aliased(version)), "/x/profile.json")).toThrow(/objects\["gizmo"\]\.name is "other", not "gizmo"/);
  });
  it("parseProfile refuses an entry that is not an object, and accepts a consistent profile", () => {
    expect(() => parseProfile(JSON.stringify({ ...aliased(2), objects: { gizmo: 7 } }), "/x")).toThrow(/objects\["gizmo"\]\.name is missing/);
    // `objects` is a map keyed by name; an array keys by index, which no pull writes.
    expect(() => parseProfile(JSON.stringify({ ...aliased(2), objects: [] }), "/x")).toThrow(/`objects` is not an object/);
    expect(parseProfile(JSON.stringify(lane), "/x").objects.hooked?.name).toBe("hooked");
  });
  // A runner caller can pass a profile object directly; the preamble refuses it there too.
  it.each([["absent (stale)", undefined], ["current", PROFILE_VERSION]])("the preamble refuses a version-%s profile before any write runs", (_name, version) => {
    expect(() => runScriptWithFaults(writes, {}, { profile: aliased(version) })).toThrow(/objects\["gizmo"\]\.name is "other"/);
  });
  // Independent of the refusal: every table is keyed by the map key, so none can name another type.
  it("the tables key by the map key", () => {
    expect(Object.keys(managedWriteFaults(aliased()))).toEqual(["managed/gizmo"]);
    expect(managedWriteFaults(aliased(2))["managed/gizmo"]?.create?.id).toBe("openidm-managed-hook/gizmo.onCreate");
    expect(Object.keys(managedPolicy(aliased(2)))).toEqual(["managed/gizmo"]);
    const bad = aliased(2);
    bad.objects.gizmo!.properties.flag = { type: "boolean", default: "nope" };
    expect(managedInvalidDefault(bad)?.id).toBe("openidm-managed-invalid-default/gizmo.flag");
  });
});

describe("an invalid default anywhere in the profile faults every managed call (sol-r1 P1-1, sol-r1c)", () => {
  const config = (flag: unknown) => ({ objects: [{ name: "gizmo", schema: { required: [], properties: { flag, note: { type: "string" } } } }] });
  const profileOf = (flag: unknown) => normaliseManagedConfig(config(flag), META);
  type Verdict = "valid" | "invalid" | "unmeasured";
  /** What the IDM emulator's config check says about the same raw config. */
  const idmVerdict = async (flag: unknown): Promise<Verdict> => {
    const evidence = new Evidence();
    try {
      await evidence.run(async () => validateDeclarations(new IdmConfig({ sync: { mappings: [] }, managed: config(flag) } as never), evidence));
      return "valid";
    } catch (error) {
      const text = String(error);
      if (/nested property behavior default/.test(text)) return "unmeasured";
      const match = /openidm-managed-(invalid|unmeasured)-default\/gizmo\.flag: (invalid|unmeasured) default: gizmo\/flag/.exec(text);
      if (!match) throw error;
      return match[1] as Verdict;
    }
  };
  const laneVerdict = (flag: unknown): Verdict => {
    const id = managedInvalidDefault(profileOf(flag))?.id;
    if (id === undefined) return "valid";
    const match = /^openidm-managed-(invalid|unmeasured)-default\/gizmo\.flag$/.exec(id);
    if (!match) throw new Error(`unexpected id ${id}`);
    return match[1] as Verdict;
  };
  // Both lanes judge the same raw declaration with the same function, so they must agree on every row.
  // The rows marked measured are the zzemu_def probe (2026-11-08) or doc 10's boolean "nope".
  it.each([
    ["measured: boolean, default \"nope\" (D5)", { type: "boolean", default: "nope" }, "invalid"],
    ["measured: boolean true (the control)", { type: "boolean", default: true }, "valid"],
    ["boolean false", { type: "boolean", default: false }, "valid"],
    ["number 0", { type: "number", default: 0 }, "valid"],
    ["number as string", { type: "number", default: "0" }, "invalid"],
    ["string", { type: "string", default: "x" }, "valid"],
    // Discriminating for sol-r1's null rule, which called this invalid.
    ["measured: null on a string (D1)", { type: "string", default: null }, "valid"],
    ["null on a boolean (only string was probed)", { type: "boolean", default: null }, "unmeasured"],
    ["measured: array of strings (D3 control)", { type: "array", items: { type: "string" }, default: ["a", "b"] }, "valid"],
    // Discriminating for the item check: the outer value is an array, an item is not a string.
    ["measured: array with a wrong-typed item (D3)", { type: "array", items: { type: "string" }, default: ["a", 2] }, "invalid"],
    // Discriminating: a null item is neither a match nor a measured mismatch.
    ["a null item in an array default (not probed)", { type: "array", items: { type: "string" }, default: ["a", null] }, "unmeasured"],
    ["an array of arrays (not probed)", { type: "array", items: { type: "array" }, default: [["a"]] }, "unmeasured"],
    // Discriminating for the object rule: the old check recursed into properties and called this invalid.
    ["measured: object with a nested mismatch (D4)", { type: "object", properties: { x: { type: "string" } }, default: { x: 1 } }, "valid"],
    ["object as a string", { type: "object", default: "s" }, "invalid"],
    ["a relationship default (no JSON shape here)", { type: "relationship", default: { _ref: "managed/x/1" } }, "unmeasured"],
    ["a default nested inside items", { type: "array", items: { type: "string", default: "a" } }, "unmeasured"],
    ["no default", { type: "boolean" }, "valid"],
  ] as const)("%s", async (_name, flag, verdict) => {
    expect(await idmVerdict(flag)).toBe(verdict);
    expect(laneVerdict(flag)).toBe(verdict);
  });
  // The IDM emulator refuses a type list outright (a shape it does not take), so this row is the lane's alone.
  it("measured: a nullable string with default: null (D2) is valid in the lane", () => {
    expect(laneVerdict({ type: ["string", "null"], default: null })).toBe("valid");
  });

  // The lane: every call faults, on the bad type, on a valid sibling, and on a type the profile lacks.
  const mixed = normaliseManagedConfig({ objects: [...config({ type: "boolean", default: "nope" }).objects,
    { name: "fine", schema: { required: [], properties: { flag: { type: "boolean", default: false } } } }] }, META);
  const given = { managed: { "managed/gizmo": [{ _id: "g0", _rev: "1", note: "a" }], "managed/fine": [{ _id: "f0", _rev: "1", flag: true }] } };
  it.each([
    ["create omitting the property", "openidm.create('managed/gizmo', 'g1', {})"],
    ["create with a valid explicit value", "openidm.create('managed/gizmo', 'g1', {flag: true})"],
    ["read", "openidm.read('managed/gizmo/g0')"],
    ["query", "openidm.query('managed/gizmo', {_queryFilter: 'true'})"],
    ["update", "openidm.update('managed/gizmo/g0', null, {note: 'b', flag: true})"],
    ["patch", "openidm.patch('managed/gizmo/g0', null, [{operation: 'replace', field: '/note', value: 'b'}])"],
    ["delete", "openidm.delete('managed/gizmo/g0', null)"],
    // Discriminating for sol-r1c: sol-r1 faulted only the bad type, and these wrote or answered.
    ["a valid sibling's create", "openidm.create('managed/fine', 'f1', {})"],
    ["a valid sibling's read", "openidm.read('managed/fine/f0')"],
    ["a valid sibling's patch", "openidm.patch('managed/fine/f0', null, [{operation: 'replace', field: '/flag', value: false}])"],
    ["a type the profile does not declare", "openidm.query('managed/absent', {_queryFilter: 'true'})"],
  ])("%s faults and leaves the store alone", (_name, call) => {
    const run = runScriptWithFaults(catching(call), given, { profile: mixed });
    expect(run.faults.map((fault) => fault.id)).toEqual(["openidm-managed-invalid-default/gizmo.flag"]);
    expect(run.effects.managedStore).toEqual(given.managed);
  });
  it("the profile is not repaired: the default is still in it", () => {
    expect(mixed.objects.gizmo?.properties.flag?.default).toBe("nope");
  });
  it("an unmeasured default faults the lane under its own id", () => {
    const run = runScriptWithFaults(catching("openidm.read('managed/gizmo/g0')"), given, { profile: profileOf({ type: "boolean", default: null }) });
    expect(run.faults.map((fault) => fault.id)).toEqual(["openidm-managed-unmeasured-default/gizmo.flag"]);
  });
  // Positive control: the same two types with the default fixed write as before.
  it("with every default valid, the sibling writes and its default lands", () => {
    const fixed = normaliseManagedConfig({ objects: [...config({ type: "boolean", default: true }).objects,
      { name: "fine", schema: { required: [], properties: { flag: { type: "boolean", default: false } } } }] }, META);
    const run = runScriptWithFaults("openidm.create('managed/fine', 'f1', {});", given, { profile: fixed });
    expect(run.faults).toEqual([]);
    expect(run.effects.managedStore?.["managed/fine"]?.[1]).toEqual({ _id: "f1", _rev: "local-rev", flag: false });
  });
});

describe("the lane answers a null default (sol-r1c, zzemu_def probe D1/D2)", () => {
  const profile = (text: PropertySchema, extra: Partial<ObjectSchema> = {}): EnvProfile => ({ ...META, profileVersion: PROFILE_VERSION, sources: [],
    objects: { gizmo: { name: "gizmo", required: [], hooks: [], properties: { text, note: { type: "string" } }, ...extra } } });
  const store = (run: ReturnType<typeof runScriptWithFaults>) => run.effects.managedStore?.["managed/gizmo"];
  const nullDefault = profile({ type: "string", default: null });
  it("a create omitting it stores text: null (present, not absent)", () => {
    const run = runScriptWithFaults("openidm.create('managed/gizmo', 'g1', {note: 'a'});", {}, { profile: nullDefault });
    expect(run.faults).toEqual([]);
    expect(store(run)).toEqual([{ _id: "g1", _rev: "local-rev", note: "a", text: null }]);
  });
  // Discriminating: the same body against default: null (201) and against default: "x" (403 NOT_NULL).
  it.each([
    ["default: null", { type: "string", default: null }, "stored"],
    ["a nullable string with default: null", { type: "string", nullable: true, default: null }, "stored"],
    ["default: \"x\"", { type: "string", default: "x" }, 403],
  ] as const)("{text: null} against %s", (_name, text, outcome) => {
    const run = runScriptWithFaults("try { openidm.create('managed/gizmo', 'g1', {text: null}); } " +
      "catch (e) { nodeState.putShared('code', e.javaException.getCode()); }", {}, { profile: profile(text) });
    expect(run.faults).toEqual([]);
    if (outcome === "stored") expect(store(run)).toEqual([{ _id: "g1", _rev: "local-rev", text: null }]);
    else {
      expect(run.effects.sharedState.final.code).toBe(403);
      expect(store(run) ?? []).toEqual([]);
    }
  });
  it("a nullable string with default: null refuses a number (D2, measured 403)", () => {
    const run = runScriptWithFaults("try { openidm.create('managed/gizmo', 'g1', {text: 5}); } " +
      "catch (e) { nodeState.putShared('code', e.javaException.getCode()); }", {}, { profile: profile({ type: "string", nullable: true, default: null }) });
    expect(run.faults).toEqual([]);
    expect(run.effects.sharedState.final.code).toBe(403);
  });
  // Unmeasured: the null-default probe used required: [] throughout.
  it.each([
    ["omitting it", "openidm.create('managed/gizmo', 'g1', {note: 'a'})"],
    ["sending null", "openidm.create('managed/gizmo', 'g1', {text: null})"],
  ])("with no required array, %s faults", (_name, call) => {
    const run = runScriptWithFaults(catching(call), {}, { profile: profile({ type: "string", default: null }, { noRequiredArray: true }) });
    expect(run.faults.map((fault) => fault.id)).toEqual(["openidm-managed-policy/gizmo"]);
    expect(store(run) ?? []).toEqual([]);
  });
});

describe("on the real JVM the fault fails the run and is tallied", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner.close(); });

  it("a caught hooked create is still the harness fault", async () => {
    expectHarnessFault("openidm-managed-hook/hooked.onCreate");
    await expect(runCase(runner, caseWith({
      given: seeded,
      script: `${catching("openidm.create('managed/hooked', 'h2', {mail: 'b'})")} action.goTo("true");`,
      expect: { outcome: "true", allowUndeclared: { openidmWrites: true } },
    }), { profile: lane })).rejects.toThrow(/openidm-managed-hook\/hooked\.onCreate/);
  });

  // The reviewer's JVM repros, through the public runCase.
  it("a create on a type with a mismatched default fails the run (was pass: true, flag \"nope\" stored)", async () => {
    expectHarnessFault("openidm-managed-invalid-default/gizmo.flag");
    const bad = normaliseManagedConfig({ objects: [{ name: "gizmo", schema: { required: [], properties: { flag: { type: "boolean", default: "nope" } } } }] }, META);
    await expect(runCase(runner, caseWith({
      // Caught, as a probe would: the fault still fails the run.
      script: 'var r = {}; try { r = openidm.create("managed/gizmo", "g1", {}); } catch (e) {} action.goTo(r.flag === "nope" ? "true" : "false");',
      expect: { outcome: "true", allowUndeclared: { openidmWrites: true } },
    }), { profile: bad })).rejects.toThrow(/openidm-managed-invalid-default\/gizmo\.flag/);
  });
  it("a profile whose key and name disagree is refused (was pass: true across four writes)", async () => {
    const aliased: EnvProfile = { tenant: "sandbox", pulledAt: META.pulledAt, sources: [],
      objects: { gizmo: { name: "other", properties: {}, required: [], hooks: ["onCreate", "onUpdate", "onDelete"] } } };
    await expect(runCase(runner, caseWith({
      script: 'openidm.create("managed/gizmo", "g1", {}); openidm.delete("managed/gizmo/g1", null); action.goTo("true");',
      expect: { outcome: "true", allowUndeclared: { openidmWrites: true } },
    }), { profile: aliased })).rejects.toThrow(/objects\["gizmo"\]\.name is "other"/);
  });
});
