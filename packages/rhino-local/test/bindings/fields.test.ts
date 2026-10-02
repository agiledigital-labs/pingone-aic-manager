import { describe, expect, it } from "vitest";
import type { JsonObject } from "../../src/case/types.ts";
import { runScript } from "./load-behaviour.ts";

describe("comma-joined fields (#17)", () => {
  const record = { _id: "example", _rev: "1", givenName: "Example", sn: "User", mail: "example@example.com", parent: { _ref: "managed/example/parent", child: true, unwanted: "omit" } };
  const rows = [
    { fields: ["givenName", "sn"], keys: ["_id", "_rev", "givenName", "sn"] },
    { fields: ["givenName,sn"], keys: ["_id", "_rev", "givenName", "sn"] },
    { fields: ["givenName, sn"], keys: ["_id", "_rev", "givenName"] },
    { fields: ["givenName,sn", "mail"], keys: ["_id", "_rev", "givenName", "mail", "sn"] },
    { fields: ["givenName,parent/child"], keys: ["_id", "_rev", "givenName", "parent"] },
    { fields: ["givenName,*"], keys: Object.keys(record).sort() },
  ];
  for (const method of ["read", "query", "params", "paramsArray"] as const) {
    // A split-and-trim implementation would incorrectly return sn in row 3.
    it.each(rows)(`${method} projects $fields`, ({ fields, keys }) => {
      const call = method === "read"
        ? `openidm.read("managed/alpha_user/example", null, ${JSON.stringify(fields)})`
        : method === "query"
          ? `openidm.query("managed/alpha_user", {_queryFilter: "true"}, ${JSON.stringify(fields)}).result[0]`
          : `openidm.query("managed/alpha_user", {_queryFilter: "true", _fields: ${JSON.stringify(method === "paramsArray" ? fields : fields.join(","))}}).result[0]`;
      const effects = runScript(`nodeState.putShared("projected", ${call});`, {
        managed: { "managed/alpha_user": [record] },
      });
      const projected: JsonObject = Object.fromEntries(keys.map((key) => [key, record[key as keyof typeof record]]));
      if (fields.includes("givenName,parent/child")) projected.parent = { _ref: "managed/example/parent", child: true };
      expect(effects.sharedState.final.projected).toEqual(projected);
    });
  }
  it("the explicit third argument takes precedence over params._fields", () => {
    const effects = runScript('nodeState.putShared("projected", openidm.query("managed/alpha_user", {_queryFilter: "true", _fields: ["mail"]}, ["givenName"]).result[0]);', {
      managed: { "managed/alpha_user": [record] },
    });
    expect(effects.sharedState.final.projected).toEqual({ _id: "example", _rev: "1", givenName: "Example" });
  });

});
