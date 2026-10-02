import { describe, expect, it } from "vitest";
import { normaliseEffects, validateCase } from "../../src/case/index.ts";
import { caseWith } from "../aic/helpers.ts";
import { runScript } from "./load-behaviour.ts";

const resource = "managed/alpha_user/example";
const record = {
  _id: "example",
  _rev: "1",
  givenName: "Original",
  userName: "fixture",
};
const seed = { managed: { "managed/alpha_user": [record] } };
const identity = 'var identity = idRepository.getIdentity("example");';
const get =
  'nodeState.putShared("text", String(identity.getAttributeValues("fr-idm-custom-attrs").get(0)));';
const write = (text: string) =>
  `identity.setAttribute("fr-idm-custom-attrs", [${JSON.stringify(text)}]); identity.store();`;

describe("custom bag edges (review #1–4)", () => {
  it.each([
    '"x"',
    "1",
    "[]",
    "null",
    '{"plain":"x"}',
    '{"custom_nested":{"a":1}}',
  ])("retains exact AM text %s across wrappers and harvest", (text) => {
    const effects = runScript(
      identity +
        write(text) +
        get +
        'nodeState.putShared("fresh", String(idRepository.getIdentity("example").getAttributeValues("fr-idm-custom-attrs").get(0)));',
      seed,
    );
    expect(effects.sharedState.final).toEqual({ text, fresh: text });
    expect(effects.identityCustomAttrs).toEqual({ [resource]: [text] });
    const expected = JSON.parse(text) as unknown;
    const properties =
      expected !== null &&
      typeof expected === "object" &&
      !Array.isArray(expected)
        ? expected
        : {};
    expect(effects.managedStore?.["managed/alpha_user"]?.[0]).toEqual({
      ...record,
      ...properties,
    });
  });

  it.each(['"x"', "1", "[]"])(
    "full IDM read throws the measured adapter error for %s; _id query succeeds",
    (text) => {
      const effects = runScript(
        identity +
          write(text) +
          `try { openidm.read("${resource}"); } catch (e) { nodeState.putShared("error", String(e)); } nodeState.putShared("rows", openidm.query("managed/alpha_user", {_queryFilter: 'userName eq "fixture"'}, ["_id"]).result);`,
        seed,
      );
      expect(effects.sharedState.final.error).toBe(
        "JavaException: org.forgerock.openam.scripting.wrappers.ResourceExceptionScriptAdapter: Response is not application/json",
      );
      expect(effects.sharedState.final.rows).toEqual([
        { _id: "example", _rev: "1" },
      ]);
    },
  );

  it("JSON null permits a full read without projected bag keys", () => {
    const effects = runScript(
      identity +
        write("null") +
        `nodeState.putShared("row", openidm.read("${resource}"));`,
      seed,
    );
    expect(effects.sharedState.final.row).toEqual(record);
  });

  it("projects unprefixed/nested bag properties and deletes them on whole-bag replacement", () => {
    const effects = runScript(
      identity +
        write('{"plain":"x","custom_nested":{"a":1}}') +
        `nodeState.putShared("first", JSON.parse(JSON.stringify(openidm.read("${resource}"))));` +
        write('{"custom_next":true}') +
        `nodeState.putShared("second", openidm.read("${resource}"));`,
      seed,
    );
    expect(effects.sharedState.final.first).toEqual({
      ...record,
      plain: "x",
      custom_nested: { a: 1 },
    });
    expect(effects.sharedState.final.second).toEqual({
      ...record,
      custom_next: true,
    });
  });

  for (const text of ['"x"', "1", "[]", "null"]) {
    it.each([
      `openidm.read("${resource}", null, ["_id"]);`,
      'openidm.query("managed/alpha_user", {_queryFilter: "true"});',
      'openidm.query("managed/alpha_user", {_queryFilter: "true"}, ["givenName"]);',
      'openidm.query("managed/alpha_user", {_queryId: "query-all-ids"});',
      `openidm.patch("${resource}", null, [{operation:"replace",field:"givenName",value:"Changed"}]);`,
      `openidm.update("${resource}", null, {_id:"example", givenName:"Changed"});`,
    ])(`refuses unmeasured IDM access with ${text}: %s`, (call) => {
      expect(() => runScript(identity + write(text) + call, seed)).toThrow(
        /carries a non-object fr-idm-custom-attrs bag \(.+\); this IDM access is unmeasured/,
      );
    });
  }

  it.each(["patch", "update"])(
    "ordinary IDM %s restores [] to one {} and refreshes the retained wrapper",
    (method) => {
      const call =
        method === "patch"
          ? `openidm.patch("${resource}", null, [{ operation:"replace", field:"givenName", value:"Changed" }]);`
          : `var row = openidm.read("${resource}"); row.givenName = "Changed"; openidm.update("${resource}", null, row);`;
      const effects = runScript(
        identity +
          'identity.setAttribute("fr-idm-custom-attrs", []); identity.store();' +
          call +
          get +
          'nodeState.putShared("old", String(identity.getAttributeValues("givenName").get(0))); nodeState.putShared("fresh", String(idRepository.getIdentity("example").getAttributeValues("givenName").get(0)));' +
          `nodeState.putShared("row", openidm.read("${resource}"));`,
        seed,
      );
      expect(effects.sharedState.final).toEqual({
        text: "{}",
        old: "Changed",
        fresh: "Changed",
        row: { ...record, givenName: "Changed" },
      });
    },
  );

  it("a retained wrapper writes to the current record after update, even when staged beforehand", () => {
    const effects = runScript(
      identity +
        'identity.setAttribute("givenName", ["Stored"]);' +
        `openidm.update("${resource}", null, ${JSON.stringify({ ...record, givenName: "Updated" })}); identity.store();` +
        'nodeState.putShared("old", String(identity.getAttributeValues("givenName").get(0))); nodeState.putShared("fresh", String(idRepository.getIdentity("example").getAttributeValues("givenName").get(0)));' +
        `nodeState.putShared("row", openidm.read("${resource}"));`,
      seed,
    );
    expect(effects.sharedState.final).toEqual({
      old: "Stored",
      fresh: "Stored",
      row: { ...record, givenName: "Stored" },
    });
  });

  it("delete then recreate refuses the retained wrapper but leaves fresh reads and stored record consistent", () => {
    const effects = runScript(
      identity +
        'identity.setAttribute("fr-idm-custom-attrs", []); identity.store();' +
        `openidm.delete("${resource}", null); openidm.create("managed/alpha_user", "example", {givenName:"Recreated"}); try { identity.getAttributeValues("givenName"); } catch(e) { nodeState.putShared("error", String(e)); }` +
        'nodeState.putShared("fresh", String(idRepository.getIdentity("example").getAttributeValues("givenName").get(0))); nodeState.putShared("bag", String(idRepository.getIdentity("example").getAttributeValues("fr-idm-custom-attrs").get(0)));' +
        `nodeState.putShared("row", openidm.read("${resource}"));`,
      seed,
    );
    expect(effects.sharedState.final.error).toMatch(
      /after delete and recreate is unmeasured; get a fresh identity wrapper/,
    );
    expect(effects.sharedState.final.fresh).toBe("Recreated");
    expect(effects.sharedState.final.bag).toBe("{}");
    expect(effects.sharedState.final.row).toEqual({
      _id: "example",
      _rev: "local-rev",
      givenName: "Recreated",
    });
    expect(effects.identityCustomAttrs).toEqual({});
  });

  it("bag metadata cannot collide across collections", () => {
    const given = {
      managed: {
        "managed/alpha_user": [record],
        "managed/alpha_role": [{ _id: "example" }],
      },
      identityCustomAttrs: { "managed/alpha_role/example": [] },
    };
    const kase = validateCase(caseWith({ given }));
    expect(kase.given.identityCustomAttrs).toEqual({
      "managed/alpha_role/example": [],
    });
    expect(runScript(identity + get, kase.given).sharedState.final.text).toBe(
      "{}",
    );
  });

  it("a cleared map entry resets in one ordinary IDM patch", () => {
    const effects = runScript(
      identity +
        `openidm.patch("${resource}", null, [{operation:"replace",field:"givenName",value:"Changed"}]);` +
        get,
      { ...seed, identityCustomAttrs: { [resource]: [] } },
    );
    expect(effects.sharedState.final.text).toBe("{}");
    expect(effects.identityCustomAttrs).toEqual({ [resource]: ["{}"] });
  });

  it.each([
    { identityCustomAttrs: { example: [] } },
    { identityCustomAttrs: { "managed/alpha_role/missing": [] } },
    { identityCustomAttrs: { [resource]: [false] } },
    { identityCustomAttrs: { [resource]: new Array<string>(1) } },
    { identityCustomAttrs: { [resource]: ["not json"] } },
    { identityCustomAttrs: { [resource]: ["{}", "{}"] } },
    { identityCustomAttrs: { "managed/alpha_role/missing": [] } },
    { identityCustomAttrs: { [resource]: ['{"plain":"unprojected"}'] } },
  ])("validates marker/bag metadata against the store: %j", (metadata) => {
    expect(() =>
      validateCase({ ...caseWith(), given: { ...seed, ...metadata } }),
    ).toThrow(/rhino-local:/);
    const effects = runScript("", seed);
    expect(() => normaliseEffects({ ...effects, ...metadata })).toThrow(
      /rhino-local:/,
    );
  });

  it("declared layouts exempt explicit seeds consistently with measured-layout overrides", () => {
    const given = {
      managed: {
        "managed/alpha_user": [
          { ...record, "fr-idm-custom-attrs": "declared" },
        ],
      },
      identityAttributes: {
        "fr-idm-custom-attrs": {
          field: "fr-idm-custom-attrs",
          cardinality: "single" as const,
        },
      },
    };
    expect(validateCase(caseWith({ given })).given.managed).toEqual(
      given.managed,
    );
    expect(runScript(identity + get, given).sharedState.final.text).toBe(
      "declared",
    );
  });
});
