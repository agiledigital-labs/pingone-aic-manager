// Regression: declared custom writes must survive harvest and a profile-backed
// carry, including when the previous AM bag was absent.
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/run.ts";
import type { Given } from "../../src/case/types.ts";
import { carryGiven } from "../../src/harness/step.ts";
import type { EnvProfile } from "../../src/profile/types.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { caseWith } from "../aic/helpers.ts";

const resource = "managed/alpha_user/example";
const profile: EnvProfile = {
  tenant: "sandbox",
  pulledAt: "2026-10-02T00:00:00Z",
  sources: ["/openidm/config/managed"],
  objects: {
    alpha_user: {
      name: "alpha_user",
      required: [],
      properties: {
        givenName: { type: "string", enum: ["Original"] },
        custom_example: { type: "string", enum: ["seeded"] },
      },
    },
  },
};
const given: Given = {
  managed: {
    "managed/alpha_user": [
      { _id: "example", givenName: "Original", custom_example: "seeded" },
    ],
  },
  identityAttributes: {
    alias: { field: "custom_example", cardinality: "single" },
  },
};

describe("declared custom writes across profile-backed Rhino passes", () => {
  let runner: RhinoRunner;
  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);
  afterAll(async () => {
    await runner.close();
  });

  it.each([
    { start: "seeded", prepare: "" },
    {
      start: "absent",
      prepare:
        'identity.setAttribute("fr-idm-custom-attrs", []); identity.store();',
    },
    {
      start: "bag-written",
      prepare:
        'identity.setAttribute("fr-idm-custom-attrs", [\'{"custom_example":false,"plain":"kept"}\']); identity.store();',
    },
  ])(
    "carries repeated declared writes from $start",
    async ({ start, prepare }) => {
      const text = JSON.stringify(
        start === "bag-written"
          ? { custom_example: "second", plain: "kept" }
          : { custom_example: "second" },
      );
      const first = await runCase(
        runner,
        caseWith({
          given,
          script:
            'var identity = idRepository.getIdentity("example");' +
            prepare +
            'identity.setAttribute("alias", ["first"]); identity.store();' +
            'identity.setAttribute("alias", ["second"]); identity.store(); action.goTo("true");',
          expect: {
            outcome: "true",
            allowUndeclared: { identityWrites: true },
            identityWrites: [
              { identity: "example", attribute: "alias", values: ["first"] },
              { identity: "example", attribute: "alias", values: ["second"] },
            ],
          },
        }),
        { profile },
      );
      expect(first.verdict.pass, first.verdict.summary).toBe(true);
      expect(first.effects.identityCustomAttrs).toEqual({ [resource]: [text] });
      const carried = carryGiven(given, first.effects, []);
      const second = await runCase(
        runner,
        caseWith({
          given: carried,
          script:
            'var identity = idRepository.getIdentity("example");' +
            'nodeState.putShared("alias", String(identity.getAttributeValues("alias").get(0)));' +
            'nodeState.putShared("bag", String(identity.getAttributeValues("fr-idm-custom-attrs").get(0))); action.goTo("true");',
          expect: {
            outcome: "true",
            sharedState: { added: { alias: "second", bag: text } },
          },
        }),
        { profile },
      );
      expect(second.verdict.pass, second.verdict.summary).toBe(true);
      expect(second.effects.managedStore).toEqual(first.effects.managedStore);
      expect(second.effects.identityCustomAttrs).toEqual({
        [resource]: [text],
      });
    },
  );
});
