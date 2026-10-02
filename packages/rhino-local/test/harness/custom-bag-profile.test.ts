import { localIdmHandle } from "../../src/harness/idm.ts";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/run.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { carryGiven } from "../../src/harness/step.ts";
import { caseWith } from "../aic/helpers.ts";
import type { EnvProfile } from "../../src/profile/types.ts";

const resource = "managed/alpha_user/example";
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
const given = {
  managed: {
    "managed/alpha_user": [{ _id: "example", givenName: "Original" }],
  },
};

describe("bag provenance through profile-backed Rhino passes (review #4)", () => {
  let runner: RhinoRunner;
  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);
  afterAll(async () => {
    await runner.close();
  });

  it.each(['{"custom_unknown":"x","plain":"y"}', '"x"', "null", "[]"])(
    "carries %s and the same profile into another runCase",
    async (text) => {
      const first = await runCase(
        runner,
        caseWith({
          given,
          script: `var identity = idRepository.getIdentity("example"); identity.setAttribute("fr-idm-custom-attrs", [${JSON.stringify(text)}]); identity.store(); action.goTo("true");`,
          expect: {
            outcome: "true",
            identityWrites: [
              {
                identity: "example",
                attribute: "fr-idm-custom-attrs",
                values: [text],
              },
            ],
          },
        }),
        { profile },
      );
      expect(first.verdict.pass, first.verdict.summary).toBe(true);
      const next = carryGiven(given, first.effects, []);
      const second = await runCase(
        runner,
        caseWith({
          given: next,
          script:
            'var identity = idRepository.getIdentity("example"); nodeState.putShared("bag", String(identity.getAttributeValues("fr-idm-custom-attrs").get(0))); action.goTo("true");',
          expect: { outcome: "true", sharedState: { added: { bag: text } } },
        }),
        { profile },
      );
      expect(second.verdict.pass, second.verdict.summary).toBe(true);
      expect(second.effects.identityCustomAttrs).toEqual({
        [resource]: [text],
      });
    },
  );

  it("unknown ordinary properties and ordinary enum errors still fail after bag provenance is carried", async () => {
    const first = await runCase(
      runner,
      caseWith({
        given,
        script:
          'var identity = idRepository.getIdentity("example"); identity.setAttribute("fr-idm-custom-attrs", [\'{"plain":"y"}\']); identity.store(); action.goTo("true");',
      }),
      { profile },
    );
    const next = carryGiven(given, first.effects, []);
    for (const extra of [
      { ordinaryUnknown: "typo" },
      { givenName: "NotInEnum" },
    ]) {
      const row = next.managed?.["managed/alpha_user"]?.[0];
      if (row === undefined) throw new Error("missing carried fixture");
      const kase = caseWith({
        given: {
          ...next,
          managed: { "managed/alpha_user": [{ ...row, ...extra }] },
        },
        script: 'action.goTo("true");',
      });
      await expect(runCase(runner, kase, { profile })).rejects.toThrow(
        /is not a property|is not an allowed value/,
      );
    }
  });
});

it("harness deletion removes bag provenance before the next pass", async () => {
  const managed = { "managed/alpha_user": [{ _id: "example" }] };
  const bags = { "managed/alpha_user/example": [] };
  await localIdmHandle(managed, () => undefined, bags).delete(
    "managed/alpha_user/example",
  );
  expect(managed).toEqual({ "managed/alpha_user": [] });
  expect(bags).toEqual({});
});
