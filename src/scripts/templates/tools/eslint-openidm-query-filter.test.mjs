// Repo-side tests only. Deliberately not listed in MANAGED in
// src/scripts/workspace.rs, so this file never ships to user workspaces.
// Run with: node --test src/scripts/templates/tools/
// The sibling package.json (also repo-side only) exists solely so that
// directory form resolves; without it node's test runner fails the dir entry.

import assert from "node:assert/strict";
import test from "node:test";

import {
  cookedToRawIndexMap,
  rawOffsetForIndex,
  validateQueryFilterFields,
} from "./eslint-openidm-query-filter.mjs";

const KNOWN_FIELDS = ["_id", "_rev", "userName", "accountStatus"];

function issuesFor(filter) {
  return validateQueryFilterFields(filter, KNOWN_FIELDS);
}

test("accepts valid query filters", () => {
  for (const filter of [
    '/userName eq "x"',
    'userName eq "x"',
    '(/userName eq "x" or /userName co "y") and /accountStatus pr',
    '!(/userName eq "x")',
    "true",
    "/userName eq 42",
    "/userName eq -1.25",
    '/userName/foo eq "x"',
  ]) {
    assert.deepEqual(issuesFor(filter), [], filter);
  }
});

test("reports unknown field roots with root spans and suggestions", () => {
  assert.deepEqual(issuesFor('/userNmae eq "x"'), [
    {
      kind: "field",
      field: "/userNmae",
      root: "userNmae",
      start: 0,
      end: 9,
      suggestion: "userName",
    },
  ]);

  assert.deepEqual(issuesFor('/zzzzzz eq "x"'), [
    {
      kind: "field",
      field: "/zzzzzz",
      root: "zzzzzz",
      start: 0,
      end: 7,
      suggestion: null,
    },
  ]);

  assert.deepEqual(issuesFor('/nope/foo eq "x"'), [
    {
      kind: "field",
      field: "/nope/foo",
      root: "nope",
      start: 0,
      end: 5,
      suggestion: null,
    },
  ]);
});

test("reports unknown operators", () => {
  assert.deepEqual(issuesFor('/userName foo "x"'), [
    {
      kind: "operator",
      operator: "foo",
      start: 10,
      end: 13,
    },
  ]);

  for (const operator of ["ne", "in"]) {
    assert.deepEqual(issuesFor(`/userName ${operator} "x"`), [
      {
        kind: "operator",
        operator,
        start: 10,
        end: 10 + operator.length,
      },
    ]);
  }
});

test("reports syntax issues", () => {
  assert.deepEqual(issuesFor('not (/userName eq "x")'), [
    {
      kind: "syntax",
      start: 0,
      end: 3,
      message: "Unsupported _queryFilter negation '{{operator}}'. Use '!'.",
      data: { operator: "not" },
    },
  ]);

  assert.deepEqual(issuesFor('/userName eq "x'), [
    {
      kind: "syntax",
      start: 13,
      end: 15,
      message: "Unterminated quoted value in _queryFilter.",
    },
  ]);

  assert.deepEqual(issuesFor('(/userName eq "x"'), [
    {
      kind: "syntax",
      start: 0,
      end: 1,
      message: "Expected ')' to close _queryFilter group.",
      data: {},
    },
  ]);

  assert.deepEqual(issuesFor("()"), [
    {
      kind: "syntax",
      start: 0,
      end: 1,
      message: "Empty _queryFilter group.",
      data: {},
    },
  ]);

  assert.deepEqual(issuesFor('/userName eq "x" blah'), [
    {
      kind: "syntax",
      start: 17,
      end: 21,
      message: "Unexpected _queryFilter token '{{token}}'.",
      data: { token: "blah" },
    },
  ]);

  assert.deepEqual(issuesFor("/userName eq"), [
    {
      kind: "syntax",
      start: 10,
      end: 12,
      message: "Expected _queryFilter value after '{{operator}}'.",
      data: { operator: "eq" },
    },
  ]);

  assert.deepEqual(issuesFor('/userName pr "x"'), [
    {
      kind: "syntax",
      start: 13,
      end: 16,
      message: "Unexpected _queryFilter token '{{token}}'.",
      data: { token: "quoted value" },
    },
  ]);
});

test("treats keywords and operators case-insensitively", () => {
  assert.deepEqual(issuesFor('/userName EQ "x"'), []);
  assert.deepEqual(issuesFor('/userName eq "x" OR /accountStatus PR'), []);
  assert.deepEqual(issuesFor('/userName eq "x" AnD /accountStatus Pr'), []);
});

test("carries tokenizer unterminated-string issues through validation", () => {
  assert.deepEqual(issuesFor('/userName eq "x'), [
    {
      kind: "syntax",
      start: 13,
      end: 15,
      message: "Unterminated quoted value in _queryFilter.",
    },
  ]);
});

test("maps cooked offsets to raw offsets across escapes", () => {
  assert.deepEqual(cookedToRawIndexMap("abc"), [0, 1, 2, 3]);
  assert.deepEqual(cookedToRawIndexMap("a\\nb"), [0, 1, 3, 4]);
  assert.deepEqual(cookedToRawIndexMap(["a", "\\", "\n", "b"].join("")), [
    0, 3, 4,
  ]);
  assert.deepEqual(cookedToRawIndexMap("a\\u{1F600}b"), [0, 1, 1, 10, 11]);
});

test("maps field issue offsets after a preceding line continuation", () => {
  const raw = ["\\", "\n", '/userNmae eq "x"'].join("");
  const cooked = raw.replace(/\\(?:\r\n|[\n\r\u2028\u2029])/g, "");
  const [issue] = issuesFor(cooked);
  const stringRange = {
    start: 0,
    end: raw.length,
    indexMap: cookedToRawIndexMap(raw),
  };

  assert.equal(issue.kind, "field");
  assert.deepEqual(
    [
      rawOffsetForIndex(stringRange, issue.start),
      rawOffsetForIndex(stringRange, issue.end),
    ],
    [2, 11]
  );
});
