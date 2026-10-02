import { describe, expect, it } from "vitest";
import { runScript } from "./load-behaviour.ts";
import type { Given, RecordedEffects } from "../../src/case/types.ts";

const resource = "managed/alpha_user/example";
const row = {
  _id: "example",
  givenName: "Original",
  custom_example: "seeded",
  custom_other: true,
};
const given: Given = {
  managed: { "managed/alpha_user": [row] },
  identityAttributes: {
    alias: { field: "custom_example", cardinality: "single" },
  },
};
const identity = 'var identity = idRepository.getIdentity("example");';
const read = `
  nodeState.putShared("alias", String(identity.getAttributeValues("alias")));
  nodeState.putShared("bag", Array.from(identity.getAttributeValues("fr-idm-custom-attrs")));
  nodeState.putShared("fresh", Array.from(idRepository.getIdentity("example").getAttributeValues("fr-idm-custom-attrs")));
`;
const stageBag = (values: string[]) =>
  `identity.setAttribute("fr-idm-custom-attrs", ${JSON.stringify(values)});`;
const catchStore =
  'try { identity.store(); } catch (e) { nodeState.putShared("error", String(e)); }';
const bagOf = (effects: RecordedEffects) => {
  const values = effects.identityCustomAttrs?.[resource];
  if (values?.[0] === undefined)
    throw new Error("expected persisted object bag");
  return JSON.parse(values[0]) as Record<string, unknown>;
};

describe("declared layouts and coherent custom bags (round 4)", () => {
  it.each([
    ["_id", "custom_id"],
    ["_rev", "custom_revision"],
    ["_internal", "custom_internal"],
    ["__proto__", "custom_prototype"],
    ["givenName", "custom_givenName"],
    ["sn", "custom_sn"],
    ["mail", "custom_mail"],
    ["telephoneNumber", "custom_phone"],
    ["displayName", "custom_displayName"],
    ["password", "custom_password"],
    ["postalAddress", "custom_address"],
    ["userName", "custom_userName"],
    ["accountStatus", "custom_status"],
    ["fr-idm-custom-attrs", "custom_bag"],
    ["frUnindexedString5", "frUnindexedString6"],
    ["frIndexedString20", "frIndexedString21"],
    ["frUnindexedMultivalued5", "frUnindexedMultivalued6"],
    ["frIndexedMultivalued5", "frIndexedMultivalued6"],
    ["frUnindexedInteger5", "frUnindexedInteger6"],
    ["frIndexedInteger5", "frIndexedInteger6"],
    ["frUnindexedDate5", "frUnindexedDate6"],
    ["frIndexedDate5", "frIndexedDate6"],
  ])("refuses bag key %s while neighbouring key %s works", (bad, good) => {
    const text = (key: string) =>
      JSON.stringify(Object.fromEntries([[key, "value"]]));
    const effects = runScript(
      identity +
        stageBag([text(bad)]) +
        catchStore +
        'nodeState.putShared("before", JSON.parse(JSON.stringify(openidm.read("managed/alpha_user/example"))));' +
        stageBag([text(good)]) +
        "identity.store();" +
        read,
      given,
    );
    expect(effects.sharedState.final.error).toMatch(
      /collides with .+; this identity mapping is unmeasured/,
    );
    expect(effects.sharedState.final.before).toEqual(row);
    expect(bagOf(effects)).toEqual({ [good]: "value" });
    expect(effects.managedStore).toEqual({
      "managed/alpha_user": [
        { _id: "example", givenName: "Original", [good]: "value" },
      ],
    });
    expect(effects.identityWrites).toHaveLength(1);
  });

  it("the guard-only documented inventory does not enable a write layout", () => {
    const effects = runScript(
      identity +
        'try { identity.setAttribute("displayName", ["Changed"]); } catch (e) { nodeState.putShared("error", String(e)); }',
      given,
    );
    expect(effects.sharedState.final.error).toMatch(
      /how AM stores "displayName" in IDM is unmeasured/,
    );
    expect(effects.managedStore).toEqual(given.managed);
    expect(effects.identityWrites).toEqual([]);
  });

  it.each([false, true])(
    "writes a seeded declared field, repeated=%s, preserving other bag keys",
    (repeat) => {
      const script =
        'identity.setAttribute("alias", ["first"]); identity.store();' +
        (repeat
          ? 'identity.setAttribute("alias", ["second"]); identity.store();'
          : "");
      const value = repeat ? "second" : "first";
      const effects = runScript(identity + script + read, given);
      expect(effects.managedStore).toEqual({
        "managed/alpha_user": [{ ...row, custom_example: value }],
      });
      expect(bagOf(effects)).toEqual({
        custom_example: value,
        custom_other: true,
      });
      expect(effects.sharedState.final.alias).toBe(`[${value}]`);
      expect(effects.sharedState.final.bag).toEqual(
        effects.identityCustomAttrs?.[resource],
      );
      expect(effects.sharedState.final.fresh).toEqual(
        effects.sharedState.final.bag,
      );
      expect(effects.identityWrites).toEqual(
        (repeat ? ["first", "second"] : ["first"]).map((entry) => ({
          identity: "example",
          attribute: "alias",
          values: [entry],
        })),
      );
    },
  );

  it("clears the bag with an unused declaration, then writes the declared field", () => {
    const effects = runScript(
      identity +
        stageBag([]) +
        'identity.store(); nodeState.putShared("cleared", Array.from(identity.getAttributeValues("fr-idm-custom-attrs")));' +
        'identity.setAttribute("alias", ["restored"]); identity.store();' +
        read,
      given,
    );
    expect(effects.sharedState.final.cleared).toEqual([]);
    expect(effects.managedStore).toEqual({
      "managed/alpha_user": [
        { _id: "example", givenName: "Original", custom_example: "restored" },
      ],
    });
    expect(bagOf(effects)).toEqual({ custom_example: "restored" });
    expect(effects.identityWrites).toEqual([
      { identity: "example", attribute: "fr-idm-custom-attrs", values: [] },
      { identity: "example", attribute: "alias", values: ["restored"] },
    ]);
    expect(effects.sharedState.final.fresh).toEqual(
      effects.sharedState.final.bag,
    );
  });

  it.each([{ values: [] }, { values: ["{}"] }])(
    "clears with $values while the custom-field declaration is unused",
    ({ values }) => {
      const effects = runScript(
        identity + stageBag(values) + "identity.store();" + read,
        given,
      );
      expect(effects.managedStore).toEqual({
        "managed/alpha_user": [{ _id: "example", givenName: "Original" }],
      });
      expect(effects.sharedState.final.bag).toEqual(values);
      expect(effects.sharedState.final.alias).toBe("[]");
      expect(effects.identityWrites).toHaveLength(1);
    },
  );

  it("a declared custom field can replace a bag-written value without replacing the whole bag", () => {
    const effects = runScript(
      identity +
        stageBag(['{"custom_example":false,"plain":"retained"}']) +
        "identity.store();" +
        'identity.setAttribute("alias", ["declared"]); identity.store();' +
        read,
      given,
    );
    expect(bagOf(effects)).toEqual({
      custom_example: "declared",
      plain: "retained",
    });
    expect(effects.managedStore).toEqual({
      "managed/alpha_user": [
        {
          _id: "example",
          givenName: "Original",
          custom_example: "declared",
          plain: "retained",
        },
      ],
    });
  });

  it("a declared unprefixed bag key follows the same key-replacement rule", () => {
    const effects = runScript(
      identity +
        stageBag(['{"plain":"old","custom_kept":true}']) +
        "identity.store();" +
        'identity.setAttribute("plainAlias", ["changed"]); identity.store();' +
        read,
      {
        ...given,
        identityAttributes: {
          plainAlias: { field: "plain", cardinality: "single" },
        },
      },
    );
    expect(bagOf(effects)).toEqual({ plain: "changed", custom_kept: true });
    expect(effects.managedStore?.["managed/alpha_user"]?.[0]?.plain).toBe(
      "changed",
    );
  });

  it.each(["single", "multi"] as const)(
    "keeps a %s declared field's empty-value shape coherent",
    (cardinality) => {
      const effects = runScript(
        identity +
          'identity.setAttribute("alias", []); identity.store();' +
          read,
        {
          ...given,
          identityAttributes: {
            alias: { field: "custom_example", cardinality },
          },
        },
      );
      expect(bagOf(effects)).toEqual(
        cardinality === "single"
          ? { custom_other: true }
          : { custom_example: [], custom_other: true },
      );
      expect(effects.managedStore?.["managed/alpha_user"]?.[0]).toEqual(
        cardinality === "single"
          ? { _id: "example", givenName: "Original", custom_other: true }
          : { ...row, custom_example: [] },
      );
    },
  );

  it.each(["custom_example", "fr-idm-custom-attrs", "elsewhere"])(
    "a declared override of the bag itself wins, targeting %s",
    (field) => {
      const effects = runScript(
        identity +
          stageBag(["not json"]) +
          "identity.store();" +
          'nodeState.putShared("bag", Array.from(identity.getAttributeValues("fr-idm-custom-attrs")));',
        {
          ...given,
          identityAttributes: {
            "fr-idm-custom-attrs": { field, cardinality: "single" },
          },
        },
      );
      expect(effects.sharedState.final.bag).toEqual(["not json"]);
      expect(effects.managedStore).toEqual({
        "managed/alpha_user": [{ ...row, [field]: "not json" }],
      });
      expect(effects.identityWrites).toEqual([
        {
          identity: "example",
          attribute: "fr-idm-custom-attrs",
          values: ["not json"],
        },
      ]);
      expect(effects.identityCustomAttrs).toEqual(
        field === "custom_example"
          ? {
              [resource]: ['{"custom_example":"not json","custom_other":true}'],
            }
          : {},
      );
    },
  );

  for (const first of ["alias", "ordinary"]) {
    it.each(['"x"', "1", "[]", "true", "null"])(
      `${first} first: refuses a key write within non-object %s before any effects`,
      (text) => {
        const alias = 'identity.setAttribute("alias", ["changed"]);';
        const ordinary = 'identity.setAttribute("givenName", ["Changed"]);';
        const seeded = { _id: "example", givenName: "Original" };
        const effects = runScript(
          identity +
            (first === "alias" ? alias + ordinary : ordinary + alias) +
            catchStore +
            read,
          {
            ...given,
            managed: { "managed/alpha_user": [seeded] },
            identityCustomAttrs: { [resource]: [text] },
          },
        );
        expect(effects.sharedState.final.error).toMatch(
          /replacing "custom_example" within a non-object.*unmeasured/,
        );
        expect(effects.identityWrites).toEqual([]);
        expect(effects.managedStore).toEqual({
          "managed/alpha_user": [seeded],
        });
        expect(effects.identityCustomAttrs).toEqual({ [resource]: [text] });
      },
    );
  }

  it.each(['"x"', "1", "[]", "true", "null"])(
    "permits a distinct ordinary write with non-object %s",
    (text) => {
      const effects = runScript(
        identity +
          'identity.setAttribute("givenName", ["Changed"]); identity.store();' +
          read,
        {
          ...given,
          managed: {
            "managed/alpha_user": [{ _id: "example", givenName: "Original" }],
          },
          identityCustomAttrs: { [resource]: [text] },
        },
      );
      expect(effects.identityWrites).toHaveLength(1);
      expect(effects.managedStore).toEqual({
        "managed/alpha_user": [{ _id: "example", givenName: "Changed" }],
      });
      expect(effects.identityCustomAttrs).toEqual({ [resource]: [text] });
    },
  );

  it.each(["example", "changed"])(
    "an _id declaration keeps %s only when the resource stays unchanged",
    (value) => {
      const effects = runScript(
        identity +
          'identity.setAttribute("givenName", ["Changed"]);' +
          `identity.setAttribute("identifier", [${JSON.stringify(value)}]);` +
          catchStore,
        {
          ...given,
          identityAttributes: {
            identifier: { field: "_id", cardinality: "single" },
          },
        },
      );
      if (value === "changed") {
        expect(effects.sharedState.final.error).toMatch(
          /changing the identity resource.*unmeasured/,
        );
        expect(effects.managedStore).toEqual(given.managed);
        expect(effects.identityWrites).toEqual([]);
      } else {
        expect(effects.sharedState.final.error).toBeUndefined();
        expect(effects.managedStore).toEqual({
          "managed/alpha_user": [{ ...row, givenName: "Changed" }],
        });
        expect(effects.identityWrites).toHaveLength(2);
      }
    },
  );
});
