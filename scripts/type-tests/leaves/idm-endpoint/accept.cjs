logger.info("plain");
logger.debug("one {} bound", request.method);
logger.warn("two {} and {}", 1, true);
logger.error("escaped \\{} literal");
logger.trace("double \\\\{} bound", "x");
var msg = "built " + String(request.method);
logger.info(msg, 1, 2, 3);

// --- managed-record projection ---------------------------------------------
// Every line drives a branch that an EMPTY `ManagedObjects` would leave
// uninstantiated. Behaviour verified live in docs/api/10-managed-objects.md.
//
// Guarded reads are not enough on their own: `if (rec.manager) { … }` compiles
// under a correct projection AND under a broken one. Each block below either
// pins a shape by assignment or has a matching UNGUARDED case in reject.cjs.

// StoredRecord: a read always carries `_id` and `_rev`, and they are strings —
// not the optional ones the interface declares for an onCreate draft.
var whole = openidm.read("managed/__aic_fixture_user/alice");
if (whole) {
  /** @type {string} */
  var wholeId = whole._id;
  /** @type {string} */
  var wholeRev = whole._rev;
  logger.info("read {} rev {}", wholeId, wholeRev);
}

// ManagedRecordOf: a record path resolves to ITS OWN object, not to whichever
// interface the map happens to hold.
var device = openidm.read("managed/__aic_fixture_device/d-1");
if (device) {
  logger.info("device {} model {}", device.deviceId, device.model);
}

// SelectedMembers: a schema-optional property projects as a REQUIRED key whose
// value MAY be null. The annotation pins requiredness; the reject file pins the
// nullability, because `string` is assignable to `string | null` and this line
// alone would survive losing the `| null`.
var projected = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "userName",
  "telephoneNumber",
]);
if (projected) {
  /** @type {string | null} */
  var phone = projected.telephoneNumber;
  /** @type {string} */
  var required = projected.userName;
  logger.info("{} {}", required, phone === null ? "none" : phone);
}

// ExpansionOf, multi-valued: an array, never null, so `.length` needs no guard.
var roles = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "authzRoles/name",
]);
if (roles) {
  logger.info("role count {}", roles.authzRoles.length);
}

// MetaMemberOf: both spellings add the expansion.
var metaPath = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "_meta/lastChanged",
]);
if (metaPath && metaPath._meta) {
  logger.info("meta {}", metaPath._meta["lastChanged"]);
}
var metaBare = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "_meta",
]);
if (metaBare && metaBare._meta) {
  logger.info("meta bare {}", metaBare._meta._ref);
}

// `*` keeps the whole record.
var star = openidm.read("managed/__aic_fixture_user/alice", undefined, ["*"]);
if (star) {
  logger.info("star {}", star.sn);
}

// query projects every row the same way a read does.
var rows = openidm.query(
  "managed/__aic_fixture_user",
  { _queryFilter: 'userName eq "alice"' },
  ["userName"]
);
// `noUncheckedIndexedAccess` is on, so the element needs binding and a guard.
var row = rows.result[0];
if (row) {
  /** @type {string} */
  var rowId = row._id;
  logger.info("row {} {}", rowId, row.userName);
}

// query does not merely NARROW — it runs the same `Projected` as `read`. A
// relationship path must expand on a row, and an optional scalar must come back
// required-and-nullable. Without these, `QueryResult<StoredRecord<
// SelectedMembers<…>>>` would pass every other query case here: it narrows away
// an unselected field and carries `_id`, and carries none of the rest.
var qexpanded = openidm.query(
  "managed/__aic_fixture_user",
  { _queryFilter: "true" },
  ["telephoneNumber", "manager/displayName", "authzRoles/name"]
);
var qexpandedRow = qexpanded.result[0];
if (qexpandedRow) {
  logger.info(
    "phone {}",
    qexpandedRow.telephoneNumber === null
      ? "none"
      : qexpandedRow.telephoneNumber
  );
  // Multi-valued expansion on a query row: an array, never null.
  logger.info("roles {}", qexpandedRow.authzRoles.length);
  if (qexpandedRow.manager) {
    logger.info("manager {}", qexpandedRow.manager._ref);
  }
}

// An unknown path keeps the loose fallback: free-form fields, `any` result.
var other = openidm.read("internal/role/openidm-admin", undefined, ["name"]);
logger.info("other {}", other);

// ContentArg: a write takes a PARTIAL of the object, on both the collection
// path (create) and the record path (update/patch).
openidm.create("managed/__aic_fixture_user", null, {
  userName: "bob",
  sn: "b",
});
openidm.update("managed/__aic_fixture_user/alice", null, { sn: "smith" });
openidm.patch("managed/__aic_fixture_user/alice", null, [
  { operation: "replace", field: "sn", value: "smith" },
]);
openidm.create("managed/__aic_fixture_device", null, { deviceId: "d-2" });
