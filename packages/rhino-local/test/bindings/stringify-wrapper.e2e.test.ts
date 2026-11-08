/**
 * The JSON.stringify wrapper (runtime.cjs) converts openidm lists and refuses
 * request lists wherever native serialisation reaches them, and otherwise
 * leaves serialisation exactly as native does it. List-free scripts run three
 * ways: a bare node:vm context with no runtime (the oracle), the runtime in
 * node:vm, and the runtime on the JVM; all three must agree.
 */
import vm from "node:vm";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { mockPreamble, withHarvest } from "../../src/bindings/preamble.ts";
import type { Given } from "../../src/case/types.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { loadBehaviour, sandboxFaults } from "./load-behaviour.ts";

// Each script's last expression is a string.
const listFree: Array<[string, string]> = [
  ["a toJSON getter runs once",
    'var n = 0, o = {}; Object.defineProperty(o, "toJSON", { get: function () { n++; return undefined; } }); JSON.stringify(o) + ":" + n'],
  ["a property an array replacer excludes is never read",
    'var o = { visible: 1, hidden: {} }; Object.defineProperty(o.hidden, "toJSON", { get: function () { throw new Error("excluded"); } }); ' +
      'try { JSON.stringify(o, ["visible"]); } catch (e) { "threw:" + e.message; }'],
  ["a getter named like the list marker is not run",
    'var n = 0, o = { a: 1 }; Object.defineProperty(o, "__rhinoLocalList", { get: function () { n++; return 1; } }); JSON.stringify(o) + ":" + n'],
  ["an enumerable getter runs once",
    'var n = 0, o = {}; Object.defineProperty(o, "g", { enumerable: true, get: function () { n++; return n; } }); JSON.stringify({ o: o }) + ":" + n'],
  ["an own __proto__ data property is kept",
    'var o = {}; Object.defineProperty(o, "__proto__", { value: "kept", enumerable: true }); o.v = [1]; JSON.stringify(o)'],
  ["an array replacer selects an inherited property, in list order",
    'var o = Object.create({ inherited: "kept" }); o.v = 1; o.w = 2; JSON.stringify(o, ["v", "inherited", "absent"])'],
  ["an array replacer normalises numbers and String objects, and drops duplicates",
    'JSON.stringify({ 1: "a", b: "c", d: "e" }, [1, "b", "b", new String("1"), {}, null])'],
  ["an array replacer leaves arrays whole",
    'JSON.stringify({ a: [1, { x: 1, a: 2 }], x: 3 }, ["a"])'],
  ["an array replacer interleaves sibling reads with their subtrees",
    'var log = []; var o = {}; Object.defineProperty(o, "a", { enumerable: true, get: function () { log.push("a"); ' +
      'return { toJSON: function () { log.push("a.toJSON"); return 1; } }; } }); ' +
      'Object.defineProperty(o, "b", { enumerable: true, get: function () { log.push("b"); return 2; } }); JSON.stringify(o, ["a", "b"]) + " " + log.join()'],
  ["a cycle under an array replacer is still a TypeError",
    'var o = {}; o.self = o; try { JSON.stringify(o, ["self"]); "returned"; } catch (e) { String(e instanceof TypeError); }'],
  ["a repeated, non-cyclic object under an array replacer serialises twice",
    'var c = { k: 1, z: 2 }; JSON.stringify({ a: c, b: c }, ["a", "b", "k"])'],
  ["a function replacer sees the real holder and its own key order",
    'var o = { v: 1, w: { x: 2 } }; var seen = []; JSON.stringify(o, function (k, v) { seen.push(k + ":" + (this === o)); return v; }); seen.join()'],
  ["a function replacer's undefined drops the key",
    'JSON.stringify({ a: 1, b: 2 }, function (k, v) { return k === "a" ? undefined : v; })'],
  ["space as a Number object and as a long string",
    'JSON.stringify({ a: [1] }, null, new Number(2)) + "|" + JSON.stringify({ a: 1 }, null, "abcdefghijkl")'],
  ["a Date and a boxed string",
    'JSON.stringify({ d: new Date(0), s: new String("x") })'],
  ["undefined, a function and no replacer at the root",
    'String(JSON.stringify(undefined)) + "|" + String(JSON.stringify(function () {})) + "|" + JSON.stringify("x")'],
];

const native = (source: string): unknown => vm.runInContext(source, vm.createContext({}));

describe("JSON.stringify without lists matches native", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  it.each(listFree)("%s (node:vm)", (_name, source) => {
    const sandbox = loadBehaviour({});
    expect(vm.runInContext(source, sandbox)).toBe(native(source));
    expect(sandboxFaults(sandbox)).toEqual([]);
  });

  it.each(listFree)("%s (JVM)", async (_name, source) => {
    const response = await runner.eval({ preamble: mockPreamble({}), source });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    expect(response.emulation?.faults ?? []).toEqual([]);
    expect(response.value).toBe(native(source));
  });
});

const given = {
  requestParameters: { x: ["a"] },
  managed: { "managed/alpha_user": [{ _id: "u1", _rev: "1" }] },
} as Given;
const QUERY = 'openidm.query("managed/alpha_user", { _queryFilter: "true" }, ["_id"]).result';
const EMPTY = 'openidm.query("managed/alpha_user", { _queryFilter: "false" }, ["_id"]).result';
// Every route by which native serialisation can reach a value: directly, nested, from toJSON, from a getter, from a replacer.
const routes = (list: string): Array<[string, string]> => [
  ["directly", `JSON.stringify(${list})`],
  ["nested", `JSON.stringify({ v: ${list} })`],
  ["from toJSON", `JSON.stringify({ toJSON: function () { return ${list}; } })`],
  ["from a getter", `var o = {}; Object.defineProperty(o, "v", { enumerable: true, get: function () { return ${list}; } }); JSON.stringify(o)`],
  ["from a replacer", `JSON.stringify({ v: 1 }, function (k, v) { return k === "v" ? ${list} : v; })`],
];
const arrayOf = (route: string, items: string) =>
  route === "nested" || route === "from a getter" || route === "from a replacer" ? `{"v":${items}}` : items;

describe("JSON.stringify of the emulated lists", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);
  const value = async (source: string) => {
    const response = await runner.eval({ preamble: mockPreamble(given), source });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    expect(response.emulation?.faults ?? []).toEqual([]);
    return response.value;
  };

  it.each(routes(QUERY))("an openidm list reached %s is a JSON array", async (route, source) => {
    expect(await value(source)).toBe(arrayOf(route, '[{"_id":"u1","_rev":"1"}]'));
  });

  it.each(routes(EMPTY))("an empty openidm list reached %s is []", async (route, source) => {
    expect(await value(source)).toBe(arrayOf(route, "[]"));
  });

  it.each(routes('requestParameters.get("x")'))("a request list reached %s faults request-list-serialize, even when caught", async (_route, source) => {
    expectHarnessFault("request-list-serialize");
    const response = await runner.eval({ preamble: mockPreamble(given), source: withHarvest(`try { ${source}; } catch (e) {} finally { 1; }`) });
    expect(response.emulation?.faults?.map((f) => f.id)).toEqual(["request-list-serialize"]);
  });

  it("a request list a replacer removes is never judged", async () => {
    expect(await value('JSON.stringify({ x: requestParameters.get("x"), y: 1 }, function (k, v) { return k === "x" ? undefined : v; })'))
      .toBe('{"y":1}');
    expect(await value('JSON.stringify({ x: requestParameters.get("x"), y: 1 }, ["y"])')).toBe('{"y":1}');
  });

  it("an own __proto__ beside a list is kept, and the list is converted", async () => {
    expect(await value(`var o = {}; Object.defineProperty(o, "__proto__", { value: "kept", enumerable: true }); o.v = ${QUERY}; JSON.stringify(o)`))
      .toBe('{"__proto__":"kept","v":[{"_id":"u1","_rev":"1"}]}');
  });

  it("an array replacer selects an inherited sibling of a list, and filters the list's records", async () => {
    expect(await value(`var o = Object.create({ inherited: "kept" }); o.v = ${QUERY}; JSON.stringify(o, ["inherited", "v", "_id"])`))
      .toBe('{"inherited":"kept","v":[{"_id":"u1"}]}');
  });

  it("a function replacer sees the author's holder beside a list, and the list itself", async () => {
    expect(await value(
      `var o = { v: ${QUERY}, w: 1 }; var seen = []; JSON.stringify(o, function (k, v) { ` +
        'if (k === "v") seen.push(Array.isArray(v), typeof v.size); if (k === "w") seen.push(this === o); return v; }); JSON.stringify(seen)',
    )).toBe('[false,"function",true]');
  });
});

describe("JSON.stringify of an openidm list in node:vm", () => {
  it("an own __proto__ beside a list is kept, and the list is converted", () => {
    const sandbox = loadBehaviour(given);
    expect(vm.runInContext(
      `var o = {}; Object.defineProperty(o, "__proto__", { value: "kept", enumerable: true }); o.v = ${QUERY}; JSON.stringify(o)`, sandbox,
    )).toBe('{"__proto__":"kept","v":[{"_id":"u1","_rev":"1"}]}');
    expect(sandboxFaults(sandbox)).toEqual([]);
  });
});
