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
  ["an array replacer unboxes Number, String and Boolean objects",
    'JSON.stringify({ n: new Number(4), s: new String("ab"), b: new Boolean(false) }, ["n", "s", "b"])'],
  ["a toJSON returning a Number object under an empty property list is a number",
    'JSON.stringify({ toJSON: function () { return new Number(4); } }, [])'],
  ["an array replacer keeps its order for integer keys",
    'JSON.stringify({ 1: "a", 2: "b" }, ["2", "1"])'],
  ["an array replacer reads integer-keyed getters in its order",
    'var n = 0, o = {}; Object.defineProperty(o, "1", { enumerable: true, get: function () { return ++n; } }); ' +
      'Object.defineProperty(o, "2", { enumerable: true, get: function () { return ++n; } }); JSON.stringify(o, ["2", "1"])'],
  ["an array replacer that grows while it is read keeps its first length",
    'var r = ["a"]; Object.defineProperty(r, "0", { get: function () { r.push("b"); return "a"; } }); JSON.stringify({ a: 1, b: 2 }, r)'],
  ["a RegExp under an array replacer",
    'JSON.stringify({ r: /x/g, k: 1 }, ["r", "k", "lastIndex"])'],
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

// Java values exist on the JVM only.
const javaListFree: Array<[string, string]> = [
  ["an array replacer writes a Java String and Integer as their values",
    'JSON.stringify({ s: new java.lang.String("ab"), n: new java.lang.Integer(4) }, ["s", "n"])'],
  ["an array replacer selects from a Java map",
    'var m = new java.util.HashMap(); m.put("a", 1); m.put("b", 2); JSON.stringify({ m: m }, ["m", "a"])'],
  ["an array replacer leaves a Java list whole",
    'var l = new java.util.ArrayList(); l.add(1); l.add(2); JSON.stringify({ l: l }, ["l"])'],
  ["an array replacer writes a Java byte array as native does",
    'JSON.stringify({ b: new java.lang.String("ab").getBytes() }, ["b"])'],
  ["an array replacer over a nested Java String array behaves as native",
    'try { JSON.stringify({ a: new java.lang.String("x,y").split(",") }, ["a"]); } catch (e) { "threw:" + e.name; }'],
  ["an array replacer over a Java map with an Integer key",
    'var m = new java.util.HashMap(); m.put(new java.lang.Integer(1), "a"); JSON.stringify({ m: m }, ["m", "1"])'],
];

// V8 values with no Rhino counterpart.
const v8ListFree: Array<[string, string]> = [
  ["an object tagged Number by Symbol.toStringTag is an object, and its tag is not read",
    'var n = 0, o = { x: 1 }; Object.defineProperty(o, Symbol.toStringTag, { get: function () { n++; return "Number"; } }); JSON.stringify(o, ["x"]) + ":" + n'],
  ["a boxed BigInt under an empty property list",
    'try { JSON.stringify({ b: Object(BigInt(1)) }, []); } catch (e) { "threw:" + e.name; }'],
];

const native = (source: string): unknown => vm.runInContext(source, vm.createContext({}));

describe("JSON.stringify without lists matches native", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  it.each([...listFree, ...v8ListFree])("%s (node:vm)", (_name, source) => {
    const sandbox = loadBehaviour({});
    expect(vm.runInContext(source, sandbox)).toBe(native(source));
    expect(sandboxFaults(sandbox)).toEqual([]);
  });

  // The JVM oracle is Rhino without the runtime: the engines differ (a RegExp).
  it.each([...listFree, ...javaListFree])("%s (JVM)", async (_name, source) => {
    const bare = await runner.eval({ source });
    expect(bare.outcome, JSON.stringify(bare.error)).toBe("ok");
    const response = await runner.eval({ preamble: mockPreamble({}), source });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    expect(response.emulation?.faults ?? []).toEqual([]);
    expect(response.value).toBe(bare.value);
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
// The same routes under a property list, which native applies to the records too.
const listedRoutes = (list: string): Array<[string, string]> => [
  ["directly", `JSON.stringify(${list}, ["_id"])`],
  ["nested", `JSON.stringify({ v: ${list} }, ["v", "_id"])`],
  ["from a getter", `var o = {}; Object.defineProperty(o, "v", { enumerable: true, get: function () { return ${list}; } }); JSON.stringify(o, ["v", "_id"])`],
];
// A getter in a property list that runs a function-replacer call of its own.
const inListGetter = (inner: string) =>
  'var r = ["x"], inner; Object.defineProperty(r, "0", { get: function () { ' +
  `try { inner = ${inner}; } catch (e) { inner = "threw"; } return "x"; } }); JSON.stringify({ x: 1 }, r) + ":" + inner`;
const nestedInListed: Array<[string, string, string]> = [
  ["a nested function replacer removes a request list before it is judged",
    'var l = requestParameters.get("x"); ' + inListGetter('JSON.stringify({ v: l }, function (k, v) { return k === "v" ? undefined : v; })'),
    '{"x":1}:{}'],
  ["a nested function replacer sees the openidm list itself",
    `var l = ${QUERY}; ` + inListGetter('String(JSON.stringify({ v: l }, function (k, v) { return k === "v" ? String(v === l) : v; }))'),
    '{"x":1}:{"v":"true"}'],
  ["a nested call does not end the outer call's conversion",
    `var l = ${QUERY}; var o = { x: 1 }; Object.defineProperty(o, "v", { enumerable: true, get: function () { JSON.stringify({ a: 1 }); return l; } }); ` +
      'JSON.stringify(o, ["v", "_id"])',
    '{"v":[{"_id":"u1"}]}'],
];
const frozen = (list: string): Array<[string, string, string]> => [
  ["frozen before the call", `var l = ${list}; Object.freeze(l); JSON.stringify(l, ["_id"])`, '[{"_id":"u1"}]'],
  ["frozen during the call, then serialised again",
    `var l = ${list}; var r = ["x"]; Object.defineProperty(r, "0", { get: function () { Object.freeze(l); return "x"; } }); ` +
      'JSON.stringify({ x: 1 }, r) + ":" + typeof l.toJSON + ":" + JSON.stringify(l, ["_id"])',
    '{"x":1}:undefined:[{"_id":"u1"}]'],
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

  it.each(listedRoutes(QUERY))("an openidm list reached %s under a property list is a JSON array of selected records", async (route, source) => {
    expect(await value(source)).toBe(arrayOf(route, '[{"_id":"u1"}]'));
  });

  it.each(listedRoutes('requestParameters.get("x")'))("a request list reached %s under a property list faults request-list-serialize", async (_route, source) => {
    expectHarnessFault("request-list-serialize");
    const response = await runner.eval({ preamble: mockPreamble(given), source: withHarvest(`try { ${source}; } catch (e) {} finally { 1; }`) });
    expect(response.emulation?.faults?.map((f) => f.id)).toEqual(["request-list-serialize"]);
  });

  // Disclosed gap (docs/rhino-local-gaps.md, request-list-serialize): native asks
  // toJSON once per value, so a list an author's toJSON returns is written as an
  // object of the listed keys. Pinned so a change of behaviour is seen.
  it("a list an author's toJSON returns under a property list is not converted", async () => {
    expect(await value(`JSON.stringify({ toJSON: function () { return ${QUERY}; } }, ["_id"])`)).toBe("{}");
  });

  it("a list has no toJSON outside a property-list call, and an author's own toJSON on a list wins", async () => {
    expect(await value(`var r = ${QUERY}; var before = typeof r.toJSON; JSON.stringify(r, ["_id"]); before + ":" + typeof r.toJSON`)).toBe("undefined:undefined");
    expect(await value(`var r = ${QUERY}; r.toJSON = function () { return "mine"; }; JSON.stringify({ v: r }, ["v"])`)).toBe('{"v":"mine"}');
  });

  // Astra r7 P1-1: a function-replacer call nested in a property-list call.
  it.each(nestedInListed)("%s", async (_name, source, want) => {
    expect(await value(source)).toBe(want);
  });

  // Astra r7 P1-2: freezing a list, before the call or during it.
  it.each(frozen(QUERY))("a frozen openidm list: %s", async (_name, source, want) => {
    expect(await value(source)).toBe(want);
  });

  it("a frozen request list under a property list faults request-list-serialize", async () => {
    expectHarnessFault("request-list-serialize");
    const response = await runner.eval({ preamble: mockPreamble(given),
      source: withHarvest(`var l = requestParameters.get("x"); Object.freeze(l); try { JSON.stringify(l, ["0"]); } catch (e) {} finally { 1; }`) });
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
  it.each(routes(QUERY))("an openidm list reached %s is a JSON array", (route, source) => {
    const sandbox = loadBehaviour(given);
    expect(vm.runInContext(source, sandbox)).toBe(arrayOf(route, '[{"_id":"u1","_rev":"1"}]'));
    expect(sandboxFaults(sandbox)).toEqual([]);
  });

  it.each(routes(EMPTY))("an empty openidm list reached %s is []", (route, source) => {
    const sandbox = loadBehaviour(given);
    expect(vm.runInContext(source, sandbox)).toBe(arrayOf(route, "[]"));
    expect(sandboxFaults(sandbox)).toEqual([]);
  });

  it.each(routes('requestParameters.get("x")'))("a request list reached %s faults request-list-serialize, even when caught", (_route, source) => {
    const sandbox = loadBehaviour(given);
    vm.runInContext(`try { ${source}; } catch (e) {}`, sandbox);
    expect(sandboxFaults(sandbox).map((f) => f.id)).toEqual(["request-list-serialize"]);
  });

  it.each(listedRoutes(QUERY))("an openidm list reached %s under a property list is a JSON array of selected records", (route, source) => {
    const sandbox = loadBehaviour(given);
    expect(vm.runInContext(source, sandbox)).toBe(arrayOf(route, '[{"_id":"u1"}]'));
    expect(sandboxFaults(sandbox)).toEqual([]);
  });

  it.each(listedRoutes('requestParameters.get("x")'))("a request list reached %s under a property list faults request-list-serialize", (_route, source) => {
    const sandbox = loadBehaviour(given);
    vm.runInContext(`try { ${source}; } catch (e) {}`, sandbox);
    expect(sandboxFaults(sandbox).map((f) => f.id)).toEqual(["request-list-serialize"]);
  });

  it.each(nestedInListed)("%s", (_name, source, want) => {
    const sandbox = loadBehaviour(given);
    expect(vm.runInContext(source, sandbox)).toBe(want);
    expect(sandboxFaults(sandbox)).toEqual([]);
  });

  it.each(frozen(QUERY))("a frozen openidm list: %s", (_name, source, want) => {
    const sandbox = loadBehaviour(given);
    expect(vm.runInContext(source, sandbox)).toBe(want);
    expect(sandboxFaults(sandbox)).toEqual([]);
  });

  it("a frozen request list under a property list faults request-list-serialize", () => {
    const sandbox = loadBehaviour(given);
    vm.runInContext('var l = requestParameters.get("x"); Object.freeze(l); try { JSON.stringify(l, ["0"]); } catch (e) {}', sandbox);
    expect(sandboxFaults(sandbox).map((f) => f.id)).toEqual(["request-list-serialize"]);
  });

  it("an own __proto__ beside a list is kept, and the list is converted", () => {
    const sandbox = loadBehaviour(given);
    expect(vm.runInContext(
      `var o = {}; Object.defineProperty(o, "__proto__", { value: "kept", enumerable: true }); o.v = ${QUERY}; JSON.stringify(o)`, sandbox,
    )).toBe('{"__proto__":"kept","v":[{"_id":"u1","_rev":"1"}]}');
    expect(sandboxFaults(sandbox)).toEqual([]);
  });
});
