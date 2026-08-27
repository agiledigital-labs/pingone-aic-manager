logger.info("plain");
logger.debug("one {} bound", nodeState.get("username"));
logger.warn("two {} and {}", 1, true);
logger.trace("escaped \\{} literal");
logger.error("double \\\\{} bound", "x");
var msg = "built at " + scriptName;
logger.error(msg, 1, 2);
logger.info(`user ${realm} said {}`, "hi");

// --- managed-record projection ---------------------------------------------
// Every line drives a branch of Projected / SelectedMembers / ExpansionOf /
// MetaMemberOf / ManagedRecordOf, which an empty `ManagedObjects` would leave
// uninstantiated. Behaviour verified live in docs/api/10-managed-objects.md.

// A record read always carries _id and _rev, whatever the interface says.
var whole = openidm.read("managed/__aic_fixture_user/alice");
if (whole) {
  logger.info("read {} rev {}", whole._id, whole._rev);
  logger.info("user {}", whole.userName);
}

// A `fields` projection narrows the result and STILL keeps _id/_rev.
var picked = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "userName",
  "telephoneNumber",
]);
if (picked) {
  logger.info("id {} name {}", picked._id, picked.userName);
  // A schema-optional property comes back as a REQUIRED key holding null.
  logger.info(
    "phone {}",
    picked.telephoneNumber === null ? "none" : picked.telephoneNumber
  );
}

// A relationship path upgrades the parent key to the expansion envelope, and
// cardinality decides the shape: single-valued is `expansion | null`.
var expanded = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "userName",
  "manager/displayName",
]);
if (expanded && expanded.manager) {
  logger.info("manager ref {}", expanded.manager._ref);
}
// Multi-valued is an ARRAY — `[]` when unset, never null.
var multi = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "authzRoles/name",
]);
if (multi) {
  logger.info("roles {}", multi.authzRoles.length);
}

// `_meta` is a relationship in its own right.
var meta = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "_meta/lastChanged",
]);
if (meta && meta._meta) {
  logger.info("meta {}", meta._meta["lastChanged"]);
}

// `*` keeps the whole record.
var star = openidm.read("managed/__aic_fixture_user/alice", undefined, ["*"]);
if (star) {
  logger.info("star {}", star.sn);
}

// query projects every row the same way.
var rows = openidm.query(
  "managed/__aic_fixture_user",
  { _queryFilter: 'userName eq "alice"' },
  ["userName"]
);
for (var i = 0; i < rows.result.length; i++) {
  // `noUncheckedIndexedAccess` is on, so the element needs binding and a guard
  // before its projected members are readable.
  var row = rows.result[i];
  if (row) {
    logger.info("row {} {}", row._id, row.userName);
  }
}

// An unknown path keeps the loose fallback: `fields` is free-form, result any.
var other = openidm.read("internal/role/openidm-admin", undefined, ["name"]);
logger.info("other {}", other);

// Writes take a Partial of the object.
openidm.create("managed/__aic_fixture_user", null, {
  userName: "bob",
  sn: "b",
});
openidm.patch("managed/__aic_fixture_user/alice", null, [
  { operation: "replace", field: "sn", value: "smith" },
]);

// --- Lookup<T>: a Java collection takes a JS string literal ------------------
// The other small conditional in the ambient declarations. Every family reaches
// a Java collection with a plain literal — `scopes.contains("openid")` — and
// typing the parameter as the collection's own element type rejected all of
// them. The widening has to stay conditional: `any` would take anything.

var headers = requestHeaders["content-type"];
if (headers) {
  logger.info("ct {}", String(headers[0]));
}
var params = requestParameters["realm"];
if (params && params.includes("alpha")) {
  logger.info("realm param present");
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
