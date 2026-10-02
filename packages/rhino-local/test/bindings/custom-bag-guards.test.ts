import { describe, expect, it } from "vitest";
import { runScript } from "./load-behaviour.ts";

const resource = "managed/alpha_user/example";
const record = {
  _id: "example",
  _rev: "1",
  givenName: "Original",
  custom_seed: "old",
};
const seed = { managed: { "managed/alpha_user": [record] } };
const identity = 'var identity = idRepository.getIdentity("example");';
const readBag =
  'nodeState.putShared("bag", String(identity.getAttributeValues("fr-idm-custom-attrs").get(0)));';
const stageBag = (text: string) =>
  `identity.setAttribute("fr-idm-custom-attrs", [${JSON.stringify(text)}]);`;
const catchCall = (call: string) =>
  `try { ${call} } catch (e) { nodeState.putShared("error", String(e)); }`;
const adapterError =
  "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: Response is not application/json";

describe("pre-mutation bag guards (round 3 #1/#3)", () => {
  for (const first of ["bag", "ordinary"]) {
    it.each(["declaredField", "custom_mapped"])(
      `${first} first: an unused declaration permits bag key %s`,
      (key) => {
        const text = JSON.stringify({ [key]: "value" });
        const bag = stageBag(text);
        const ordinary = 'identity.setAttribute("givenName", ["Changed"]);';
        const effects = runScript(
          identity +
            (first === "bag" ? bag + ordinary : ordinary + bag) +
            "identity.store();" +
            readBag,
          {
            ...seed,
            identityAttributes: {
              alias: { field: "declaredField", cardinality: "single" },
              customAlias: { field: "custom_mapped", cardinality: "single" },
            },
          },
        );
        expect(effects.managedStore).toEqual({
          "managed/alpha_user": [
            { _id: "example", _rev: "1", givenName: "Changed", [key]: "value" },
          ],
        });
        expect(effects.identityWrites).toHaveLength(2);
        expect(effects.sharedState.final.bag).toBe(text);
      },
    );

    // A contradiction must never reach harvest, regardless of store ordering.
    it.each([
      "_id",
      "_rev",
      "_internal",
      "__proto__",
      "givenName",
      "displayName",
      "password",
      "postalAddress",
      "userName",
      "accountStatus",
      "frIndexedString1",
      "fr-idm-custom-attrs",
    ])(`${first} first: refuses collision %s and applies nothing`, (key) => {
      const bag = stageBag(
        JSON.stringify(Object.fromEntries([[key, "collision"]])),
      );
      const ordinary = 'identity.setAttribute("givenName", ["Changed"]);';
      const effects = runScript(
        identity +
          catchCall(
            (first === "bag" ? bag + ordinary : ordinary + bag) +
              "identity.store();",
          ) +
          readBag,
        {
          ...seed,
          identityAttributes: {
            alias: { field: "declaredField", cardinality: "single" },
            customAlias: { field: "custom_mapped", cardinality: "single" },
          },
        },
      );
      expect(effects.sharedState.final.error).toMatch(
        /collides with .+; this identity mapping is unmeasured/,
      );
      expect(effects.sharedState.final.bag).toBe('{"custom_seed":"old"}');
      expect(effects.managedStore).toEqual(seed.managed);
      expect(effects.identityWrites).toEqual([]);
      expect(effects.identityCustomAttrs).toEqual({});
    });

    it(
      "permits distinct ordinary and bag targets in either order: " + first,
      () => {
        const bag = stageBag('{"plain":"x","custom_next":true}');
        const ordinary = 'identity.setAttribute("givenName", ["Changed"]);';
        const effects = runScript(
          identity +
            (first === "bag" ? bag + ordinary : ordinary + bag) +
            "identity.store();" +
            readBag,
          seed,
        );
        expect(effects.managedStore).toEqual({
          "managed/alpha_user": [
            {
              _id: "example",
              _rev: "1",
              givenName: "Changed",
              plain: "x",
              custom_next: true,
            },
          ],
        });
        expect(effects.identityWrites).toHaveLength(2);
        expect(effects.sharedState.final.bag).toBe(
          '{"plain":"x","custom_next":true}',
        );
      },
    );
  }

  for (const method of ["setAttribute", "addAttribute"]) {
    for (const first of ["target", "clear"]) {
      it(`${method}, ${first} first: refuses ordinary bag-owned givenName before any mutation`, () => {
        const row = { _id: "example", _rev: "1", givenName: "Original" };
        const text = '{"givenName":"Original"}';
        const target = `identity.${method}("givenName", ${method === "setAttribute" ? '["Changed"]' : '"Changed"'});`;
        const clear = 'identity.setAttribute("fr-idm-custom-attrs", []);';
        const effects = runScript(
          identity +
            catchCall(
              (first === "target" ? target + clear : clear + target) +
                "identity.store();",
            ),
          {
            managed: { "managed/alpha_user": [row] },
            identityCustomAttrs: { [resource]: [text] },
          },
        );
        expect(effects.sharedState.final.error).toMatch(
          /another AM attribute's IDM field.*unmeasured/,
        );
        expect(effects.identityWrites).toEqual([]);
        expect(effects.managedStore).toEqual({ "managed/alpha_user": [row] });
        expect(effects.identityCustomAttrs).toEqual({ [resource]: [text] });
      });

      it(`${method}, ${first} first: refuses a bag replacement and a declared key write in one store`, () => {
        const row = { _id: "example", _rev: "1", custom_owned: "old" };
        const bagText = '{"custom_owned":"old"}';
        const target = `identity.${method}("alias", ${method === "setAttribute" ? '["Changed"]' : '"Changed"'});`;
        const clear = 'identity.setAttribute("fr-idm-custom-attrs", []);';
        const effects = runScript(
          identity +
            catchCall(
              (first === "target" ? target + clear : clear + target) +
                "identity.store();",
            ) +
            readBag,
          {
            managed: { "managed/alpha_user": [row] },
            identityCustomAttrs: { [resource]: [bagText] },
            identityAttributes: {
              alias: { field: "custom_owned", cardinality: "multi" },
            },
          },
        );
        expect(effects.sharedState.final.error).toMatch(
          /bag replacement and another attribute write in the same store.*unmeasured/,
        );
        expect(effects.managedStore).toEqual({ "managed/alpha_user": [row] });
        expect(effects.identityCustomAttrs).toEqual({ [resource]: [bagText] });
        expect(effects.identityWrites).toEqual([]);
      });
    }
  }

  it("resolves the current bag at store after an intervening IDM patch", () => {
    const row = { _id: "example", _rev: "1", givenName: "Original" };
    const effects = runScript(
      identity +
        'identity.setAttribute("givenName", ["Changed"]); identity.setAttribute("alias", ["Staged"]);' +
        `openidm.patch("${resource}", null, [{operation:"add", field:"custom_later", value:"patched"}]);` +
        "identity.store();" +
        readBag,
      {
        managed: { "managed/alpha_user": [row] },
        identityAttributes: {
          alias: { field: "custom_later", cardinality: "single" },
        },
      },
    );
    expect(effects.managedStore).toEqual({
      "managed/alpha_user": [
        { ...row, givenName: "Changed", custom_later: "Staged" },
      ],
    });
    expect(effects.identityWrites).toHaveLength(2);
    expect(effects.sharedState.final.bag).toBe('{"custom_later":"Staged"}');
    expect(effects.identityCustomAttrs).toEqual({
      [resource]: ['{"custom_later":"Staged"}'],
    });
  });

  it.each(["true", "false"])(
    "boolean %s persists but breaks a full read",
    (text) => {
      const effects = runScript(
        identity +
          stageBag(text) +
          "identity.store();" +
          catchCall(`openidm.read("${resource}");`) +
          readBag,
        seed,
      );
      expect(effects.sharedState.final.error).toBe(adapterError);
      expect(effects.sharedState.final.bag).toBe(text);
    },
  );

  it("a string bag blocks delete without changing the record, and an AM object reset permits deletion", () => {
    const effects = runScript(
      identity +
        stageBag('"x"') +
        "identity.store();" +
        catchCall(`openidm.delete("${resource}", null);`) +
        `nodeState.putShared("rows", openidm.query("managed/alpha_user", {_queryFilter:"true"}, ["_id"]).result);` +
        readBag +
        stageBag("{}") +
        `identity.store(); openidm.delete("${resource}", null);`,
      seed,
    );
    expect(effects.sharedState.final.error).toBe(adapterError);
    expect(effects.sharedState.final.rows).toEqual([
      { _id: "example", _rev: "1" },
    ]);
    expect(effects.sharedState.final.bag).toBe('"x"');
    expect(effects.managedStore).toEqual({ "managed/alpha_user": [] });
    expect(effects.identityCustomAttrs).toEqual({});
  });

  for (const text of ["1", "[]", "true", "false", "null"]) {
    it.each(["delete", "patch", "update", "create"])(
      `${text}: %s refuses unmeasured mutation before changing state`,
      (method) => {
        const call =
          method === "delete"
            ? `openidm.delete("${resource}", null);`
            : method === "patch"
              ? `openidm.patch("${resource}", null, [{operation:"replace",field:"givenName",value:"Changed"}]);`
              : method === "update"
                ? `openidm.update("${resource}", null, {_id:"example",givenName:"Changed"});`
                : 'openidm.create("managed/alpha_user", "example", {givenName:"Changed"});';
        const effects = runScript(
          identity +
            stageBag(text) +
            "identity.store();" +
            catchCall(call) +
            readBag,
          seed,
        );
        expect(effects.sharedState.final.error).toMatch(
          /this IDM access is unmeasured/,
        );
        expect(effects.sharedState.final.bag).toBe(text);
        expect(effects.managedStore).toEqual({
          "managed/alpha_user": [
            { _id: "example", _rev: "1", givenName: "Original" },
          ],
        });
        expect(effects.identityCustomAttrs).toEqual({ [resource]: [text] });
      },
    );
  }

  it("a boolean _id query remains unmeasured", () => {
    const effects = runScript(
      identity +
        stageBag("true") +
        "identity.store();" +
        catchCall(
          'openidm.query("managed/alpha_user", {_queryFilter:"true"}, ["_id"]);',
        ),
      seed,
    );
    expect(effects.sharedState.final.error).toMatch(
      /bag \(boolean\); this IDM access is unmeasured/,
    );
  });
});
