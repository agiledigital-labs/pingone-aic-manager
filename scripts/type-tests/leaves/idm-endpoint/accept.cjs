logger.info("plain");
logger.debug("one {} bound", request.method);
logger.warn("two {} and {}", 1, true);
logger.error("escaped \\{} literal");
logger.trace("double \\\\{} bound", "x");
var msg = "built " + String(request.method);
logger.info(msg, 1, 2, 3);

// --- managed-record projection (the IDM ambient copy) -----------------------
// The same machinery as am/types/nextgen-common.d.ts, shipped separately
// because the two workspaces cannot share a program. Exercised here so a fix to
// one copy that misses the other fails a gate rather than a client's build.

var whole = openidm.read("managed/__aic_fixture_user/alice");
if (whole) {
  logger.info("read {} rev {}", whole._id, whole._rev);
}

var picked = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "userName",
  "telephoneNumber",
]);
if (picked) {
  // Schema-optional property: a required key holding null, not an absent key.
  logger.info(
    "phone {}",
    picked.telephoneNumber === null ? "none" : picked.telephoneNumber
  );
}

// Single-valued relationship: expansion or null. Multi-valued: an array.
var expanded = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "manager/displayName",
  "authzRoles/name",
]);
if (expanded) {
  logger.info("roles {}", expanded.authzRoles.length);
  if (expanded.manager) {
    logger.info("manager {}", expanded.manager._ref);
  }
}

var rows = openidm.query(
  "managed/__aic_fixture_user",
  { _queryFilter: 'userName eq "alice"' },
  ["userName"]
);
var row = rows.result[0];
if (row) {
  logger.info("row {} {}", row._id, row.userName);
}

// --- discriminating shape checks -------------------------------------------
// The guards above compile under a CORRECT projection and under a broken one,
// so on their own they prove nothing. These lines pin the exact shapes, and
// each fails if the corresponding property is lost:

// A schema-optional property projects as a REQUIRED key holding `string | null`
// — not an optional key, and not non-null. `Pick` was wrong here twice.
var projected = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "telephoneNumber",
]);
if (projected) {
  /** @type {string | null} */
  var phone = projected.telephoneNumber;
  logger.info("phone {}", phone);
}

// A MULTI-valued relationship is an array and is never null, so `.length` needs
// no guard. If cardinality handling collapses, this line stops compiling.
var roles = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "authzRoles/name",
]);
if (roles) {
  logger.info("role count {}", roles.authzRoles.length);
}
