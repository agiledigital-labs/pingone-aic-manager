// Round 7: host coverage helpers must never cross into Rhino via Function.toString().
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { RhinoRunner } from "../../src/runner.ts";
import { validateCase } from "../../src/case/validate.ts";
import { mockPreamble, withHarvest } from "../../src/bindings/preamble.ts";
import { parseHarvest } from "../../src/bindings/harvest.ts";
import { caseWith } from "../aic/helpers.ts";

const coverage = vi.hoisted(() => ({ calls: 0 }));
vi.mock("../../src/case/identity-policy.ts", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../src/case/identity-policy.ts")>();
  const { runInNewContext } = await import("node:vm");
  // The function body references a host-only counter, as Istanbul instrumentation does.
  function instrument(original: unknown): unknown {
    return runInNewContext(
      "(function(property) { cov_policyHost(); return original(property); })",
      {
        original,
        cov_policyHost: () => {
          coverage.calls += 1;
        },
      },
    );
  }
  const exports: Record<string, unknown> = {
    ...actual,
    identityPolicy: {
      ...actual.identityPolicy,
      collisionReason: instrument(actual.identityPolicy.collisionReason),
    },
  };
  // Exercise the old embedding on 5c726c8 as well as the canonical-asset loader.
  if (typeof exports.createIdentityPolicy === "function") {
    exports.createIdentityPolicy = instrument(exports.createIdentityPolicy);
  }
  return exports;
});

describe("host-instrumented identity policy", () => {
  let runner: RhinoRunner;
  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);
  afterAll(async () => {
    await runner.close();
  });

  it("runs host collision checks and initializes Rhino without host helpers", async () => {
    const resource = "managed/alpha_user/example";
    const managed = { "managed/alpha_user": [{ _id: "example" }] };
    const seed = { managed, identityCustomAttrs: { [resource]: [] } };
    expect(() =>
      validateCase({
        ...caseWith(),
        given: {
          ...seed,
          identityCustomAttrsOwnedKeys: { [resource]: ["givenName"] },
        },
      }),
    ).toThrow(/collides with/);
    expect(coverage.calls).toBeGreaterThan(0);

    const response = await runner.eval({
      preamble: mockPreamble(seed),
      source: withHarvest(`
        var identity = idRepository.getIdentity("example");
        identity.setAttribute("fr-idm-custom-attrs", ['{"plain":"stored"}']);
        identity.store();
      `),
    });
    expect(response.outcome, JSON.stringify(response)).toBe("ok");
    if (typeof response.value !== "string") throw new Error("missing harvest");
    const effects = parseHarvest(response.value);
    expect(effects.managedStore).toEqual({
      "managed/alpha_user": [{ _id: "example", plain: "stored" }],
    });
    expect(effects.identityCustomAttrs).toEqual({
      [resource]: ['{"plain":"stored"}'],
    });
  });
});
