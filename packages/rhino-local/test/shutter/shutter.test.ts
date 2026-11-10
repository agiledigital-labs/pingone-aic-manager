import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
import { defineCase } from "../../src/case/index.ts";
import { RhinoRunner } from "../../src/runner.ts";

/**
 * Each row is a construction `docs/api/12-script-bindings-matrix.md` measured
 * on a live next-gen decision node (class shutter section, verified
 * 2026-09-08). `allowed: false` rows are the ones that passed locally before
 * the shutter existed — every one of them was a false pass.
 */
const MATRIX: Array<{ what: string; js: string; allowed: boolean }> = [
  { what: "new java.util.ArrayList", js: `new java.util.ArrayList()`, allowed: true },
  { what: "new java.util.HashSet", js: `new java.util.HashSet()`, allowed: true },
  { what: "new java.util.LinkedHashSet", js: `new java.util.LinkedHashSet()`, allowed: true },
  { what: "new java.util.TreeSet", js: `new java.util.TreeSet()`, allowed: true },
  { what: "java.util.Collections.emptyMap", js: `java.util.Collections.emptyMap()`, allowed: true },
  { what: "new java.util.HashMap", js: `new java.util.HashMap()`, allowed: false },
  { what: "new java.util.LinkedHashMap", js: `new java.util.LinkedHashMap()`, allowed: false },
  { what: "new java.util.TreeMap", js: `new java.util.TreeMap()`, allowed: false },
  // The iterator rows: the rule is the ITERATOR's class, not iteration.
  { what: "HashSet.iterator()", js: `new java.util.HashSet().iterator()`, allowed: true },
  { what: "ArrayList.iterator()", js: `new java.util.ArrayList().iterator()`, allowed: false },
  { what: "LinkedHashSet.iterator()", js: `new java.util.LinkedHashSet().iterator()`, allowed: false },
  { what: "TreeSet.iterator()", js: `new java.util.TreeSet().iterator()`, allowed: false },
  { what: "getClass()", js: `new java.util.ArrayList().getClass()`, allowed: false },
];

const probe = (expr: string) => `
var out;
try {
  var v = EXPR;
  out = { ok: true };
} catch (e) {
  out = { ok: false, error: String(e) };
}
nodeState.putShared("probe", JSON.stringify(out));
action.goTo("true");
`.replace("EXPR", expr);

describe("AM Java class shutter", () => {
  let runner: RhinoRunner;

  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);

  afterAll(async () => {
    await runner.close();
  }, 30_000);

  async function attempt(js: string, shutter: boolean) {
    const run = await runCase(
      runner,
      defineCase({ name: "shutter-probe", script: probe(js), expect: { outcome: "true" } }),
      { timeoutMs: 5_000, classShutter: shutter }
    );
    return JSON.parse(String(run.effects.sharedState.final.probe)) as {
      ok: boolean;
      error?: string;
    };
  }

  for (const row of MATRIX) {
    it(`${row.what} -> ${row.allowed ? "allowed" : "prohibited"}, as measured on AIC`, async () => {
      const result = await attempt(row.js, true);
      expect(result.ok, `${row.what}: ${result.error ?? ""}`).toBe(row.allowed);
    });
  }

  it("every prohibited row USED to pass — these were the false passes", async () => {
    // The control that gives the suite above its meaning. Without it, a shutter
    // that hid everything would also turn the matrix green.
    const denied = MATRIX.filter((row) => !row.allowed);
    for (const row of denied) {
      const before = await attempt(row.js, false);
      expect(before.ok, `${row.what} should succeed with no shutter`).toBe(true);
    }
    expect(denied.length).toBeGreaterThan(0);
  }, 30_000);

  it("reports AM's two distinct failure shapes, not one", async () => {
    // A name the script writes itself never resolves past the package object.
    const named = await attempt(`new java.util.HashMap()`, true);
    expect(named.ok).toBe(false);
    expect(named.error).toContain("JavaPackage java.util.HashMap");
    expect(named.error).toContain("TypeError");

    // An instance handed over by a call is a prohibited-access error instead.
    const handed = await attempt(`new java.util.ArrayList().iterator()`, true);
    expect(handed.ok).toBe(false);
    expect(handed.error).toContain("prohibited");
    expect(handed.error).toContain("java.util.ArrayList$Itr");
  });

  it("leaves the portable answers working — index loop and toArray", async () => {
    const indexed = await attempt(
      `(function () { var l = new java.util.ArrayList(); l.add("a"); return l.get(0) + l.size(); })()`,
      true
    );
    expect(indexed.ok).toBe(true);
    const toArray = await attempt(
      `(function () { var s = new java.util.HashSet(); s.add("a"); return s.toArray()[0]; })()`,
      true
    );
    expect(toArray.ok).toBe(true);
  });

  // Measured 2026-11-10 on AIC, legacy and next-gen (docs/api/12): a script
  // naming the engine package gets a [JavaPackage ...] placeholder, so `new`
  // is a catchable TypeError. Without this a script could build a Context,
  // decompile emulator-installed functions or mint an unguarded scope.
  const ENGINE_REFUSED = "TypeError: [JavaPackage org.mozilla.javascript.Context] is not a function, it is object.";
  const engineRoutes: Array<[string, string]> = [
    ["Packages.org... construct", `new Packages.org.mozilla.javascript.Context()`],
    ["org... construct", `new org.mozilla.javascript.Context()`],
    [
      "decompileFunction route",
      `new Packages.org.mozilla.javascript.Context().decompileFunction(function () {}, 0)`,
    ],
    [
      "fresh standard scope route",
      `new Packages.org.mozilla.javascript.Context().initStandardObjects().Function.prototype.toSource()`,
    ],
  ];
  for (const [what, js] of engineRoutes) {
    it(`author access to the engine package is refused as AM refuses it: ${what}`, async () => {
      const result = await attempt(js, true);
      expect(result.ok).toBe(false);
      expect(result.error).toBe(ENGINE_REFUSED);
    });
  }

  it("JavaImporter(Packages.org.mozilla.javascript) yields no Context, as measured", async () => {
    const result = await attempt(
      `(function () { var i = JavaImporter(Packages.org.mozilla.javascript); if (typeof i.Context !== "undefined") throw new Error(typeof i.Context); })()`,
      true
    );
    expect(result.ok, result.error).toBe(true);
  });

  it("typeof the engine package and class is still 'object', as measured", async () => {
    const result = await attempt(
      `(function () { if (typeof Packages.org.mozilla.javascript.Context !== "object") throw new Error("typeof"); })()`,
      true
    );
    expect(result.ok).toBe(true);
  });

  it("internal engine use still works under the shutter: template literals", async () => {
    const result = await attempt(
      "(function () { var who = 'w'; var s = `hi ${who} ${1 + 1}`; if (s !== 'hi w 2') throw new Error(s); })()",
      true
    );
    expect(result.ok, result.error).toBe(true);
  });
});
