/**
 * Generated decision-node workload for phase timing.
 *
 * Shape, not a copy of any suite: many files, several tests each, a few
 * two-step journeys, small required libraries, and a seed of managed records
 * (with has-many arrays), request headers and parameters, and ESVs. The
 * subject walks query results and relationship arrays as Java lists, calls an
 * exported helper, and touches nodeState, callbacks, headers and an ESV.
 *
 * The counts below are the knobs that move the phase mix. `bench/calibrate.ts`
 * prints the resulting shares of JVM time.
 */

import { managed } from "../src/harness/lease.ts";
import type { FixtureSpec } from "../src/harness/types.ts";
import type { JsonObject } from "../src/case/types.ts";

/** Files in the vitest suite. */
export const FILE_COUNT = 100;

/**
 * Files 0–49 have 5 tests, files 50–99 have 6. 550 tests.
 * Test 0 of files 0–76 is a two-step journey: 77 extra jobs, 627 jobs.
 */
export function testsInFile(file: number): number {
  return file < 50 ? 5 : 6;
}

export function isJourney(file: number, test: number): boolean {
  return test === 0 && file < 77;
}

export const USER_COUNT = 8;
export const LINKS_PER_USER = 4;
export const HEADER_COUNT = 40;
export const PARAM_COUNT = 12;
export const ESV_COUNT = 6;
/**
 * How many times each job re-queries and walks the lists. The walk, not the
 * fixture size, is what makes the subject's own work the largest phase:
 * a warm prelude is a fraction of a millisecond, so a large fixture only
 * inflates the harvest.
 */
export const WORK_ROUNDS = 6;

const TYPE = "managed/alpha_thing";

export function userName(index: number): string {
  return `thing-${String(index).padStart(3, "0")}`;
}

export function linkName(index: number): string {
  return `n${index}`;
}

interface RefProps extends JsonObject {
  _id: string;
  _rev: string;
}

interface Link extends JsonObject {
  _id: string;
  _rev: string;
  _ref: string;
  _refResourceCollection: string;
  _refResourceId: string;
  _refProperties: RefProps;
  name: string;
}

interface User extends JsonObject {
  _id: string;
  _rev: string;
  userName: string;
  links: Link[];
}

export function makeUsers(): User[] {
  const users: User[] = [];
  for (let i = 0; i < USER_COUNT; i += 1) {
    const id = `t${String(i).padStart(3, "0")}`;
    const links: Link[] = [];
    for (let j = 0; j < LINKS_PER_USER; j += 1) {
      const name = linkName(j);
      links.push({
        _id: `${id}-l${j}`,
        _rev: "1",
        _ref: `managed/alpha_role/r${j}`,
        _refResourceCollection: "managed/alpha_role",
        _refResourceId: `r${j}`,
        _refProperties: { _id: `${id}-e${j}`, _rev: "1" },
        name,
      });
    }
    users.push({ _id: id, _rev: "1", userName: userName(i), links });
  }
  return users;
}

export function buildFixtures(): Record<string, FixtureSpec> {
  const fixtures: Record<string, FixtureSpec> = {};
  for (const user of makeUsers()) {
    fixtures[user._id] = managed(TYPE, user);
  }
  return fixtures;
}

export function buildHeaders(): Record<string, string[]> {
  const headers: Record<string, string[]> = {};
  for (let i = 0; i < HEADER_COUNT; i += 1) {
    headers[`x-h-${String(i).padStart(2, "0")}`] = [`v${i}`];
  }
  return headers;
}

export function buildParams(): Record<string, string[]> {
  const params: Record<string, string[]> = {};
  for (let i = 0; i < PARAM_COUNT; i += 1) {
    params[`p${String(i).padStart(2, "0")}`] = ["on"];
  }
  return params;
}

export function buildEsv(): Record<string, string> {
  const esv: Record<string, string> = { "feature-flag": "on" };
  for (let i = 1; i < ESV_COUNT; i += 1) {
    esv[`flag-${i}`] = `value-${i}`;
  }
  return esv;
}

/** The seed map a direct `runCase` needs. The harness adds the `esv.` prefix itself. */
export function givenEsv(): Record<string, string> {
  return Object.fromEntries(Object.entries(buildEsv()).map(([name, value]) => [`esv.${name}`, value]));
}

/** Library bodies, keyed by the name `require` is given. */
export function benchLibraries(): Record<string, string> {
  return {
    "bench-labels": [
      "exports.mark = function (n) { return n; };",
    ].join("\n"),
    "bench-helpers": [
      'var labels = require("bench-labels");',
      "exports.code = function (row) {",
      "  var n = 0;",
      "  var name = String(row.userName);",
      "  var i;",
      "  for (i = 0; i < name.length; i += 1) n += name.charCodeAt(i);",
      "  var links = row.links;",
      "  if (links && typeof links.size === 'function') {",
      "    var k;",
      "    var m = links.size();",
      "    for (k = 0; k < m; k += 1) {",
      "      var el = links.get(k);",
      "      if (el && el.name) n += String(el.name).length;",
      "    }",
      "  }",
      "  return labels.mark(n);",
      "};",
    ].join("\n"),
  };
}

/**
 * One decision script. `ask === "yes"` and an empty callback list suspends;
 * a resumed pass records the reply and continues. Every pass does the same
 * query and walk, so a journey's two jobs cost about the same.
 */
export function subjectScript(): string {
  return [
    'var helpers = require("bench-helpers");',
    "var n = 0;",
    "var round;",
    `for (round = 0; round < ${WORK_ROUNDS}; round += 1) {`,
    `  var rows = openidm.query("${TYPE}", { _queryFilter: "true" }, ["_id", "userName", "links/*"]).result;`,
    "  var copied = [].concat(rows);",
    "  var i;",
    "  for (i = 0; i < copied.length; i += 1) n += helpers.code(copied[i]);",
    "  if (copied.length > 0) {",
    `    var owner = openidm.read("${TYPE}/" + copied[0]._id, null, ["links/*"]);`,
    "    if (owner && owner.links) n += owner.links.size();",
    "  }",
    "}",
    "var headerBytes = 0;",
    "requestHeaders.forEach(function (key, value) {",
    "  headerBytes += String(key).length;",
    "  if (value !== null && typeof value.size === 'function') headerBytes += value.size();",
    "});",
    "n += headerBytes;",
    'var mode = requestParameters.get("p00");',
    "if (mode !== null && typeof mode.size === 'function') n += mode.size();",
    'var flag = systemEnv.getProperty("esv.feature-flag");',
    "n += String(flag).length;",
    "if (!callbacks.isEmpty()) {",
    "  var answer = String(callbacks.getNameCallbacks().get(0));",
    '  nodeState.putShared("answer", answer);',
    "  n += answer.length;",
    "}",
    'nodeState.putShared("n", n);',
    'nodeState.putShared("keyCount", nodeState.keys().size());',
    'if (nodeState.get("ask") === "yes" && callbacks.isEmpty()) {',
    '  callbacksBuilder.nameCallback("Confirm");',
    "} else {",
    '  action.goTo("done");',
    "}",
  ].join("\n");
}

function codeOf(user: User): number {
  let n = 0;
  for (let i = 0; i < user.userName.length; i += 1) n += user.userName.charCodeAt(i);
  for (const link of user.links) n += link.name.length;
  return n;
}

/** The `n` a pass stores. A resumed pass adds the reply's length. */
export function expectedN(resumed: boolean): number {
  const users = makeUsers();
  let perRound = 0;
  for (const user of users) perRound += codeOf(user);
  perRound += users[0]?.links.length ?? 0;
  let n = perRound * WORK_ROUNDS;
  for (const key of Object.keys(buildHeaders())) n += key.length + 1;
  n += 1; // requestParameters.get("p00") is a one-element list
  n += buildEsv()["feature-flag"]!.length;
  if (resumed) n += "ok".length;
  return n;
}

/** Shared-state key count at the `keys()` call. */
export function expectedKeyCount(kind: "ordinary" | "journey-final"): number {
  // ordinary: only `n` has been written. journey final: carried `ask`, `n`
  // and `keyCount`, then `n` overwritten, before `keyCount` is written again.
  return kind === "ordinary" ? 1 : 3;
}
