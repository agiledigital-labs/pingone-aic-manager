/**
 * openidm.query `result` and a has-many relationship array on a read, on the
 * real JVM, compared with the live captures (openidm-query-result-type and
 * openidm-query-result-string probes, sandbox, 2026-11-08; docs/api/12). The script is the probe fixture itself,
 * verbatim, with only its `emit` replaced so the payload lands in shared state.
 */
import { readFileSync } from "node:fs";
import vm from "node:vm";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { decisionNodeClassAllowList } from "../../src/bindings/allowlist.ts";
import { mockPreamble, withHarvest } from "../../src/bindings/preamble.ts";
import { parseHarvest } from "../../src/bindings/harvest.ts";
import type { Given } from "../../src/case/types.ts";
import { repoRoot } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { loadBehaviour, sandboxFaults } from "./load-behaviour.ts";

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
      { _id: "u1", _rev: "1", userName: "zzemu-a", n: 3, roles: first },
      { _id: "u2", _rev: "1", userName: "zzemu-b", roles: [] },
    ],
  },
} as Given);

// The tenant runs a decision node behind its class allow-list; getClass() and a caught Java
// exception read as the capture has them only behind it.
const SHUTTERED = decisionNodeClassAllowList();

describe("openidm lists match the live capture", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);
  const value = async (source: string, given: Given = users()) => {
    const response = await runner.eval({ preamble: mockPreamble(given), source, classAllowList: SHUTTERED });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    expect(response.emulation?.faults ?? []).toEqual([]);
    return response.value;
  };

  it("the probe reads a query result, an empty query and an empty has-many array as captured", async () => {
    const response = await runner.eval({ preamble: mockPreamble(users()), source: withHarvest(capture), classAllowList: SHUTTERED });
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

  it.each([
    ["one past the end", 2], ["further past the end", 5], ["negative", -1],
    // The boundary controls for the beyond-int refusal below: a Java int is still the measured answer.
    ["at the Java int maximum", 2147483647], ["at the Java int minimum", -2147483648],
  ])("get(i) %s throws the measured InternalError, which a script may catch", async (_name, i) => {
    expect(await value(
      'var r = openidm.query("managed/alpha_user", { _queryFilter: "true" }, ["_id"]).result; ' +
        `try { r.get(${i}); } catch (e) { String(e); }`,
    )).toBe(`InternalError: Index ${i} out of bounds for length 2`);
  });

  it("String(v) is Java's form, and there is no toJSON member (both measured)", async () => {
    expect(await value(
      'var r = openidm.query("managed/alpha_user", { _queryFilter: "true" }, ["_id"]).result; ' +
        'var e = openidm.query("managed/alpha_user", { _queryFilter: "false" }, ["_id"]).result; ' +
        'JSON.stringify([String(r), "" + r, r.toString(), String(e), typeof r.toJSON, typeof e.toJSON])',
    )).toBe('["[{_id=u1, _rev=1}, {_id=u2, _rev=1}]","[{_id=u1, _rev=1}, {_id=u2, _rev=1}]",' +
      '"[{_id=u1, _rev=1}, {_id=u2, _rev=1}]","[]","undefined","undefined"]');
  });

  it("an array method on an empty result is the measured TypeError, worded with Java's []", async () => {
    expect(await value(
      'var e = openidm.query("managed/alpha_user", { _queryFilter: "false" }, ["_id"]).result; ' +
        "var out = []; try { e.some(function () {}); } catch (x) { out.push(x.name + \": \" + x.message); } " +
        "try { e.map(function () {}); } catch (x) { out.push(x.name + \": \" + x.message); } JSON.stringify(out)",
    )).toBe('["TypeError: Cannot find function some in object [].","TypeError: Cannot find function map in object []."]');
  });

  it("String(v) of a has-many list renders nested maps and null as Java does", async () => {
    const withNull = { ...role("r1"), _refProperties: { _id: "r1-edge", _rev: "1", note: null } };
    expect(await value(
      'String(openidm.read("managed/alpha_user/u1", null, ["roles"]).roles)',
      users([withNull]),
    )).toBe("[{_ref=managed/alpha_role/r1, _refResourceCollection=managed/alpha_role, _refResourceId=r1, " +
      "_refProperties={_id=r1-edge, _rev=1, note=null}}]");
  });

  // openidm-query-response-order probe, 2026-11-09: for-in and Object.keys agree on every response;
  // AIC's JSON.stringify order varies between calls, so it is not asserted.
  it.each([
    ["populated, no fields", '{ _queryFilter: "true" }', ""],
    ["empty, with fields", '{ _queryFilter: "false" }', ', ["_id"]'],
  ])("a query response's keys come in the measured for-in order (%s)", async (_name, params, fields) => {
    expect(await value(
      `var r = openidm.query("managed/alpha_user", ${params}${fields}); var f = []; for (var k in r) f.push(k); ` +
        "JSON.stringify([f.join(), Object.keys(r).join()])",
    )).toBe(JSON.stringify(Array(2).fill("pagedResultsCookie,totalPagedResultsPolicy,totalPagedResults,resultCount,result")));
  });

  it("JSON.stringify keeps its replacer and spacing, and leaves the record it was given unchanged", async () => {
    expect(await value(
      'var rec = openidm.read("managed/alpha_user/u1", null, ["roles"]); var before = rec.roles; ' +
        'var s = JSON.stringify({ rec: rec }, function (k, v) { return k === "_refProperties" ? undefined : v; }, 1); ' +
        "JSON.stringify([s, rec.roles === before, Array.isArray(rec.roles)])",
      users([role("r1")]),
    )).toBe(JSON.stringify([
      JSON.stringify({ rec: { _id: "u1", _rev: "1", roles: [{ _ref: "managed/alpha_role/r1", _refResourceCollection: "managed/alpha_role", _refResourceId: "r1" }] } }, null, 1),
      true, false,
    ]));
  });

  it("JSON.stringify runs a getter once, as it does without the wrapper", async () => {
    expect(await value(
      'var n = 0; var o = {}; Object.defineProperty(o, "g", { enumerable: true, get: function () { n += 1; return n; } }); ' +
        "var s = JSON.stringify({ o: o }); JSON.stringify([s, n])",
    )).toBe('["{\\"o\\":{\\"g\\":1}}",1]');
  });

  it.each([
    ["* first", '["*", "roles"]'],
    ["* last", '["roles", "*"]'],
  ])("a relationship named beside a wildcard (%s) is the same list as without it", async (_name, fields) => {
    expect(await value(
      `var v = openidm.read("managed/alpha_user/u1", null, ${fields}).roles; ` +
        "JSON.stringify([Array.isArray(v), v.size(), typeof v.map, [].concat(v).map(function (x) { return x._refResourceId; })])",
      users([role("r1"), role("r2")]),
    )).toBe('[false,2,"undefined",["r1","r2"]]');
  });

  it("concat over many lists stays linear", async () => {
    const records = Array.from({ length: 3000 }, (_, i) => ({ _id: `u${i}`, _rev: "1", userName: `zzemu-${i}`, roles: [role(`r${i}`)] }));
    // Timed inside the script, round the concat loop only, so seeding and JVM load do not count.
    const [n, verdict] = JSON.parse(await value(
      'var r = openidm.query("managed/alpha_user", { _queryFilter: "true" }, ["roles"]).result; var lists = []; ' +
        "for (var i = 0; i < r.size(); i++) { lists.push(r.get(i).roles); } " +
        "var t = Date.now(); var n = 0; for (var j = 0; j < lists.length; j++) { n += [].concat(lists[j]).length; } " +
        // A verdict, not the milliseconds: the two JVM lanes are compared for equality.
        'JSON.stringify([n, Date.now() - t < 1500 ? "linear" : "slow"])',
      { managed: { "managed/alpha_user": records } } as Given,
    ) as string) as [number, string];
    expect(n).toBe(3000);
    // The registry this replaced scanned every live list per concat: about 2 ms each here, 6 s in all.
    // This loop takes about 100 ms now.
    expect(verdict).toBe("linear");
  }, 30_000);

  it("calling an array method is the measured TypeError, not a fault", async () => {
    const v = await value(
      'var r = openidm.query("managed/alpha_user", { _queryFilter: "true" }, ["_id"]).result; ' +
        "try { r.some(function () { return true; }); } catch (e) { e.name + \": \" + e.message; }",
    );
    expect(v).toMatch(/^TypeError: Cannot find function some/);
  });

  it("Array.prototype.concat with a null or undefined receiver still throws TypeError", async () => {
    expect(await value('var out = []; [null, undefined].forEach(function (r) { try { Array.prototype.concat.call(r, 1); out.push("returned"); } ' +
      'catch (e) { out.push(e instanceof TypeError); } }); JSON.stringify(out)')).toBe("[true,true]");
  });

  it("[].concat of plain values is untouched", async () => {
    expect(await value('JSON.stringify([[1].concat(2, [3, [4]], { length: 1, 0: "x" }), Array.prototype.concat.length, Array.prototype.propertyIsEnumerable("concat")])'))
      .toBe('[[1,2,3,[4],{"length":1,"0":"x"}],1,false]');
  });
});

// node:vm runs the runtime too, with no Java in reach; the wrappers must not
// change what the engine does with a receiver they do not own.
describe("the concat and stringify wrappers in node:vm", () => {
  const run = (source: string) => {
    const sandbox = loadBehaviour({ managed: { "managed/alpha_user": [{ _id: "u1", _rev: "1" }] } } as Given);
    return { value: vm.runInContext(source, sandbox) as unknown, faults: sandboxFaults(sandbox) };
  };

  it.each(["null", "undefined"])("Array.prototype.concat.call(%s) throws the engine's TypeError", (receiver) => {
    const { value, faults } = run(`try { Array.prototype.concat.call(${receiver}, 1); "returned"; } catch (e) { e instanceof TypeError; }`);
    expect(value).toBe(true);
    expect(faults).toEqual([]);
  });

  it("[].concat and JSON.stringify of an openidm list", () => {
    const { value, faults } = run('var r = openidm.query("managed/alpha_user", { _queryFilter: "true" }, ["_id"]).result; ' +
      "JSON.stringify([[].concat(r).length, JSON.parse(JSON.stringify(r)), typeof r.toJSON])");
    expect(value).toBe('[1,[{"_id":"u1","_rev":"1"}],"undefined"]');
    expect(faults).toEqual([]);
  });
});
