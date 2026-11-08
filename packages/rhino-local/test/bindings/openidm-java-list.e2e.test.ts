/**
 * openidm.query `result` and a has-many relationship array on a read, on the
 * real JVM, compared with the live capture (openidm-query-result-type probe,
 * sandbox, 2026-11-08; docs/api/12). The script is the probe fixture itself,
 * verbatim, with only its `emit` replaced so the payload lands in shared state.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { mockPreamble, withHarvest } from "../../src/bindings/preamble.ts";
import { parseHarvest } from "../../src/bindings/harvest.ts";
import type { Given } from "../../src/case/types.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { repoRoot } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";

const probe = readFileSync(join(repoRoot, "scripts/rhino-script-tester/fixtures/openidm-query-result-type.script.js"), "utf8");
// A later declaration of the same function wins, so this replaces the probe's emit.
const capture = `${probe}\nfunction emit(payload) { nodeState.putShared("r", JSON.stringify(payload)); }`;

const SHUTTER = 'err:InternalError: Access to Java class "java.lang.Class" is prohibited.';
// The capture's per-list record, for a list of `n` records.
const described = (n: number) => ({
  typeof: "object",
  isArray: false,
  length: n,
  index0Type: n > 0 ? "object" : "undefined",
  size: n,
  get0Type: n > 0 ? "object" : "err:InternalError: Index 0 out of bounds for length 0",
  typeofSome: "undefined",
  typeofMap: "undefined",
  typeofForEach: "function",
  typeofFilter: "undefined",
  typeofJoin: "undefined",
  some: "err:TypeError: Cannot find function some",
  concatIsArray: true,
  concatLength: n,
  sliceCall: n,
  forInKeys: n,
  getClass: SHUTTER,
});
// The capture's error strings end in " (<script>#<line>)", which names the probe's own script.
const located = (value: unknown): unknown =>
  typeof value === "string" ? value.replace(/ \([^()]*#\d+\)$/, "")
    : value !== null && typeof value === "object"
      ? Object.fromEntries(Object.entries(value).map(([k, v]) => [k, located(v)])) : value;

const role = (id: string) => ({
  _id: id, _rev: "1", _ref: `managed/alpha_role/${id}`, _refResourceCollection: "managed/alpha_role",
  _refResourceId: id, _refResourceRev: "1", _refProperties: { _id: `${id}-edge`, _rev: "1" }, name: id,
});
const users = (first: unknown[] = []): Given => ({
  managed: {
    "managed/alpha_user": [
      { _id: "u1", _rev: "1", userName: "zzemu-a", roles: first },
      { _id: "u2", _rev: "1", userName: "zzemu-b", roles: [] },
    ],
  },
} as Given);

describe("openidm lists match the live capture", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);
  const value = async (source: string, given: Given = users()) => {
    const response = await runner.eval({ preamble: mockPreamble(given), source });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    expect(response.emulation?.faults ?? []).toEqual([]);
    return response.value;
  };

  it("the probe reads a query result, an empty query and an empty has-many array as captured", async () => {
    const response = await runner.eval({ preamble: mockPreamble(users()), source: withHarvest(capture) });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    expect(response.emulation?.faults ?? []).toEqual([]);
    const payload = JSON.parse(parseHarvest(response.value as string).sharedState.final.r as string) as unknown;
    expect(located(payload)).toEqual({ ok: true, query: described(2), emptyQuery: described(0), readRoles: described(0) });
  });

  it("[].concat copies a list into a JS array, wherever it sits among the arguments", async () => {
    expect(await value(
      'var r = openidm.query("managed/alpha_user", { _queryFilter: "true" }, ["_id"]).result; ' +
        "var c = [].concat(r); var m = [0].concat(r, [9], r); " +
        "JSON.stringify([Array.isArray(c), c.map(function (x) { return x._id; }), c[0] === r[0], " +
        "m.map(function (x) { return typeof x === \"object\" ? x._id : x; })])",
    )).toBe('[true,["u1","u2"],true,[0,"u1","u2",9,"u1","u2"]]');
  });

  it("a populated has-many array is the same list, and serialises as a JSON array", async () => {
    expect(await value(
      'var rec = openidm.read("managed/alpha_user/u1", null, ["roles/*"]); var v = rec.roles; ' +
        'JSON.stringify([Array.isArray(v), v.size(), typeof v.map, [].concat(v).map(function (x) { return x.name; }), JSON.parse(JSON.stringify(rec)).roles.length])',
      users([role("r1"), role("r2")]),
    )).toBe('[false,2,"undefined",["r1","r2"],2]');
  });

  it("JSON.stringify of a query response writes the result as an array", async () => {
    expect(await value('JSON.stringify(openidm.query("managed/alpha_user", { _queryFilter: "true" }, ["_id"]).result)'))
      .toBe('[{"_id":"u1","_rev":"1"},{"_id":"u2","_rev":"1"}]');
  });

  it("get(i) one past the end throws the measured InternalError, which a script may catch", async () => {
    expect(await value(
      'var r = openidm.query("managed/alpha_user", { _queryFilter: "true" }, ["_id"]).result; ' +
        "try { r.get(2); } catch (e) { String(e); }",
    )).toBe("InternalError: Index 2 out of bounds for length 2");
  });

  it("calling an array method is the measured TypeError, not a fault", async () => {
    const v = await value(
      'var r = openidm.query("managed/alpha_user", { _queryFilter: "true" }, ["_id"]).result; ' +
        "try { r.some(function () { return true; }); } catch (e) { e.name + \": \" + e.message; }",
    );
    expect(v).toMatch(/^TypeError: Cannot find function some/);
  });

  it.each([
    ["forEach(fn)", "v.forEach(function () {})"],
    ["get(-1)", "v.get(-1)"],
    ["get(3), past one beyond the end", "v.get(3)"],
    ['get("0")', 'v.get("0")'],
    ["get()", "v.get()"],
    ["isEmpty()", "v.isEmpty()"],
    ["toArray()", "v.toArray()"],
    ["toString(1)", "v.toString(1)"],
    ["concat with the list as receiver", "Array.prototype.concat.call(v, [1])"],
  ])("%s on an openidm list faults openidm-list-unmeasured, even when caught", async (_name, call) => {
    expectHarnessFault("openidm-list-unmeasured");
    const response = await runner.eval({
      preamble: mockPreamble(users()),
      source: withHarvest(`var v = openidm.query("managed/alpha_user", { _queryFilter: "true" }, ["_id"]).result; try { ${call}; } catch (e) {} finally { 1; }`),
    });
    expect(response.emulation?.faults?.map((f) => f.id)).toEqual(["openidm-list-unmeasured"]);
  });

  it("[].concat of a request value list is unmeasured and faults request-list-unmeasured", async () => {
    expectHarnessFault("request-list-unmeasured");
    const response = await runner.eval({
      preamble: mockPreamble({ requestParameters: { q: ["a", "b"] } }),
      source: withHarvest('try { [].concat(requestParameters.get("q")); } catch (e) {} finally { 1; }'),
    });
    expect(response.emulation?.faults?.map((f) => f.id)).toEqual(["request-list-unmeasured"]);
  });

  it("[].concat of plain values is untouched", async () => {
    expect(await value('JSON.stringify([[1].concat(2, [3, [4]], { length: 1, 0: "x" }), Array.prototype.concat.length, Array.prototype.propertyIsEnumerable("concat")])'))
      .toBe('[[1,2,3,[4],{"length":1,"0":"x"}],1,false]');
  });
});
