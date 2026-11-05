/**
 * E15b: script-tester writes on a type whose profile declares hooks or policies fault, because
 * the lane runs neither. The profile records hook names without bodies. Every type is invented.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/run.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { normaliseManagedConfig } from "../../src/profile/normalise.ts";
import type { EnvProfile, ObjectSchema } from "../../src/profile/types.ts";
import { managedWriteFaults, type WriteMethod, writeFault } from "../../src/profile/write-guard.ts";
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
    } },
  }, { name: "bare" }] }, META);

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
    expect(JSON.stringify(profile)).not.toMatch(/BODY-|logger/);
  });
});

describe("writeFault decides per type and method", () => {
  const object = (extra: Partial<ObjectSchema>): ObjectSchema => ({ name: "gizmo", properties: {}, required: [], hooks: [], ...extra });
  const cases: [string, ObjectSchema, WriteMethod, string | undefined][] = [
    ["a stale profile cannot say", { name: "gizmo", properties: {}, required: [] }, "update", "openidm-managed-profile-stale"],
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
    ["read hooks count for delete", object({ hooks: ["onRetrieve"] }), "delete", "openidm-managed-hook/gizmo.onRetrieve"],
    ["an unknown hook counts everywhere", object({ hooks: ["onFrobnicate"] }), "delete", "openidm-managed-hook/gizmo.onFrobnicate"],
    ["required is a policy", object({ required: ["mail"] }), "create", "openidm-managed-policy/gizmo"],
    ["an enum is a policy", object({ properties: { state: { type: "string", enum: ["a"] } } }), "update", "openidm-managed-policy/gizmo"],
    ["an items enum is a policy", object({ properties: { tags: { type: "array", items: { type: "string", enum: ["a"] } } } }), "patch", "openidm-managed-policy/gizmo"],
    ["property policies", object({ properties: { code: { type: "string", policies: ["regexpMatches"] } } }), "create", "openidm-managed-policy/gizmo"],
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
    const table = managedWriteFaults({ ...META, sources: [], objects: { gizmo: object({ hooks: ["onDelete"] }), quiet: { ...object({}), name: "quiet" } } });
    expect(Object.keys(table)).toEqual(["managed/gizmo"]);
    expect(Object.keys(table["managed/gizmo"] ?? {})).toEqual(["delete"]);
  });
});

const lane: EnvProfile = { ...META, sources: [], objects: {
  hooked: { name: "hooked", properties: { mail: { type: "string" } }, required: [], hooks: ["onCreate", "onUpdate", "onDelete"] },
  quiet: { name: "quiet", properties: { mail: { type: "string" } }, required: [], hooks: [] },
} };
const seeded = { managed: { "managed/hooked": [{ _id: "h1", _rev: "1", mail: "a" }], "managed/quiet": [{ _id: "q1", _rev: "1", mail: "a" }] } };
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
