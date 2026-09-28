import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
import { defineCase } from "../../src/case/index.ts";
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

  it("an UNdeclared type still throws, naming the environment", async () => {
    // The discriminating pair: AIC returns null for both, which is exactly why
    // a fixture typo is invisible there and must not be invisible here.
    const result = await probeRead(
      readCase("undeclared", "managed/zzz_not_a_type/anything", false),
      true
    );
    expect(result.threw).toBe(true);
    expect(result.error).toContain("is not a managed object");
    expect(result.error).toContain("sandbox");
  });

  it("without a profile, a declared-but-unseeded type is still a missing fixture", async () => {
    // Pre-profile behaviour must be untouched, or every existing case changes
    // meaning the day a profile is introduced.
    const result = await probeRead(
      readCase("no-profile", "managed/alpha_organization/anything", false),
      false
    );
    expect(result.threw).toBe(true);
    expect(result.error).toContain("no given.managed entry");
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
