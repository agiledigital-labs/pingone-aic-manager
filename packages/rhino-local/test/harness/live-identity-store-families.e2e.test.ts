import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, managed, useLease } from "../../src/harness/index.ts";

const ID = "00000000-0000-4000-8000-0000000000dc";
const RESOURCE = `managed/alpha_user/${ID}`;

const REFUSED = (code: number) =>
  "threw:JavaException: org.forgerock.openam.scripting.api.identity.ScriptedIdentityScriptWrapper$IdentityUpdateException: " +
  "Exception persisting attribute: Plug-in org.forgerock.openam.idrepo.ldap.DJLDAPv3Repo encountered a ldap exception.  " +
  `ldap errorcode=${code}`;

// One store() per row, each through a fresh getIdentity wrapper, in order, so
// a refused row shows the previous row's value still in place.
// [label, AM attribute(s) -> values, IDM property read back, store result,
//  IDM JSON after, AM getAttributeValues JSON after]
type Row = [string, Record<string, string[]>, string, string, string, string];
const ABSENT = "absent";
const ROWS: Row[] = [
  ["istr", { "fr-attr-istr1": ["indexed"] }, "frIndexedString1", "ok", '"indexed"', '["indexed"]'],
  ["istrTwo", { "fr-attr-istr1": ["p", "q"] }, "frIndexedString1", REFUSED(65), '"indexed"', '["indexed"]'],
  ["istrEmpty", { "fr-attr-istr1": [] }, "frIndexedString1", "ok", ABSENT, "[]"],
  ["imultiOne", { "fr-attr-imulti1": ["only"] }, "frIndexedMultivalued1", "ok", '["only"]', '["only"]'],
  ["imultiTwo", { "fr-attr-imulti1": ["a", "b"] }, "frIndexedMultivalued1", "ok", '["a","b"]', '["a","b"]'],
  ["imultiEmpty", { "fr-attr-imulti1": [] }, "frIndexedMultivalued1", "ok", "[]", "[]"],
  ["idate", { "fr-attr-idate1": ["20261001120000Z"] }, "frIndexedDate1", "ok", '"2026-10-01T12:00:00Z"', '["20261001120000Z"]'],
  ["idateIso", { "fr-attr-idate1": ["2026-10-02T12:00:00Z"] }, "frIndexedDate1", REFUSED(21), '"2026-10-01T12:00:00Z"', '["20261001120000Z"]'],
  ["idateDay", { "fr-attr-idate1": ["2026-10-03"] }, "frIndexedDate1", REFUSED(21), '"2026-10-01T12:00:00Z"', '["20261001120000Z"]'],
  ["idateText", { "fr-attr-idate1": ["not-a-date"] }, "frIndexedDate1", REFUSED(21), '"2026-10-01T12:00:00Z"', '["20261001120000Z"]'],
  ["idateTwo", { "fr-attr-idate1": ["20261001120000Z", "20261002120000Z"] }, "frIndexedDate1", REFUSED(65), '"2026-10-01T12:00:00Z"', '["20261001120000Z"]'],
  ["idateEmpty", { "fr-attr-idate1": [] }, "frIndexedDate1", "ok", ABSENT, "[]"],
  ["date", { "fr-attr-date1": ["20261005083000Z"] }, "frUnindexedDate1", "ok", '"2026-10-05T08:30:00Z"', '["20261005083000Z"]'],
  ["dateIso", { "fr-attr-date1": ["2026-10-02T12:00:00Z"] }, "frUnindexedDate1", REFUSED(21), '"2026-10-05T08:30:00Z"', '["20261005083000Z"]'],
  ["dateTwo", { "fr-attr-date1": ["20261001120000Z", "20261002120000Z"] }, "frUnindexedDate1", REFUSED(65), '"2026-10-05T08:30:00Z"', '["20261005083000Z"]'],
  ["dateEmpty", { "fr-attr-date1": [] }, "frUnindexedDate1", "ok", ABSENT, "[]"],
  ["iint", { "fr-attr-iint1": ["42"] }, "frIndexedInteger1", "ok", "42", '["42"]'],
  ["iintText", { "fr-attr-iint1": ["abc"] }, "frIndexedInteger1", REFUSED(21), "42", '["42"]'],
  ["iintTwo", { "fr-attr-iint1": ["1", "2"] }, "frIndexedInteger1", REFUSED(65), "42", '["42"]'],
  ["iintEmpty", { "fr-attr-iint1": [] }, "frIndexedInteger1", "ok", ABSENT, "[]"],
  ["int", { "fr-attr-int1": ["7"] }, "frUnindexedInteger1", "ok", "7", '["7"]'],
  ["intFraction", { "fr-attr-int1": ["4.5"] }, "frUnindexedInteger1", REFUSED(21), "7", '["7"]'],
  ["intTwo", { "fr-attr-int1": ["1", "2"] }, "frUnindexedInteger1", REFUSED(65), "7", '["7"]'],
  ["intEmpty", { "fr-attr-int1": [] }, "frUnindexedInteger1", "ok", ABSENT, "[]"],
  ["strTwo", { "fr-attr-str1": ["p", "q"] }, "frUnindexedString1", REFUSED(65), '"old"', '["old"]'],
  ["strEmpty", { "fr-attr-str1": [] }, "frUnindexedString1", "ok", ABSENT, "[]"],
  ["multiEmpty", { "fr-attr-multi1": [] }, "frUnindexedMultivalued1", "ok", "[]", "[]"],
  ["given", { givenName: ["Given"] }, "givenName", "ok", '"Given"', '["Given"]'],
  ["givenTwo", { givenName: ["x", "y"] }, "givenName", "ok", '["x","y"]', '["x","y"]'],
  ["givenEmpty", { givenName: [] }, "givenName", "ok", ABSENT, "[]"],
  ["sn", { sn: ["Surname"] }, "sn", "ok", '"Surname"', '["Surname"]'],
  ["snTwo", { sn: ["p", "q"] }, "sn", "ok", '["p","q"]', '["p","q"]'],
  ["snEmpty", { sn: [] }, "sn", REFUSED(65), '["p","q"]', '["p","q"]'],
  ["tel", { telephoneNumber: ["+61 2 5550 0000"] }, "telephoneNumber", "ok", '"+61 2 5550 0000"', '["+61 2 5550 0000"]'],
  ["telTwo", { telephoneNumber: ["1", "2"] }, "telephoneNumber", "ok", '["1","2"]', '["1","2"]'],
  ["telEmpty", { telephoneNumber: [] }, "telephoneNumber", "ok", ABSENT, "[]"],
  ["mailTwo", { mail: ["p@example.com", "q@example.com"] }, "mail", "ok", '["p@example.com","q@example.com"]', '["p@example.com","q@example.com"]'],
  ["mailEmpty", { mail: [] }, "mail", "ok", ABSENT, "[]"],
  // A store DS refuses applies none of its attributes, the good one included.
  ["mixed", { "fr-attr-str3": ["good"], "fr-attr-int2": ["bad"] }, "frUnindexedString3", REFUSED(21), ABSENT, "[]"],
];

const lines = ROWS.map(([label, writes, idm]) => {
  const first = Object.keys(writes)[0];
  const sets = Object.entries(writes)
    .map(([attribute, values]) => `w.setAttribute(${JSON.stringify(attribute)}, ${JSON.stringify(values)});`)
    .join(" ");
  return [
    "(function () {",
    "  var w = idRepository.getIdentity(id);",
    `  nodeState.putShared("${label}Store", t(function () { ${sets} w.store(); return "ok"; }));`,
    `  nodeState.putShared("${label}Idm", t(function () { return idm("${idm}"); }));`,
    `  nodeState.putShared("${label}Am", t(function () { return am(${JSON.stringify(first)}); }));`,
    "})();",
  ].join("\n");
});

const suite = defineSuite({
  name: "identity-store-families",
  script: [
    'function t(f) { try { return String(f()); } catch (e) { return "threw:" + e; } }',
    `var id = "${ID}";`,
    `var path = "managed/alpha_user/${ID}";`,
    // An unset property reads null on AIC and is missing locally; that is a
    // separate, known difference (docs/rhino-local-gaps.md), so both read as
    // "absent" here. Every present value is compared exactly.
    'function idm(f) { var v = openidm.read(path, null, [f])[f]; return v === null || v === undefined ? "absent" : JSON.stringify(v); }',
    "function am(a) { return JSON.stringify(idRepository.getIdentity(id).getAttributeValues(a).toArray().map(String)); }",
    ...lines,
    'action.goTo("done");',
  ].join("\n"),
  outcomes: ["done"],
  fixtures: {
    subject: managed("managed/alpha_user", {
      _id: ID, userName: "identity-families-probe", mail: "identity-families-probe@example.com",
      givenName: "Identity", sn: "Probe", frUnindexedString1: "old", frUnindexedMultivalued1: ["first"],
    }),
  },
  cleanup: async (idm) => {
    await idm.delete(RESOURCE);
  },
});

const added: Record<string, string> = {};
const writes: { identity: string; attribute: string; values: string[] }[] = [];
for (const [label, rowWrites, , store, idm, am] of ROWS) {
  added[`${label}Store`] = store;
  added[`${label}Idm`] = idm;
  added[`${label}Am`] = am;
  if (store === "ok") {
    for (const [attribute, values] of Object.entries(rowWrites)) {
      writes.push({ identity: ID, attribute, values });
    }
  }
}

describe("identity store families", () => {
  const lease = useLease(suite, aicWhenEnabled("live-identity-store-families"));

  // Measured 2026-10-01 on alpha. Each fr-attr family is represented by its
  // first member; standard attributes are multi-valued in DS.
  it("lays each attribute family out in IDM, and refuses what DS refuses", async () => {
    const run = await lease.run().expect({
      outcome: "done",
      sharedState: { added },
      identityWrites: writes,
    });
    expect(run.verdict.mismatches).toEqual([]);
    expect(run.verdict.pass).toBe(true);
  });
});
