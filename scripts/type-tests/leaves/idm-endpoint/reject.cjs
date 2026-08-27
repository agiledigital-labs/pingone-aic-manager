logger.info("two {} and {}", 1); // expect: TS2345 — deficit
logger.debug("none", 1); // expect: TS2345 — surplus non-throwable
logger.error("boom", undefined); // expect: TS2345 — a dropped argument, not a throwable

openidm.read("managed/__aic_fixture_user/alice", undefined, ["userNam"]); // expect: TS2820 — tsc suggests userName
openidm.create("managed/__aic_fixture_user", null, { userNam: "bob" }); // expect: TS2561 — tsc suggests userName

var narrow = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "sn",
]);
if (narrow) {
  narrow.mail; // expect: TS2339 — not selected, so not on the projected type
}

// A SINGLE-valued relationship expansion is `expansion | null` — unset comes
// back null. Typing it always-present cost a live 500 on the first request
// against a user with no manager, which every gate had passed. Reading it
// without a null check must not compile.
var withManager = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "manager/displayName",
]);
if (withManager) {
  withManager.manager._ref; // expect: TS18047 — single-valued expansion is nullable
}
