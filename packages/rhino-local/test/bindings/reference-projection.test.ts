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
    "%s refuses scalar, non-reference array and null parents rather than inventing their projection",
    (method) => {
      // An array of `_ref` objects is a measured relationship array (relationship-array-projection.test.ts).
      for (const parent of ["scalar", 1, true, null, [lastChanged]]) {
        expect(() => projection(parent, ["_meta/lastChanged"], method)).toThrow(
          /fields "_meta\/lastChanged": projecting a child of .* is unmeasured/,
        );
      }
    },
  );

  it.each(["read", "query"])(
    "%s refuses unresolved references instead of expanding seeded targets",
    (method) => {
      const parent = { _ref: "managed/alpha_usermeta/meta-example" };
      expect(() => projection(parent, ["_meta/lastChanged"], method)).toThrow(
        /carries no lastChanged, and reference expansion is unmeasured/,
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
      expect(() => projection([lastChanged], fields)).toThrow(/unmeasured/);
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
          [lastChanged],
        ]) {
          expect(() => projection(parent, fields, method)).toThrow(
            /unmeasured/,
          );
        }
        // Even with a measured parent the combined selector is unmeasured.
        expect(() => projection(meta, fields, method)).toThrow(
          /combining \* with a child selector is unmeasured unless the parent is a relationship array, and _meta is an object/,
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
      // The object parent refuses the wildcard combination before the deep path is reached.
    ).toThrow(/combining \* with a child selector is unmeasured/);
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

// astra-e15ab r3 P2: projection copied selected keys by assignment, so in the vm lane an own
// `__proto__` field set the result's prototype instead and vanished; Rhino keeps it as data.
describe("projection keeps an own __proto__ field as data", () => {
  const record = JSON.parse(
    '{"_id":"x","_rev":"1","__proto__":"yes","box":{"__proto__":"nested","q":1},' +
      '"roles":[{"_ref":"managed/alpha_role/r","_id":"r","__proto__":"el","name":"r"}]}',
  ) as JsonValue;
  const read = (fields: string[]) =>
    JSON.parse(
      runScript(
        `nodeState.putShared("row", JSON.stringify(openidm.read("managed/alpha_user/x", null, ${JSON.stringify(fields)})));`,
        { managed: { "managed/alpha_user": [record] } as never },
      ).sharedState.final.row as string,
    ) as Record<string, unknown>;
  const own = (o: unknown, key: string) => Object.prototype.hasOwnProperty.call(o, key) ? (o as Record<string, unknown>)[key] : "<absent>";

  it.each([
    [["__proto__"], (r: Record<string, unknown>) => own(r, "__proto__"), "yes"],
    [["box/__proto__"], (r: Record<string, unknown>) => own(r.box, "__proto__"), "nested"],
    [["__proto__", "box/q"], (r: Record<string, unknown>) => [own(r, "__proto__"), own(r.box, "q")], ["yes", 1]],
    [["roles/*"], (r: Record<string, unknown>) => own((r.roles as unknown[])[0], "__proto__"), "el"],
    [["roles/*/__proto__"], (r: Record<string, unknown>) => own((r.roles as unknown[])[0], "__proto__"), "el"],
  ] as const)("%j", (fields, pick, expected) => {
    expect(pick(read([...fields]))).toEqual(expected);
  });
});
