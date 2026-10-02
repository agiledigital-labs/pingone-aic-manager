// Round 6: supplied history must not promote ordinary/metadata properties into
// the bag, bypassing collision guards and profile validation through IDM writes.
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/run.ts";
import { parseHarvest } from "../../src/bindings/harvest.ts";
import { mockPreamble, withHarvest } from "../../src/bindings/preamble.ts";
import { normaliseEffects } from "../../src/case/verdict.ts";
import { validateCase } from "../../src/case/validate.ts";
import { createIdentityPolicy } from "../../src/case/identity-policy.ts";
import { carryGiven } from "../../src/harness/step.ts";
import type { EnvProfile } from "../../src/profile/types.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { caseWith, lintAmScript } from "../aic/helpers.ts";

const resource = "managed/alpha_user/example";
const managed = { "managed/alpha_user": [{ _id: "example" }] };
const seed = {
  managed,
  identityCustomAttrs: { [resource]: [] },
};
const emptyEffects = {
  outcome: null,
  sharedState: { initial: {}, final: {} },
  transientState: { initial: {}, final: {} },
  secureState: { initial: {}, final: {} },
  callbacks: [],
  openidm: [],
  http: [],
  logs: [],
  managedStore: managed,
  identityCustomAttrs: seed.identityCustomAttrs,
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

function mutation(method: string, key: string): string {
  return method === "patch"
    ? `openidm.patch("${resource}", null, [{operation:"add",field:"plain",value:"partial"},{operation:"add",field:${JSON.stringify(key)},value:"invalid"}]);`
    : `openidm.update("${resource}", null, ${JSON.stringify({ plain: "partial", [key]: "invalid" })});`;
}

describe("ownership collision checks on IDM writes", () => {
  let runner: RhinoRunner;
  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);
  afterAll(async () => {
    await runner.close();
  });

  it("the factory shared with Node remains AM-safe after host transformation", async () => {
    expect(
      await lintAmScript(
        `var policy = (${createIdentityPolicy.toString()})();`,
        "cases/identity-policy.cjs",
      ),
    ).toEqual([]);
  });

  for (const method of ["patch", "update"]) {
    it.each(["givenName", "_rev"])(
      `${method} rejects supplied historical %s before it reaches the JVM`,
      async (key) => {
        const given = {
          ...seed,
          identityCustomAttrsOwnedKeys: { [resource]: [key] },
        };
        await expect(
          runCase(
            runner,
            {
              ...caseWith(),
              given,
              script: mutation(method, key),
            },
            { profile },
          ),
        ).rejects.toThrow(/identityCustomAttrsOwnedKeys.*collides with/);
      },
    );

    it.each(["givenName", "_rev"])(
      `${method} also preflights historical %s when Node validation is bypassed`,
      async (key) => {
        // Inject after valid seeding to exercise the runtime's own guard.
        const result = await runner.eval({
          preamble: mockPreamble({
            ...seed,
            identityCustomAttrsOwnedKeys: { [resource]: ["plain"] },
          }),
          source: withHarvest(`
            __rhinoLocal.identityCustomAttrsOwnedKeys["${resource}"] = ["plain", ${JSON.stringify(key)}];
            try { ${mutation(method, key)} } catch(e) { nodeState.putShared("error", String(e)); }
            nodeState.putShared("record", JSON.parse(JSON.stringify(__rhinoLocal.managed["managed/alpha_user"][0])));
            nodeState.putShared("values", __rhinoLocal.identityCustomAttrs["${resource}"].slice());
            nodeState.putShared("keys", __rhinoLocal.identityCustomAttrsOwnedKeys["${resource}"].slice());
            // Remove only the injected claim before parsing the harvest.
            __rhinoLocal.identityCustomAttrsOwnedKeys["${resource}"] = ["plain"];
          `),
        });
        expect(result.outcome).toBe("ok");
        if (typeof result.value !== "string")
          throw new Error("missing harvest");
        const effects = parseHarvest(result.value);
        expect(effects.sharedState.final.error).toMatch(
          new RegExp(`openidm\\.${method}:.*collides with.*unmeasured`),
        );
        expect(effects.sharedState.final.record).toEqual(
          managed["managed/alpha_user"][0],
        );
        expect(effects.sharedState.final.values).toEqual([]);
        expect(effects.sharedState.final.keys).toEqual(["plain", key]);
        expect(effects.managedStore).toEqual(managed);
        expect(effects.identityCustomAttrs).toEqual(seed.identityCustomAttrs);
      },
    );

    it(`${method} accepts a plain historical key and keeps givenName ordinary`, async () => {
      const given = {
        ...seed,
        identityCustomAttrsOwnedKeys: { [resource]: ["plain"] },
      };
      const result = await runCase(
        runner,
        caseWith({
          given,
          script: mutation(method, "givenName") + 'action.goTo("true");',
          expect: { outcome: "true", allowUndeclared: { openidmWrites: true } },
        }),
        { profile },
      );
      expect(result.verdict.pass, result.verdict.summary).toBe(true);
      expect(result.effects.identityCustomAttrs).toEqual({
        [resource]: ['{"plain":"partial"}'],
      });
      expect(
        result.effects.managedStore?.["managed/alpha_user"]?.[0]?.givenName,
      ).toBe("invalid");
      await expect(
        runCase(
          runner,
          caseWith({
            given: carryGiven(given, result.effects, []),
            script: 'action.goTo("true");',
          }),
          { profile },
        ),
      ).rejects.toThrow(/is not an allowed value.*givenName/);
    });
  }

  it.each([
    "givenName",
    "_rev",
    "_internal",
    "displayName",
    "frIndexedString1",
  ])("given and harvested ownership both reject %s", (key) => {
    const metadata = { [resource]: [key] };
    expect(() =>
      validateCase(
        caseWith({
          given: { ...seed, identityCustomAttrsOwnedKeys: metadata },
        }),
      ),
    ).toThrow(/identityCustomAttrsOwnedKeys.*collides with/);
    expect(() =>
      normaliseEffects({
        ...emptyEffects,
        identityCustomAttrsOwnedKeys: metadata,
      }),
    ).toThrow(/identityCustomAttrsOwnedKeys.*collides with/);
  });

  it.each([
    "givenName",
    "_rev",
    "_internal",
    "displayName",
    "frIndexedString1",
  ])("values-only provenance cannot bypass the same policy for %s", (key) => {
    const given = {
      managed: { "managed/alpha_user": [{ _id: "example", [key]: "invalid" }] },
      identityCustomAttrs: {
        [resource]: [JSON.stringify({ [key]: "invalid" })],
      },
    };
    expect(() => validateCase({ ...caseWith(), given })).toThrow(
      /given.identityCustomAttrs.*collides with/,
    );
    expect(() =>
      normaliseEffects({
        ...emptyEffects,
        managedStore: given.managed,
        identityCustomAttrs: given.identityCustomAttrs,
      }),
    ).toThrow(/effects.identityCustomAttrs.*collides with/);
  });
});
