import { describe, expect, it } from "vitest";
import { runScript } from "./load-behaviour.ts";
import type { JsonObject, JsonValue } from "../../src/case/types.ts";

const envelope: JsonObject = {
  _id: "meta-example",
  _rev: "meta-rev",
  _ref: "managed/alpha_usermeta/meta-example",
  _refResourceCollection: "managed/alpha_usermeta",
  _refResourceId: "meta-example",
  _refResourceRev: "meta-rev",
  _refProperties: { _id: "ref-example", _rev: "ref-rev" },
};
const lastChanged = { date: "2026-10-02T00:00:00Z" };
const meta = {
  ...envelope,
  lastChanged,
  unwanted: "omit",
  _secret: "omit",
  _unrelated: { private: true },
};
const projection = (
  parent: JsonValue,
  fields: string[],
  method: string = "read",
) =>
  runScript(
    method === "read"
      ? `nodeState.putShared("row", openidm.read("managed/alpha_user/example", null, ${JSON.stringify(fields)}));`
      : method === "query"
        ? `nodeState.putShared("row", openidm.query("managed/alpha_user", {_queryFilter:"true"}, ${JSON.stringify(fields)}).result[0]);`
        : `nodeState.putShared("row", openidm.query("managed/alpha_user", {_queryFilter:"true", _fields:${JSON.stringify(method === "_fields array" ? fields : fields.join(","))}}).result[0]);`,
    {
      managed: {
        "managed/alpha_user": [{ _id: "example", _rev: "1", _meta: parent }],
      },
    },
  );

describe("documented reference-envelope projection (round 3 #2)", () => {
  // Blanket underscore copying used to leak arbitrary siblings.
  it.each(["read", "query"])(
    "%s returns precisely _meta/lastChanged plus the documented envelope",
    (method) => {
      expect(
        projection(meta, ["_meta/lastChanged"], method).sharedState.final.row,
      ).toEqual({
        _id: "example",
        _rev: "1",
        _meta: { ...envelope, lastChanged },
      });
    },
  );

  it("an explicitly requested underscore sibling is still selected", () => {
    expect(
      projection(meta, ["_meta/lastChanged", "_meta/_unrelated"]).sharedState
        .final.row,
    ).toEqual({
      _id: "example",
      _rev: "1",
      _meta: { ...envelope, lastChanged, _unrelated: { private: true } },
    });
  });

  it.each(["read", "query"])(
    "%s refuses scalar, array and null parents rather than inventing their projection",
    (method) => {
      for (const parent of ["scalar", 1, true, null, [meta]]) {
        expect(() => projection(parent, ["_meta/lastChanged"], method)).toThrow(
          /scalar or array parent is unmeasured/,
        );
      }
    },
  );

  it.each(["read", "query"])(
    "%s refuses unresolved references instead of expanding seeded targets",
    (method) => {
      const parent = { _ref: "managed/alpha_usermeta/meta-example" };
      expect(() => projection(parent, ["_meta/lastChanged"], method)).toThrow(
        /unresolved _ref or reference expansion is unmeasured/,
      );
      const script =
        method === "read"
          ? 'openidm.read("managed/alpha_user/example", null, ["_meta/lastChanged"]);'
          : 'openidm.query("managed/alpha_user", {_queryFilter:"true"}, ["_meta/lastChanged"]);';
      expect(() =>
        runScript(script, {
          managed: {
            "managed/alpha_user": [{ _id: "example", _meta: parent }],
            "managed/alpha_usermeta": [{ _id: "meta-example", lastChanged }],
          },
        }),
      ).toThrow(/reference expansion is unmeasured/);
    },
  );

  it.each([
    { fields: ["_meta", "_meta/lastChanged"] },
    { fields: ["_meta/lastChanged", "_meta"] },
  ])(
    "a whole-parent selector does not hide an unsupported child selector: %j",
    ({ fields }) => {
      expect(() => projection([meta], fields)).toThrow(/unmeasured/);
    },
  );

  it("a deep path also refuses an array-valued parent", () => {
    expect(() =>
      projection({ ...envelope, lastChanged: [lastChanged] }, [
        "_meta/lastChanged/date",
      ]),
    ).toThrow(/unmeasured/);
  });

  // Round 5: wildcard returns must not bypass any child-path refusal.
  for (const method of ["read", "query", "_fields array", "_fields string"]) {
    it.each([
      { fields: ["*", "_meta/lastChanged"] },
      { fields: ["_meta/lastChanged", "*"] },
      { fields: ["*,_meta/lastChanged"] },
      { fields: ["_meta/lastChanged,*"] },
    ])(
      `${method} preflights wildcard/child selectors $fields`,
      ({ fields }) => {
        for (const parent of [
          { _ref: "managed/alpha_usermeta/meta-example" },
          "scalar",
          1,
          null,
          [meta],
        ]) {
          expect(() => projection(parent, fields, method)).toThrow(
            /unmeasured/,
          );
        }
        // Even with a measured parent the combined selector is unmeasured.
        expect(() => projection(meta, fields, method)).toThrow(
          /combining \* with parent\/child selectors is unmeasured/,
        );
      },
    );

    it(`${method} still permits a standalone wildcard`, () => {
      expect(projection(meta, ["*"], method).sharedState.final.row).toEqual({
        _id: "example",
        _rev: "1",
        _meta: meta,
      });
    });
  }

  it("a wildcard also cannot bypass deep child validation", () => {
    expect(() =>
      projection({ ...envelope, lastChanged: [lastChanged] }, [
        "*,_meta/lastChanged/date",
      ]),
    ).toThrow(/scalar or array parent is unmeasured/);
  });

  it("the explicit fields argument still takes precedence over unmeasured params selectors", () => {
    const effects = runScript(
      'nodeState.putShared("row", openidm.query("managed/alpha_user", {_queryFilter:"true", _fields:"*,_meta/lastChanged"}, ["_id"]).result[0]);',
      {
        managed: {
          "managed/alpha_user": [
            {
              _id: "example",
              _rev: "1",
              _meta: { _ref: "managed/alpha_usermeta/meta-example" },
            },
          ],
        },
      },
    );
    expect(effects.sharedState.final.row).toEqual({
      _id: "example",
      _rev: "1",
    });
  });
});
