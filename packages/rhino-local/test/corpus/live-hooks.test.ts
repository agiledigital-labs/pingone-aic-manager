/**
 * `liveHooks` (E15ab fix round 2, P2-2): a corpus case may name object hooks its live run exercised,
 * so the lane does not fault them for that case. Three constraints keep it from becoming a bypass,
 * and each is pinned here against the real `realCase` and `profileForRun`.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/run.ts";
import { defineCase } from "../../src/case/index.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { RhinoRunner } from "../../src/runner.ts";
import type { Given } from "../../src/case/types.ts";
import type { RunCaseOptions } from "../../src/bindings/run.ts";
import type { EnvProfile } from "../../src/profile/types.ts";
import { managedWriteFaults } from "../../src/profile/write-guard.ts";
import { realCases } from "../../cases/real/index.ts";
import { type LiveHooks, profileForRun, type RealEntry, realCase } from "../../cases/real/load.ts";

const writes = realCases.find((entry) => entry.kase.name === "binding-openidm-writes") as RealEntry;
const profile: EnvProfile = { profileVersion: 3, tenant: "sandbox", pulledAt: "2026-09-29T00:00:00.000Z", sources: [],
  objects: { alpha_role: { name: "alpha_role", properties: {}, required: [], hooks: ["onDelete", "postCreate", "onUpdate"] } } };
const live: LiveHooks = { probed: "2026-09-29", hooks: { alpha_role: ["postCreate", "onDelete"] } };
type Input = Parameters<typeof realCase>[0];
const build = (overrides: Partial<Input>, withProfile = true) => {
  const input: Input = { name: "live-hooks-probe", origin: writes.origin, expect: writes.kase.expect, liveHooks: live, ...overrides };
  if (withProfile) input.profile = overrides.profile ?? profile;
  return realCase(input);
};

describe("liveHooks is a corpus-only, profile-bound, live-anchored acknowledgement", () => {
  it("the committed writes case uses it, and its live run backs it", () => {
    expect(writes.liveHooks?.hooks).toEqual({ alpha_role: ["postCreate", "onDelete"] });
    // The case's own profile still states both hooks: the acknowledgement does not edit it.
    expect(writes.profile?.objects.alpha_role?.hooks).toEqual(["onDelete", "postCreate"]);
  });

  it("subtracts exactly the named hooks; any other hook of the profile still faults", () => {
    const run = profileForRun(build({}));
    expect(run?.objects.alpha_role?.hooks).toEqual(["onUpdate"]);
    const table = managedWriteFaults(run as EnvProfile);
    expect(table["managed/alpha_role"]?.create).toBeUndefined();
    expect(table["managed/alpha_role"]?.delete).toBeUndefined();
    expect(table["managed/alpha_role"]?.update?.id).toBe("openidm-managed-hook/alpha_role.onUpdate");
  });

  it("without liveHooks the profile is used as written", () => {
    const { liveHooks: _live, ...rest } = writes;
    expect(profileForRun(rest)).toBe(writes.profile);
  });

  // Constraint 1: typed on RealEntry only. If `Given` or `RunCaseOptions` ever gains the field,
  // these @ts-expect-error lines stop erroring and typecheck fails.
  it("is not a field of Given or RunCaseOptions", () => {
    // @ts-expect-error liveHooks is corpus-only
    const given: Given = { liveHooks: live };
    // @ts-expect-error liveHooks is corpus-only
    const options: RunCaseOptions = { liveHooks: live };
    expect([given, options]).toHaveLength(2);
  });

  // Constraint 2: it may only name hooks the case's own profile declares.
  it.each([
    ["a hook the type does not declare", { probed: "2026-09-29", hooks: { alpha_role: ["postUpdate"] } }, true, /alpha_role\.postUpdate, which the case's profile does not declare/],
    ["a type the profile does not have", { probed: "2026-09-29", hooks: { alpha_user: ["postCreate"] } }, true, /alpha_user\.postCreate/],
    ["no profile at all", live, false, /needs the case's own profile/],
  ] as const)("refuses %s", (_name, liveHooks, withProfile, message) => {
    expect(() => build({ liveHooks }, withProfile)).toThrow(message);
  });

  // Constraint 3: the case must carry a live payload.
  it("refuses a case with no HiddenValueCallback payload", () => {
    expect(() => build({ expect: { outcome: "true" } })).toThrow(/needs a live payload/);
  });
  it("refuses a blocked case", () => {
    expect(() => build({ blocked: { method: "openidm.create", throw: "x" } })).toThrow(/needs a live payload/);
  });
  it.each([["soon"], ["2026-13-45"], ["2026-02-30"]])("refuses probe date %s", (probed) => {
    expect(() => build({ liveHooks: { probed, hooks: live.hooks } })).toThrow(/names no probe date/);
  });
});

// sol-r1 P2-2: the corpus runner consumes profileForRun, and a RealEntry built by spreading the
// committed one never went through realCase. Each of these used to return the hook-free profile.
describe("profileForRun re-checks the acknowledgement on the entry it is given", () => {
  const spread: [string, RealEntry, RegExp][] = [
    ["no payload", { ...writes, kase: { ...writes.kase, expect: { outcome: "true" } } }, /needs a live payload/],
    ["a blocked entry", { ...writes, blocked: { method: "openidm.create", throw: "refused" } }, /needs a live payload/],
    ["an invalid date", { ...writes, liveHooks: { ...writes.liveHooks!, probed: "soon" } }, /names no probe date/],
    ["a hook the profile lacks", { ...writes, liveHooks: { ...writes.liveHooks!, hooks: { alpha_role: ["postUpdate"] } } }, /does not declare/],
    ["no profile", (({ profile: _p, ...rest }) => rest)(writes), /needs the case's own profile/],
  ];
  it.each(spread)("refuses %s", (_name, entry, message) => {
    expect(() => profileForRun(entry)).toThrow(message);
  });
  it("the committed entry, untouched, still gets its exemption (positive control)", () => {
    expect(profileForRun(writes)?.objects.alpha_role?.hooks).toEqual([]);
  });

  describe("on the JVM, the path the corpus runner takes", () => {
    let runner: RhinoRunner;
    beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
    afterAll(async () => { await runner.close(); });
    // The reviewer's repro: a create with no live payload and an invalid date passed through
    // runCase(profileForRun(entry)). Now no profile comes back to run with.
    const kase = defineCase({ name: "no-live-payload", script: 'openidm.create("managed/alpha_role", "g1", {}); action.goTo("true");',
      given: { managed: { "managed/alpha_role": [] } }, expect: { outcome: "true", allowUndeclared: { openidmWrites: true } } });
    const entry: RealEntry = { ...writes, liveHooks: { ...writes.liveHooks!, probed: "soon" }, kase };
    it("refuses before the run, where it used to pass", async () => {
      await expect(async () => runCase(runner, entry.kase, { profile: profileForRun(entry)! })).rejects.toThrow(/names no probe date/);
    });
    it("with the case's own profile the hook still faults (positive control)", async () => {
      expectHarnessFault("openidm-managed-hook/alpha_role.postCreate");
      await expect(runCase(runner, entry.kase, { profile: entry.profile! })).rejects.toThrow(/managed\/alpha_role: on AIC a create runs postCreate/);
    });
  });
});
