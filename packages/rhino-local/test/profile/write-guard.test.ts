/**
 * E15b: script-tester writes on a type whose profile declares hooks or policies fault, because
 * the lane runs neither. The profile records hook names without bodies. Every type is invented.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/run.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { normaliseManagedConfig } from "../../src/profile/normalise.ts";
import { type EnvProfile, type ObjectSchema, PROFILE_VERSION } from "../../src/profile/types.ts";
import { managedDefaults, managedWriteFaults, type WriteMethod, writeFault } from "../../src/profile/write-guard.ts";
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
    expect(managedDefaults(profile)).toEqual({});
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

describe("managedDefaults gives the lane plain lists (P2-2)", () => {
  const object = (extra: Partial<ObjectSchema>): ObjectSchema => ({ name: "gizmo", required: [], hooks: [], properties: {
    n: { type: "number", default: 0 }, b: { type: "boolean", default: false }, cleared: { type: "string", default: null }, plain: { type: "string" },
  }, ...extra });
  const rule = (extra: Partial<ObjectSchema>) =>
    managedDefaults({ ...META, profileVersion: PROFILE_VERSION, sources: [], objects: { gizmo: object(extra) } })["managed/gizmo"];
  it("non-null defaults are applied and refused as null; a null default is neither", () => {
    expect(rule({})).toEqual({ defaults: { n: 0, b: false }, notNull: ["n", "b"], emulable: ["n", "b"], defaulted: ["n", "b", "cleared"] });
  });
  // P3-1 (fix round 2): a null default is still a default to stock policy.js, so a type whose only
  // default is null still gets a rule; the lane faults a PATCH remove of it.
  it("a type whose only default is null still lists it as defaulted", () => {
    const profile: EnvProfile = { ...META, profileVersion: PROFILE_VERSION, sources: [], objects: { gizmo: { name: "gizmo", required: [], hooks: [],
      properties: { cleared: { type: "string", default: null }, plain: { type: "string" } } } } };
    expect(managedDefaults(profile)["managed/gizmo"]).toEqual({ defaults: {}, notNull: [], emulable: [], defaulted: ["cleared"] });
  });
  it("a required-named property is not emulable (AIC adds VALID_TYPE)", () => {
    expect(rule({ required: ["b"] })?.emulable).toEqual(["n"]);
  });
  it("no required array: nothing is emulable", () => {
    expect(rule({ noRequiredArray: true })?.emulable).toEqual([]);
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
    expect(fault?.message).toContain("runs onCreate, postCreate, secret.onStore");
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
} };
const seeded = { managed: { "managed/hooked": [{ _id: "h1", _rev: "1", mail: "a" }], "managed/quiet": [{ _id: "q1", _rev: "1", mail: "a" }],
  "managed/defaulted": [{ _id: "d1", _rev: "1", count: 1, flag: true, tags: [] }], "managed/loose": [] } };
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
    ["two nulls", "openidm.create('managed/defaulted', 'd2', {count: null, flag: null})"],
    ["an existing id", "openidm.create('managed/defaulted', 'd1', {count: null})"],
    ["an update", "openidm.update('managed/defaulted/d1', null, {count: null})"],
    ["no required array", "openidm.create('managed/loose', 'l1', {count: null})"],
  ])("a defaulted null on %s is unmeasured and faults", (_name, call) => {
    const type = call.includes("loose") ? "loose" : "defaulted";
    const run = runScriptWithFaults(catching(call), seeded, { profile: lane });
    expect(run.faults.map((fault) => fault.id)).toEqual([`openidm-managed-policy/${type}`]);
  });

  it("an explicit value is stored and an undefaulted null still writes", () => {
    const run = runScriptWithFaults("openidm.create('managed/defaulted', 'd2', {count: 7, mail: null});", seeded, { profile: lane });
    expect(run.faults).toEqual([]);
    expect(store(run, "defaulted")?.[1]).toMatchObject({ count: 7, mail: null, flag: false });
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
});
