import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
import { defineCase } from "../../src/case/index.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { SchemaViolation } from "../../src/profile/validate.ts";
import type { EnvProfile } from "../../src/profile/types.ts";

const profile: EnvProfile = {
  tenant: "sandbox",
  pulledAt: "2026-09-12T00:00:00.000Z",
  sources: ["/openidm/config/managed"],
  objects: {
    alpha_user: {
      name: "alpha_user",
      properties: {
        _id: { type: "string" },
        userName: { type: "string" },
        mail: { type: "string", nullable: true },
      },
      required: ["userName"],
    },
    // Declared by the environment, seeded by nobody. The whole point.
    alpha_organization: { name: "alpha_organization", properties: {}, required: [] },
  },
};

/** Report what `openidm.read` did, without throwing out of the script. */
const probe = `
var out;
try {
  var rec = openidm.read(RESOURCE);
  out = { threw: false, value: rec === null ? "null" : "record" };
} catch (e) {
  out = { threw: true, error: String(e) };
}
nodeState.putShared("probe", JSON.stringify(out));
action.goTo("true");
`;

function readCase(name: string, resource: string, seeded: boolean) {
  return defineCase({
    name,
    script: probe.replace("RESOURCE", JSON.stringify(resource)),
    given: seeded
      ? { managed: { "managed/alpha_user": [{ _id: "alice", userName: "alice" }] } }
      : {},
    expect: { outcome: "true" },
  });
}

describe("environment profile through the JVM runner", () => {
  let runner: RhinoRunner;

  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);

  afterAll(async () => {
    await runner.close();
  }, 30_000);

  async function probeRead(kase: ReturnType<typeof readCase>, withProfile: boolean) {
    const run = await runCase(runner, kase, {
      timeoutMs: 5_000,
      ...(withProfile ? { profile } : {}),
    });
    return JSON.parse(String(run.effects.sharedState.final.probe)) as {
      threw: boolean;
      value?: string;
      error?: string;
    };
  }

  it("a declared type with no seeded record reads as null, like AIC", async () => {
    const result = await probeRead(
      readCase("declared-unseeded", "managed/alpha_organization/anything", false),
      true
    );
    expect(result).toEqual({ threw: false, value: "null" });
  });

  // A type the profile says the tenant does not have: emulated where AIC is
  // measured, a harness fault where it is not.
  async function probeCall(call: string) {
    const run = await runCase(
      runner,
      defineCase({
        name: "undeclared",
        script: probe.replace("openidm.read(RESOURCE)", call),
        given: {},
        expect: { outcome: "true" },
      }),
      { timeoutMs: 5_000, profile }
    );
    return JSON.parse(String(run.effects.sharedState.final.probe)) as {
      threw: boolean;
      value?: string;
      error?: string;
    };
  }

  it("a read of an UNdeclared type returns null, as AIC does (docs/api/10)", async () => {
    expect(await probeCall('openidm.read("managed/zzz_not_a_type/anything")')).toEqual({
      threw: false,
      value: "null",
    });
  });

  it("a query of an UNdeclared type throws AIC's catchable not-found (corpus query/2/absent-type)", async () => {
    expect(
      await probeCall('openidm.query("managed/zzz_not_a_type", { _queryFilter: "true" })')
    ).toEqual({
      threw: true,
      error: "Error: rhino-local: openidm.query: Resource &#39;managed/zzz_not_a_type&#39; not found",
    });
  });

  // Every write, create and action included (round-2 P2-2: those two used to
  // succeed, so a typo'd type passed locally). The record-level action would
  // otherwise be fixture/openidm-action, so it discriminates the check's order.
  it.each([
    ["update", 'openidm.update("managed/zzz_not_a_type/anything", null, {})'],
    ["patch", 'openidm.patch("managed/zzz_not_a_type/anything", null, [])'],
    ["delete", 'openidm.delete("managed/zzz_not_a_type/anything", null)'],
    ["create", 'openidm.create("managed/zzz_not_a_type", "a", { k: 1 })'],
    ["create", 'openidm.create("managed/zzz_not_a_type", null, { k: 1 })'],
    ["action", 'openidm.action("managed/zzz_not_a_type", "patch", [])'],
    ["action", 'openidm.action("managed/zzz_not_a_type", "nosuchaction", {})'],
    ["action", 'openidm.action("managed/zzz_not_a_type/anything", "custom", {})'],
  ])("an %s of an UNdeclared type is a harness fault naming the environment: unmeasured on AIC (%s)", async (method, call) => {
    expectHarnessFault("openidm-unknown-managed-type");
    await expect(probeCall(call)).rejects.toThrow(
      new RegExp(`harness fault openidm-unknown-managed-type: openidm\\.${method}: "managed\\/zzz_not_a_type" is not a managed object in environment "sandbox"`)
    );
  });

  it("without a profile, a declared-but-unseeded type is still a missing fixture", async () => {
    // Pre-profile behaviour must be untouched, or every existing case changes
    // meaning the day a profile is introduced.
    expectHarnessFault("fixture/managed");
    await expect(
      probeRead(readCase("no-profile", "managed/alpha_organization/anything", false), false)
    ).rejects.toThrow(
      /harness fault fixture\/managed: openidm\.read: no given\.managed entry for "managed\/alpha_organization"; seed it, or run with a profile/
    );
  });

  it("a seeded record still reads back with a profile present", async () => {
    const result = await probeRead(
      readCase("seeded-hit", "managed/alpha_user/alice", true),
      true
    );
    expect(result).toEqual({ threw: false, value: "record" });
  });

  it("a missing record in a SEEDED collection reads as null", async () => {
    const result = await probeRead(
      readCase("seeded-miss", "managed/alpha_user/nobody", true),
      true
    );
    expect(result).toEqual({ threw: false, value: "null" });
  });

  it("refuses a fixture naming a property the environment does not define", async () => {
    const kase = defineCase({
      name: "typo-fixture",
      script: probe.replace("RESOURCE", JSON.stringify("managed/alpha_user/alice")),
      given: { managed: { "managed/alpha_user": [{ _id: "alice", emai1: "x" }] } },
      expect: { outcome: "true" },
    });
    await expect(runCase(runner, kase, { timeoutMs: 5_000, profile })).rejects.toThrow(
      SchemaViolation
    );
    await expect(runCase(runner, kase, { timeoutMs: 5_000, profile })).rejects.toThrow(
      /"typo-fixture".*emai1/s
    );
  });

  it("accepts the same fixture when no profile is supplied", async () => {
    const kase = defineCase({
      name: "typo-fixture-unchecked",
      script: probe.replace("RESOURCE", JSON.stringify("managed/alpha_user/alice")),
      given: { managed: { "managed/alpha_user": [{ _id: "alice", emai1: "x" }] } },
      expect: { outcome: "true" },
    });
    // Without a profile the harness has no schema to check against, so the
    // same fixture runs. (The verdict still fails on the probe's undeclared
    // state write — that is the case's own expect, not the schema.)
    const run = await runCase(runner, kase, { timeoutMs: 5_000 });
    const probeOut = JSON.parse(String(run.effects.sharedState.final.probe)) as {
      threw: boolean;
      value?: string;
    };
    expect(probeOut).toEqual({ threw: false, value: "record" });
  });
});
