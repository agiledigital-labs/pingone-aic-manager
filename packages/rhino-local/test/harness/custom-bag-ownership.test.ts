// Round 5: an unprefixed key remains bag-owned after either kind of clear.
// Exercise the real JVM, harvest, callback carry and optional profile checks.
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/run.ts";
import type { Given } from "../../src/case/types.ts";
import { carryGiven } from "../../src/harness/step.ts";
import type { EnvProfile } from "../../src/profile/types.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { caseWith } from "../aic/helpers.ts";

const resource = "managed/alpha_user/example";
const record = { _id: "example", givenName: "Original" };
const given: Given = {
  managed: { "managed/alpha_user": [record] },
  identityAttributes: {
    plainAlias: { field: "plain", cardinality: "single" },
  },
};
const profile: EnvProfile = {
  tenant: "sandbox",
  pulledAt: "2026-10-02T00:00:00Z",
  sources: ["/openidm/config/managed"],
  objects: {
    alpha_user: {
      name: "alpha_user",
      required: [],
      properties: { givenName: { type: "string", enum: ["Original"] } },
    },
  },
};
const clears = [
  { name: "alias", script: 'identity.setAttribute("plainAlias", []);' },
  {
    name: "whole empty",
    script: 'identity.setAttribute("fr-idm-custom-attrs", ["{}"]);',
  },
  {
    name: "whole absent",
    script: 'identity.setAttribute("fr-idm-custom-attrs", []);',
  },
];

describe("historical custom bag ownership across JVM callback passes", () => {
  let runner: RhinoRunner;
  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);
  afterAll(async () => {
    await runner.close();
  });

  async function pass(
    input: Given,
    script: string,
    withProfile: boolean,
    expectedBag?: string,
  ) {
    const result = await runCase(
      runner,
      caseWith({
        given: input,
        script:
          'var identity = idRepository.getIdentity("example");' +
          script +
          'action.goTo("true");',
        expect: {
          outcome: "true",
          allowUndeclared: { identityWrites: true },
          ...(expectedBag === undefined
            ? {}
            : { sharedState: { added: { bag: expectedBag } } }),
        },
      }),
      withProfile ? { profile } : {},
    );
    expect(result.verdict.pass, result.verdict.summary).toBe(true);
    return { result, next: carryGiven(input, result.effects, []) };
  }

  async function clearThenRestore(clear: string, withProfile: boolean) {
    const seeded = await pass(
      given,
      'identity.setAttribute("fr-idm-custom-attrs", [\'{"plain":"old"}\']); identity.store();',
      withProfile,
    );
    const cleared = await pass(
      seeded.next,
      clear + "identity.store();",
      withProfile,
    );
    expect(cleared.result.effects.managedStore).toEqual(given.managed);
    const restored = await pass(
      cleared.next,
      'identity.setAttribute("plainAlias", ["restored"]); identity.store();',
      withProfile,
    );
    expect(restored.result.effects.managedStore).toEqual({
      "managed/alpha_user": [{ ...record, plain: "restored" }],
    });
    return restored;
  }

  it.each(clears)(
    "$name clear/re-add survives the following profile-backed pass",
    async ({ script }) => {
      const restored = await clearThenRestore(script, true);
      // On 7299ddd this pass rejects plain as an unknown ordinary property.
      const read = await pass(
        restored.next,
        'nodeState.putShared("bag", String(identity.getAttributeValues("fr-idm-custom-attrs").get(0)));',
        true,
        '{"plain":"restored"}',
      );
      expect(read.result.effects.sharedState.final.bag).toBe(
        '{"plain":"restored"}',
      );
      expect(read.result.effects.identityCustomAttrs).toEqual({
        [resource]: ['{"plain":"restored"}'],
      });
    },
  );

  it.each(clears)(
    "whole-bag removal after $name clear/re-add removes the restored property",
    async ({ script }) => {
      const restored = await clearThenRestore(script, false);
      const removed = await pass(
        restored.next,
        'identity.setAttribute("fr-idm-custom-attrs", []); identity.store();',
        false,
      );
      // Without a profile 7299ddd leaves the orphaned plain property behind.
      expect(removed.result.effects.managedStore).toEqual(given.managed);
      expect(removed.result.effects.identityCustomAttrs).toEqual({
        [resource]: [],
      });
    },
  );
});
