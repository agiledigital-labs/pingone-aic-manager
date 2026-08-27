logger.info("two {} and {}", 1); // expect: TS2345 — deficit
logger.debug("none", 1); // expect: TS2345 — surplus non-throwable
logger.error("boom", undefined); // expect: TS2345 — a dropped argument, not a throwable

// --- managed-record projection ---------------------------------------------
// Each marked line must produce exactly ONE diagnostic, so anything that would
// also trip the null check on `openidm.read` is bound and guarded first.

// A misspelled field in a `fields` list, pinned to the offending element.
openidm.read("managed/__aic_fixture_user/alice", undefined, ["userNam"]); // expect: TS2820 — tsc suggests userName

// A misspelled field in `content`.
openidm.create("managed/__aic_fixture_user", null, { userNam: "bob" }); // expect: TS2561 — tsc suggests userName

// A misspelled property on the record.
var rec = openidm.read("managed/__aic_fixture_user/alice");
if (rec) {
  rec.userNam; // expect: TS2551 — tsc suggests userName
}

// ManagedRecordOf: a device path must NOT resolve to the user interface.
var dev = openidm.read("managed/__aic_fixture_device/d-1");
if (dev) {
  dev.userName; // expect: TS2339 — that property belongs to the other object
}

// FieldsArg: an UNKNOWN managed path has no schema to check against, so its
// `fields` argument is `never` rather than free-form — pull the schema again.
openidm.read("managed/__aic_fixture_absent/x", undefined, ["anything"]); // expect: TS2345 — unknown managed path

// SelectedMembers: a field the projection did not ask for is genuinely absent.
var narrow = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "sn",
]);
if (narrow) {
  narrow.mail; // expect: TS2339 — not selected, so not on the projected type
}

// SelectedMembers, the other half: a projected optional is NULLABLE. Assigning
// it to a plain `string` must fail — the accept file's `string | null`
// annotation pins requiredness only and survives losing the `| null`.
var nullable = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "telephoneNumber",
]);
if (nullable) {
  /** @type {string} */
  var notNull = nullable.telephoneNumber; // expect: TS2322 — may be null
  logger.info("{}", notNull);
}

// ExpansionOf, single-valued: `expansion | null`. Typing it always-present cost
// a live 500 on the first request against a user with no manager, which every
// gate had passed. An unguarded read must not compile.
var withManager = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "manager/displayName",
]);
if (withManager) {
  withManager.manager._ref; // expect: TS18047 — single-valued expansion is nullable
}

// PathParentOf: selecting one relationship must not expand the other.
var onlyManager = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "manager/displayName",
]);
if (onlyManager) {
  onlyManager.authzRoles; // expect: TS2339 — not requested, so not projected
}

// MetaMemberOf: `_meta` is itself a relationship, so it is nullable...
var meta = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "_meta/lastChanged",
]);
if (meta) {
  meta._meta._ref; // expect: TS18047 — the _meta expansion is nullable
}
// ...and it is absent unless requested.
var noMeta = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "userName",
]);
if (noMeta) {
  noMeta._meta; // expect: TS2339 — _meta was not requested
}

// query projects rows exactly as read does: an unselected field is absent.
var qrows = openidm.query(
  "managed/__aic_fixture_user",
  { _queryFilter: 'userName eq "alice"' },
  ["userName"]
);
var qrow = qrows.result[0];
if (qrow) {
  qrow.mail; // expect: TS2339 — not selected, so not on the projected row
}
