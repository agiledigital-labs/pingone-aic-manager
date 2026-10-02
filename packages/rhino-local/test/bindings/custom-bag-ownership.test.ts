// Round 5: historical ownership is local carry metadata, independent of values.
import { describe, expect, it } from "vitest";
import { validateCase } from "../../src/case/validate.ts";
import { normaliseEffects } from "../../src/case/verdict.ts";
import { caseWith } from "../aic/helpers.ts";
import { runScript } from "./load-behaviour.ts";

const resource = "managed/alpha_user/example";
const row = { _id: "example", givenName: "Original" };
const seed = {
  managed: { "managed/alpha_user": [row] },
  identityCustomAttrs: { [resource]: [] },
  identityCustomAttrsOwnedKeys: { [resource]: ["plain"] },
  identityAttributes: {
    alias: { field: "plain", cardinality: "single" as const },
  },
};
const identity = 'var identity = idRepository.getIdentity("example");';

describe("validated historical bag ownership", () => {
  it.each([
    null,
    [],
    { example: ["plain"] },
    { "managed/alpha_role/missing": ["plain"] },
    { [resource]: "plain" },
    { [resource]: [true] },
    { [resource]: new Array<string>(1) },
    { [resource]: ["plain", "plain"] },
    { [resource]: ["givenName"] },
  ])("validates ownership in given and harvest: %j", (raw) => {
    const kase = caseWith({ given: seed });
    expect(() =>
      validateCase({
        ...kase,
        given: { ...seed, identityCustomAttrsOwnedKeys: raw },
      }),
    ).toThrow(/rhino-local: given.identityCustomAttrsOwnedKeys/);
    const effects = runScript("", seed);
    expect(() =>
      normaliseEffects({ ...effects, identityCustomAttrsOwnedKeys: raw }),
    ).toThrow(/rhino-local: effects.identityCustomAttrsOwnedKeys/);
  });

  it("does not allow an ownership entry to omit a current bag key", () => {
    const given = {
      ...seed,
      managed: { "managed/alpha_user": [{ ...row, plain: "old" }] },
      identityCustomAttrs: { [resource]: ['{"plain":"old"}'] },
      identityCustomAttrsOwnedKeys: { [resource]: [] },
    };
    expect(() => validateCase(caseWith({ given }))).toThrow(
      /omits a current bag key "plain"/,
    );
    const effects = runScript("", {
      ...given,
      identityCustomAttrsOwnedKeys: { [resource]: ["plain"] },
    });
    expect(() =>
      normaliseEffects({
        ...effects,
        identityCustomAttrsOwnedKeys: given.identityCustomAttrsOwnedKeys,
      }),
    ).toThrow(/omits a current bag key "plain"/);
  });

  it("rejects an owned property present outside the persisted bag", () => {
    const given = {
      ...seed,
      managed: { "managed/alpha_user": [{ ...row, plain: "orphan" }] },
    };
    expect(() => validateCase(caseWith({ given }))).toThrow(
      /owns managed property "plain" absent from the current bag/,
    );
    const effects = runScript("", { managed: given.managed });
    expect(() =>
      normaliseEffects({
        ...effects,
        identityCustomAttrs: given.identityCustomAttrs,
        identityCustomAttrsOwnedKeys: given.identityCustomAttrsOwnedKeys,
      }),
    ).toThrow(/owns managed property "plain" absent from the current bag/);
  });

  it("historical ownership does not collide across collections", () => {
    const role = "managed/alpha_role/example";
    const given = {
      managed: { ...seed.managed, "managed/alpha_role": [{ _id: "example" }] },
      identityAttributes: seed.identityAttributes,
      identityCustomAttrs: { [role]: [] },
      identityCustomAttrsOwnedKeys: { [role]: ["plain"] },
    };
    const effects = runScript(
      identity +
        'identity.setAttribute("alias", ["ordinary"]); identity.store();',
      given,
    );
    expect(effects.managedStore?.["managed/alpha_user"]).toEqual([
      { ...row, plain: "ordinary" },
    ]);
    expect(effects.identityCustomAttrs).toEqual(given.identityCustomAttrs);
    expect(effects.identityCustomAttrsOwnedKeys).toEqual(
      given.identityCustomAttrsOwnedKeys,
    );
  });

  it.each(["patch", "update"])(
    "an ordinary IDM %s preserves absent historical keys and later re-add projects them",
    (method) => {
      const write =
        method === "patch"
          ? `openidm.patch("${resource}", null, [{operation:"replace",field:"givenName",value:"Changed"}]);`
          : `openidm.update("${resource}", null, {_id:"example",givenName:"Changed"});`;
      const effects = runScript(
        write +
          identity +
          'identity.setAttribute("alias", ["restored"]); identity.store();',
        seed,
      );
      expect(effects.identityCustomAttrs).toEqual({
        [resource]: ['{"plain":"restored"}'],
      });
      expect(effects.identityCustomAttrsOwnedKeys).toEqual({
        [resource]: ["plain"],
      });
      expect(effects.managedStore).toEqual({
        "managed/alpha_user": [
          { _id: "example", givenName: "Changed", plain: "restored" },
        ],
      });
    },
  );

  it("an IDM re-add to a historically owned key projects it too", () => {
    const effects = runScript(
      `openidm.patch("${resource}", null, [{operation:"add",field:"plain",value:"restored"}]);`,
      seed,
    );
    expect(effects.identityCustomAttrs).toEqual({
      [resource]: ['{"plain":"restored"}'],
    });
    expect(effects.identityCustomAttrsOwnedKeys).toEqual({
      [resource]: ["plain"],
    });
  });

  it("delete/recreate removes history and the new wrapper cannot inherit ownership", () => {
    const effects = runScript(
      `openidm.delete("${resource}", null); openidm.create("managed/alpha_user", "example", {givenName:"New"});` +
        identity +
        'identity.setAttribute("alias", ["ordinary"]); identity.store();',
      seed,
    );
    expect(effects.identityCustomAttrs).toEqual({});
    expect(effects.identityCustomAttrsOwnedKeys).toEqual({});
    expect(effects.managedStore?.["managed/alpha_user"]?.[0]?.plain).toBe(
      "ordinary",
    );
  });

  it("same-store overlap with a historically owned absent key refuses atomically", () => {
    const effects = runScript(
      identity +
        'identity.setAttribute("alias", ["restored"]); identity.setAttribute("fr-idm-custom-attrs", []); try { identity.store(); } catch(e) { nodeState.putShared("error", String(e)); }',
      seed,
    );
    expect(effects.sharedState.final.error).toMatch(
      /bag replacement and another attribute write in the same store.*unmeasured/,
    );
    expect(effects.identityWrites).toEqual([]);
    expect(effects.managedStore).toEqual(seed.managed);
    expect(effects.identityCustomAttrsOwnedKeys).toEqual(
      seed.identityCustomAttrsOwnedKeys,
    );
  });

  it("a historical key write into a non-object bag refuses before changing ownership", () => {
    const given = { ...seed, identityCustomAttrs: { [resource]: ["null"] } };
    const effects = runScript(
      identity +
        'identity.setAttribute("alias", ["restored"]); try { identity.store(); } catch(e) { nodeState.putShared("error", String(e)); }',
      given,
    );
    expect(effects.sharedState.final.error).toMatch(
      /within a non-object.*unmeasured/,
    );
    expect(effects.identityWrites).toEqual([]);
    expect(effects.managedStore).toEqual(seed.managed);
    expect(effects.identityCustomAttrsOwnedKeys).toEqual(
      seed.identityCustomAttrsOwnedKeys,
    );
  });

  it.each(["_rev", "_internal", "__proto__"])(
    "historical metadata cannot bypass the reserved-key guard for %s",
    (property) => {
      const given = {
        ...seed,
        identityCustomAttrsOwnedKeys: { [resource]: [property] },
        identityAttributes: {
          alias: { field: property, cardinality: "single" as const },
        },
      };
      const effects = runScript(
        identity +
          'identity.setAttribute("alias", ["reserved"]); try { identity.store(); } catch(e) { nodeState.putShared("error", String(e)); }',
        given,
      );
      expect(effects.sharedState.final.error).toMatch(
        /reserved record metadata.*unmeasured/,
      );
      expect(effects.identityWrites).toEqual([]);
      expect(effects.managedStore).toEqual(seed.managed);
      expect(effects.identityCustomAttrsOwnedKeys).toEqual(
        given.identityCustomAttrsOwnedKeys,
      );
    },
  );
});
