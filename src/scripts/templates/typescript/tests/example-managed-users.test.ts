// `example-managed-users`, driven through `dispatch` the way IDM drives it.
//
// The handlers there call `openidm`, which does not exist under node, so these
// tests deliberately stop at the gates that run BEFORE a handler: routing and
// per-route validation. That is the whole reachable surface without a store
// double, and it is the half that a typo in a route declaration breaks.
//
// The typed `openidm` surface itself is covered by `npm run type-check` — the
// point of that endpoint is that its mistakes are build errors, not test
// failures.
//
// User file — seeded once, yours to change.

import assert from "node:assert/strict";
import { test } from "node:test";

import type { CrestErrorResponse } from "../framework/index.ts";
import users, {
  USER_COLLECTION,
} from "../src/endpoints/example-managed-users.ts";
import { callContext, crestErrorFrom, crestRequest } from "./harness.ts";

/**
 * Which inputs a validation failure blamed. The caller-facing `message` is the
 * same generic line for every one of them, so asserting on it would pass
 * whatever broke — the paths are the part that pins the route declaration.
 */
function issuePaths(error: CrestErrorResponse): string[] {
  const detail = error.detail as { issues?: { path: string }[] } | undefined;
  return (detail?.issues ?? []).map((issue) => issue.path);
}

test("the collection path is one the tenant actually has", () => {
  // Typed as `ManagedName`, so this is really a compile-time assertion; the
  // runtime check is here so a regenerated schema that DROPS alpha_user fails
  // loudly rather than leaving a dead endpoint.
  assert.equal(USER_COLLECTION, "managed/alpha_user");
});

test("every route is declared once, with the CREST method it needs", () => {
  const routes = users.definition.routes.map(
    (route) => route.method + " " + route.path
  );
  assert.deepEqual(routes, [
    "read /{userId}",
    "read /{userId}/contact",
    "query /",
  ]);
});

test("a userId that is not a UUID is a 400 before any store call", () => {
  const error = crestErrorFrom(() =>
    users.dispatch(
      crestRequest("read", { resourcePath: "not-a-uuid" }),
      callContext()
    )
  );
  assert.equal(error.code, 400);
  // `path.`, not `params.`: the issue path names where the value came from.
  assert.deepEqual(issuePaths(error), ["path.userId"]);
});

test("pageSize above its declared maximum is a 400", () => {
  const error = crestErrorFrom(() =>
    users.dispatch(
      crestRequest("query", { query: { pageSize: "5000" } }),
      callContext()
    )
  );
  assert.equal(error.code, 400);
  assert.deepEqual(issuePaths(error), ["query.pageSize"]);
});

test("an unknown account status is a 400, not a query for it", () => {
  const error = crestErrorFrom(() =>
    users.dispatch(
      crestRequest("query", { query: { status: "archived" } }),
      callContext()
    )
  );
  assert.equal(error.code, 400);
  assert.deepEqual(issuePaths(error), ["query.status"]);
});

test("a sub-path no route matches is a 404", () => {
  const error = crestErrorFrom(() =>
    users.dispatch(
      crestRequest("read", { resourcePath: "nope/nope/nope" }),
      callContext()
    )
  );
  assert.equal(error.code, 404);
});
