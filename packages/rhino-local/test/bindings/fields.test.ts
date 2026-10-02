import { describe, expect, it } from "vitest";
import { runScript } from "./load-behaviour.ts";

describe("comma-joined fields (#17)", () => {
  const record = { _id: "example", _rev: "1", givenName: "Example", sn: "User", mail: "example@example.com", parent: { child: true } };
  const rows = [
    { fields: ["givenName", "sn"], keys: ["_id", "_rev", "givenName", "sn"] },
    { fields: ["givenName,sn"], keys: ["_id", "_rev", "givenName", "sn"] },
    { fields: ["givenName, sn"], keys: ["_id", "_rev", "givenName"] },
    { fields: ["givenName,sn", "mail"], keys: ["_id", "_rev", "givenName", "mail", "sn"] },
    { fields: ["givenName,parent/child"], keys: ["_id", "_rev", "givenName", "parent"] },
    { fields: ["givenName,*"], keys: Object.keys(record).sort() },
  ];
  for (const method of ["read", "query", "params"] as const) {
    // A split-and-trim implementation would incorrectly return sn in row 3.
    it.each(rows)(`${method} projects $fields`, ({ fields, keys }) => {
      const call = method === "read"
        ? `openidm.read("managed/alpha_user/example", null, ${JSON.stringify(fields)})`
        : method === "query"
          ? `openidm.query("managed/alpha_user", {_queryFilter: "true"}, ${JSON.stringify(fields)}).result[0]`
          : `openidm.query("managed/alpha_user", {_queryFilter: "true", _fields: ${JSON.stringify(fields.join(","))}}).result[0]`;
      const effects = runScript(`nodeState.putShared("keys", Object.keys(${call}).sort());`, {
        managed: { "managed/alpha_user": [record] },
      });
      expect(effects.sharedState.final.keys).toEqual(keys);
    });
  }
});
