/**
 * The runtime replaces Function.prototype.bind and Function.prototype.toString
 * so that a JsonValue detail stub cannot be bound or rendered through them
 * (runtime.cjs, __rhinoLocalNativeMethod). For every other receiver they must
 * be the builtins: same metadata (length, name, no own `prototype`, not a
 * constructor), same results, same errors. Each script runs on the bare engine
 * (the oracle) and with the runtime loaded, on both node:vm and the JVM.
 */
import vm from "node:vm";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { parseHarvest } from "../../src/bindings/harvest.ts";
import { mockPreamble, withHarvest } from "../../src/bindings/preamble.ts";
import type { Given } from "../../src/case/types.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { loadBehaviour, runScriptWithFaults, sandboxFaults } from "./load-behaviour.ts";

const meta = (f: string) =>
  `var f = ${f}; [f.length, f.name, Object.prototype.hasOwnProperty.call(f, "prototype"), ` +
  'Object.getOwnPropertyNames(f).sort().join(","), typeof f].join("/")';
// Rhino names a scope object by its identity hash, which differs per object.
const thrown = (expression: string) =>
  `try { ${expression}; "returned"; } catch (e) { (e.name + ": " + e.message).replace(/@[0-9a-f]+/, "@<hash>"); }`;

// Each script's last expression is a string.
const oracle: Array<[string, string]> = [
  ["bind's metadata", meta("Function.prototype.bind")],
  ["toString's metadata", meta("Function.prototype.toString")],
  ["bind rendered", "String(Function.prototype.bind)"],
  ["toString rendered by itself", "Function.prototype.toString.call(Function.prototype.toString)"],
  ["bind is not a constructor", thrown("new Function.prototype.bind()")],
  ["toString is not a constructor", thrown("new Function.prototype.toString()")],
  ["binding a receiver and arguments",
    'function f(a, b) { return this.k + a + b; } var g = f.bind({ k: 1 }, 2); [g(3), g.length, g.name].join("/")'],
  ["a bound constructor",
    'function P(x) { this.x = x; } var B = P.bind(null, 5); var o = new B(); [o.x, o instanceof P].join("/")'],
  ["borrowed bind on a non-function", thrown("Function.prototype.bind.call({})")],
  ["borrowed bind on null", thrown("Function.prototype.bind.call(null)")],
  ["a function rendered", "String(function named(x) { return x; })"],
  ["a function rendered by borrowed toString", "Function.prototype.toString.call(function (x) { return x + 1; })"],
  ["a bound function rendered", "String(function g() {}.bind(null))"],
  ["borrowed toString on a non-function", thrown("Function.prototype.toString.call({})")],
  ["borrowed toString on null", thrown("Function.prototype.toString.call(null)")],
];

const native = (source: string): unknown => vm.runInContext(source, vm.createContext({}));

describe("the bind and toString replacements behave as the builtins", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  it.each(oracle)("%s (node:vm)", (_name, source) => {
    const sandbox = loadBehaviour({});
    expect(vm.runInContext(source, sandbox)).toBe(native(source));
    expect(sandboxFaults(sandbox)).toEqual([]);
  });

  it.each(oracle)("%s (JVM)", async (_name, source) => {
    const bare = await runner.eval({ source });
    expect(bare.outcome, JSON.stringify(bare.error)).toBe("ok");
    const response = await runner.eval({ preamble: mockPreamble({}), source });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    expect(response.emulation?.faults ?? []).toEqual([]);
    expect(response.value).toBe(bare.value);
  });
});

// A detail stub through the borrowed toString (Sol r3): directly, through apply, and as a child's
// member whose rendering is then serialised.
const given: Given = {
  managed: { "managed/widget": [] },
  openidmFailures: [{ match: { method: "delete", resource: "managed/widget/w1", ordinal: 1 }, reply: { code: 404, detail: { s: "v", a: ["alpha"], n: null } } }],
};
const stubRoutes: Array<[string, string]> = [
  ["call", "Function.prototype.toString.call(d.asBoolean)"],
  ["apply on a missing child's member", 'Function.prototype.toString.apply(d.get("missing").asMap, [])'],
  ["call on a string child's member, inside JSON", 'JSON.stringify({ m: Function.prototype.toString.call(d.get("s").keys) })'],
];
const inScript = (read: string) =>
  `try { openidm.delete("managed/widget/w1", null); } catch (e) { var d = e.javaException.getDetail(); try { ${read}; } catch (x) {} }`;

describe("a detail stub rendered by borrowed Function.prototype.toString", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  it.each(stubRoutes)("through %s faults openidm-failure-detail (node:vm)", (_name, read) => {
    const run = runScriptWithFaults(inScript(read), given);
    expect(run.faults.map((f) => f.id)).toEqual(["openidm-failure-detail"]);
  });

  it.each(stubRoutes)("through %s faults openidm-failure-detail, even when caught (JVM)", async (_name, read) => {
    expectHarnessFault("openidm-failure-detail");
    const response = await runner.eval({ preamble: mockPreamble(given), source: withHarvest(inScript(read)) });
    expect(response.emulation?.faults?.map((f) => f.id)).toEqual(["openidm-failure-detail"]);
  });
});

// Measured members: only asString's string form was measured (docs/api/12). Every route that renders a
// member must agree: its own toString, borrowed Function.prototype.toString, String() and concatenation.
const renderings = (member: string): Array<[string, string]> => [
  ["its own toString", `${member}.toString()`],
  ["borrowed Function.prototype.toString", `Function.prototype.toString.call(${member})`],
  ["String()", `String(${member})`],
  ["concatenation", `"" + ${member}`],
];
const ASSTRING = "function asString() {/*\njava.lang.String asString()\n*/}\n";
const rendered = (read: string) =>
  `try { openidm.delete("managed/widget/w1", null); } catch (e) { var d = e.javaException.getDetail(); nodeState.putShared("r", ${read}); }`;

describe("a measured detail member rendered as a string", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  it.each([...renderings("d.get"), ...renderings('d.get("a").isNull')])(
    "get through %s faults openidm-failure-detail (node:vm)", (_name, read) => {
      const run = runScriptWithFaults(inScript(read), given);
      expect(run.faults.map((f) => f.id)).toEqual(["openidm-failure-detail"]);
    });

  it.each([...renderings("d.get"), ...renderings('d.get("a").isNull')])(
    "get through %s faults openidm-failure-detail, even when caught (JVM)", async (_name, read) => {
      expectHarnessFault("openidm-failure-detail");
      const response = await runner.eval({ preamble: mockPreamble(given), source: withHarvest(inScript(read)) });
      expect(response.emulation?.faults?.map((f) => f.id)).toEqual(["openidm-failure-detail"]);
    });

  it.each(renderings("d.asString"))("asString through %s is the measured text (node:vm)", (_name, read) => {
    const run = runScriptWithFaults(rendered(read), given);
    expect(run.faults).toEqual([]);
    expect(run.effects.sharedState.final.r).toBe(ASSTRING);
  });

  it.each(renderings("d.asString"))("asString through %s is the measured text (JVM)", async (_name, read) => {
    const response = await runner.eval({ preamble: mockPreamble(given), source: withHarvest(rendered(read)) });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    expect(response.emulation?.faults ?? []).toEqual([]);
    expect(parseHarvest(response.value as string).sharedState.final.r).toBe(ASSTRING);
  });
});
