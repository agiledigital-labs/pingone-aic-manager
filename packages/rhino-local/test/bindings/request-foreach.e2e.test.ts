/**
 * requestParameters / requestHeaders / requestCookies `forEach`, and the value
 * list a request map hands over, on the real JVM, compared with the live
 * capture (request-map-foreach probe, sandbox, 2026-11-08; docs/api/12). The
 * script is the probe fixture itself, verbatim, with only its `emit` replaced
 * so the payload lands in shared state instead of a callback.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { mockPreamble, withHarvest } from "../../src/bindings/preamble.ts";
import type { Given } from "../../src/case/types.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { repoRoot } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { parseHarvest } from "../../src/bindings/harvest.ts";

const probe = readFileSync(join(repoRoot, "scripts/rhino-script-tester/fixtures/request-map-foreach.script.js"), "utf8");
// A later declaration of the same function wins, so this replaces the probe's emit.
const capture = `${probe}\nfunction emit(payload) { nodeState.putShared("r", JSON.stringify(payload)); }`;

// The live capture's per-key record for a two-element and a one-element list.
const list = (items: string[]) => ({
  value: {
    typeof: "object",
    isArray: false,
    length: items.length,
    size: items.length,
    idx0: items[0],
    get0: items[0],
    str: `[${items.join(", ")}]`,
    typeofIdx0: "string",
    jsMap: "undefined",
    jsJoin: "undefined",
    cls: 'err:InternalError: Access to Java class "java.lang.Class" is prohibited.',
  },
  third: "undefined",
  thisType: "object",
  thisIsMap: false,
});
// The capture's error strings end in " (<script>#<line>)", which names the probe's own script.
const located = (text: string) => text.replace(/ \([^()]*#\d+\)$/, "");
const common = {
  typeofForEach: "function",
  ret: { typeof: "undefined", str: "undefined" },
  throwInside: "err:Error: probe-stop",
  throwInsideClass: { name: "Error", message: "probe-stop", isError: true, hasJava: "undefined" },
  noArgs: "err:InternalError: Can't find method java.util.Map.forEach().",
  nonFunction: "err:InternalError: Can't find method java.util.Map.forEach(number).",
};

describe("request map forEach matches the live capture", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);
  const observe = async (given: Given, script = capture) => {
    const response = await runner.eval({ preamble: mockPreamble(given), source: withHarvest(script) });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    return JSON.parse(parseHarvest(response.value as string).sharedState.final.r as string) as Record<string, Record<string, unknown>>;
  };
  const normalise = (side: Record<string, unknown>) => {
    const probes = side.probes as Record<string, { value: Record<string, unknown> }>;
    for (const key of Object.keys(probes)) probes[key]!.value.cls = located(String(probes[key]!.value.cls));
    return { ...side, noArgs: located(String(side.noArgs)), nonFunction: located(String(side.nonFunction)) };
  };

  it("parameters and headers: (key, Java list) pairs, undefined return, the throw propagates", async () => {
    const out = await observe({
      requestParameters: { probeq: ["alpha", "bravo"], probeqsingle: ["solo"], probeqjoined: ["alpha,bravo"], other: ["x"] },
      requestHeaders: { "X-Aic-Probe": ["alpha", "bravo"], "X-Aic-Probe-Single": ["solo"], "X-Aic-Probe-Joined": ["alpha,bravo"], Host: ["h"] },
    });
    expect(out.ok).toBe(true);
    // Key order on AIC is the map's hash order and is not compared; the emulator iterates in seeding order.
    for (const [side, keys] of [["parameters", ["probeq", "probeqsingle", "probeqjoined"]], ["headers", ["x-aic-probe", "x-aic-probe-single", "x-aic-probe-joined"]]] as const) {
      expect(normalise(out[side]!)).toEqual({
        ...common,
        calls: 4, arity: { 2: 4 }, keyTypes: { string: 4 }, order: keys, oneArgFn: 4, forInCount: 4,
        probes: { [keys[0]]: list(["alpha", "bravo"]), [keys[1]]: list(["solo"]), [keys[2]]: list(["alpha,bravo"]) },
      });
    }
  });

  it("cookies: (key, string) pairs", async () => {
    const script = capture.replace(/emit\(\{ ok: true, feature: "request-map-foreach", [^\n]*\n/, 'emit({ ok: true, cookies: probe(requestCookies) });\n');
    expect(script).not.toBe(capture);
    const out = await observe({ requestCookies: { probeqc: "alpha", probeqd: "bravo" } }, script);
    const str = (v: string) => ({
      value: { typeof: "string", isArray: false, length: 5, size: "typeof undefined", idx0: v[0], str: v, typeofIdx0: "string", jsMap: "undefined", jsJoin: "undefined",
        get0: `err:TypeError: Cannot find function get in object ${v}.`, cls: `err:TypeError: Cannot find function getClass in object ${v}.` },
      third: "undefined", thisType: "object", thisIsMap: false,
    });
    const cookies = out.cookies!;
    const probes = cookies.probes as Record<string, { value: Record<string, unknown> }>;
    for (const key of Object.keys(probes)) {
      probes[key]!.value.get0 = located(String(probes[key]!.value.get0));
      probes[key]!.value.cls = located(String(probes[key]!.value.cls));
    }
    expect({ ...cookies, noArgs: located(String(cookies.noArgs)), nonFunction: located(String(cookies.nonFunction)) }).toEqual({
      ...common, calls: 2, arity: { 2: 2 }, keyTypes: { string: 2 }, order: ["probeqc", "probeqd"], oneArgFn: 2, forInCount: 2,
      probes: { probeqc: str("alpha"), probeqd: str("bravo") },
    });
  });

  it.each([
    ["a string", 'requestParameters.forEach("x")'],
    ["an object", "requestParameters.forEach({ accept: function () {} })"],
    ["a function and an extra argument", "requestParameters.forEach(function () {}, 1)"],
    ["a string while join/push are replaced", 'var J = Array.prototype.join, P = Array.prototype.push; Array.prototype.join = function () { throw new Error("j"); }; Array.prototype.push = function () { throw new Error("p"); }; try { requestParameters.forEach("bad"); } finally { Array.prototype.join = J; Array.prototype.push = P; }'],
  ])("%s faults request-map-foreach-argument, even when caught", async (_name, call) => {
    expectHarnessFault("request-map-foreach-argument");
    const response = await runner.eval({ preamble: mockPreamble({ requestParameters: { a: ["1"] } }), source: withHarvest(`try { ${call}; } catch (e) {}`) });
    expect(response.emulation?.faults?.map((f) => f.id)).toEqual(["request-map-foreach-argument"]);
  });

  it.each([
    ["get()", "v.get()"],
    ['get("length")', 'v.get("length")'],
    ['get("0")', 'v.get("0")'],
    ["get(0, 1)", "v.get(0, 1)"],
    ["get(2), out of range", "v.get(2)"],
    ["get(-1)", "v.get(-1)"],
    ["get(0.5)", "v.get(0.5)"],
    ["size(1)", "v.size(1)"],
    ["toArray()", "JSON.stringify(v.toArray())"],
    ["isEmpty()", "v.isEmpty()"],
    ['contains("a")', 'v.contains("a")'],
    ['indexOf("a")', 'v.indexOf("a")'],
    ["iterator()", "v.iterator()"],
    ['set(0, "z")', 'v.set(0, "z")'],
    ["remove(0)", "v.remove(0)"],
    ["listIterator()", "v.listIterator()"],
    ["forEach(fn)", "v.forEach(function () {})"],
    ["sort()", "v.sort()"],
    ["get(an object whose toString throws)", 'v.get({ toString: function () { throw new Error("conversion"); } })'],
    ["get(0, an object whose toString throws)", 'v.get(0, { toString: function () { throw new Error("conversion"); } })'],
    ["get(2) while String is replaced", 'var S = String; String = function () { throw new Error("s"); }; try { v.get(2); } finally { String = S; }'],
    ['get("0") while Array.prototype.map is replaced', 'var M = Array.prototype.map; Array.prototype.map = function () { throw new Error("m"); }; try { v.get("0"); } finally { Array.prototype.map = M; }'],
    ["toString(1)", "v.toString(1)"],
    ["toString(an object whose toString throws)", 'v.toString({ toString: function () { throw new Error("conversion"); } })'],
    ["getClass(1)", "v.getClass(1)"],
  ])("%s on a value list faults request-list-unmeasured, even when caught", async (_name, call) => {
    expectHarnessFault("request-list-unmeasured");
    const response = await runner.eval({
      preamble: mockPreamble({ requestParameters: { q: ["a", "b"] } }),
      source: withHarvest(`var v = requestParameters.get("q"); try { ${call}; } catch (e) {} finally { 1; }`),
    });
    expect(response.emulation?.faults?.map((f) => f.id)).toEqual(["request-list-unmeasured"]);
  });

  it("the measured list calls still answer: size(), get(i) in range, and the README's copy loop", async () => {
    const response = await runner.eval({
      preamble: mockPreamble({ requestParameters: { q: ["a", "b"] } }),
      source: 'var v = requestParameters.get("q"); var out = []; for (var i = 0; i < v.size(); i++) out.push(v.get(i)); ' +
        'JSON.stringify([v.size(), v.get(0), v.get(1), v.length, v[1], out, String(v), "" + v, v.toString()])',
    });
    expect(response.emulation?.faults ?? []).toEqual([]);
    expect(response.value).toBe('[2,"a","b",2,"b",["a","b"],"[a, b]","[a, b]","[a, b]"]');
  });

  it.each([
    ["parameters via get", 'JSON.stringify(requestParameters.get("q"))'],
    ["parameters via brackets", 'JSON.stringify(requestParameters["q"])'],
    ["parameters via the forEach callback", "requestParameters.forEach(function (k, v) { JSON.stringify(v); })"],
    ["headers via get", 'JSON.stringify(requestHeaders.get("x-h"))'],
    ["a list nested in an object", 'JSON.stringify({ q: requestParameters.get("q") })'],
  ])("serialising a value list (%s) faults request-list-serialize, even when caught", async (_name, call) => {
    expectHarnessFault("request-list-serialize");
    const response = await runner.eval({
      preamble: mockPreamble({ requestParameters: { q: ["a", "b"] }, requestHeaders: { "X-H": ["a", "b"] } }),
      source: withHarvest(`try { ${call}; } catch (e) {} finally { 1; }`),
    });
    expect(response.emulation?.faults?.map((f) => f.id)).toEqual(["request-list-serialize"]);
  });

  it("a cookie value still serialises as the string it is", async () => {
    const response = await runner.eval({ preamble: mockPreamble({ requestCookies: { c: "v" } }), source: 'JSON.stringify(requestCookies.get("c"))' });
    expect(response.outcome).toBe("ok");
    expect(response.value).toBe('"v"');
  });

  it("the legacy engine faults request-map-foreach-legacy", async () => {
    expectHarnessFault("request-map-foreach-legacy");
    const response = await runner.eval({ preamble: mockPreamble({ engine: "legacy", requestParameters: { a: ["1"] } }), source: withHarvest("try { requestParameters.forEach(function () {}); } catch (e) {}") });
    expect(response.emulation?.faults?.map((f) => f.id)).toEqual(["request-map-foreach-legacy"]);
  });
});
