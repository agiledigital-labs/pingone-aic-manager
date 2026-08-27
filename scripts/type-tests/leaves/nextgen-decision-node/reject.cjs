logger.info("two {} and {}", 1); // expect: TS2345 — deficit
logger.debug("none", 1); // expect: TS2345 — surplus non-throwable
logger.warn("one {}", 1, 2); // expect: TS2345 — surplus non-throwable
// A misspelled field in a `fields` list, pinned to the offending element.
openidm.read("managed/__aic_fixture_user/alice", undefined, ["userNam"]); // expect: TS2820 — tsc suggests userName

// A misspelled field in `content`.
openidm.create("managed/__aic_fixture_user", null, { userNam: "bob" }); // expect: TS2561 — tsc suggests userName

// A misspelled property on the record. Bound and guarded first, so the only
// diagnostic on the marked line is the spelling one and not the null check.
var rec = openidm.read("managed/__aic_fixture_user/alice");
if (rec) {
  rec.userNam; // expect: TS2551 — tsc suggests userName
}

// A field the projection did not ask for is genuinely absent from the result.
var narrow = openidm.read("managed/__aic_fixture_user/alice", undefined, [
  "sn",
]);
if (narrow) {
  narrow.mail; // expect: TS2339 — not selected, so not on the projected type
}

// Lookup<T> widens for a Java STRING collection and must not widen for others.
var nums = /** @type {JavaArray<number>} */ (/** @type {unknown} */ ([]));
nums.includes("2"); // expect: TS2345 — a number collection does not take a string

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
