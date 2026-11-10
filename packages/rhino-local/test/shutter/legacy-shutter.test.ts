import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { CaseRunError, runCase } from "../../src/bindings/index.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { defineCase } from "../../src/case/index.ts";
import { RhinoRunner } from "../../src/runner.ts";

/**
 * The legacy evaluator through `runCase`'s real engine selection. Measured
 * 2026-11-10 on a live legacy decision node (docs/api/12): the engine package
 * and `java.lang.Class` resolve to [JavaPackage ...] placeholders, while legacy
 * keeps resolving classes the next-gen allow-list hides (`java.util.HashMap`).
 */
const CONTEXT = "TypeError: [JavaPackage org.mozilla.javascript.Context] is not a function, it is object.";

describe("legacy engine: measured Java access", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner.close(); }, 30_000);

  async function legacy(expr: string): Promise<string> {
    const script = `
var out;
try { out = String(EXPR); } catch (e) { out = "THROW " + String(e); }
nodeState.putShared("r", out);
outcome = "done";`.replace("EXPR", expr);
    const run = await runCase(
      runner,
      defineCase({ name: "legacy-shutter", script, given: { engine: "legacy" }, expect: { outcome: "done" } }),
      { timeoutMs: 5_000 }
    );
    return String(run.effects.sharedState.final.r);
  }

  const rows: Array<[string, string, string]> = [
    ["typeof Packages...Context", `typeof Packages.org.mozilla.javascript.Context`, "object"],
    ["typeof org...Context", `typeof org.mozilla.javascript.Context`, "object"],
    ["typeof Packages...ScriptableObject", `typeof Packages.org.mozilla.javascript.ScriptableObject`, "object"],
    ["new Packages...Context()", `new Packages.org.mozilla.javascript.Context()`, `THROW ${CONTEXT}`],
    ["new org...Context()", `new org.mozilla.javascript.Context()`, `THROW ${CONTEXT}`],
    [
      "Class.forName",
      `java.lang.Class.forName("org.mozilla.javascript.Context")`,
      'THROW TypeError: Cannot call property forName in object [JavaPackage java.lang.Class]. It is not a function, it is "object".',
    ],
    ["decompile continuation", `new Packages.org.mozilla.javascript.Context().decompileFunction(function author() { return 1; }, 0)`, `THROW ${CONTEXT}`],
    ["JavaImporter(...).Context", `typeof JavaImporter(Packages.org.mozilla.javascript).Context`, "undefined"],
  ];
  for (const [what, expr, want] of rows) {
    it(`${what}, as measured on legacy`, async () => {
      expect(await legacy(expr)).toBe(want);
    });
  }

  it("control: a class the legacy engine permits (and next-gen hides) still works", async () => {
    expect(await legacy(`new java.util.HashMap().size()`)).toBe("0");
  });

  it("control: template literals still work", async () => {
    expect(await legacy("`a${1 + 1}`")).toBe("a2");
  });
});

// An engine Context handed over by a Java API, not named by the script: the
// route is unmeasured on AIC, so it is a named, tallied fault on both engines.
describe("an engine capability instance reaching a script indirectly", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner.close(); }, 30_000);

  const viaXml = (use: string) => `
var xml = '<java><object class="org.mozilla.javascript.Context"/></java>';
var decoder = new java.beans.XMLDecoder(new java.io.ByteArrayInputStream(new java.lang.String(xml).getBytes("UTF-8")));
var r;
try { var c = decoder.readObject(); r = String(${use}); } catch (e) { r = "THROW " + String(e); } finally { decoder.close(); }
nodeState.putShared("r", r);
outcome = "done";`;

  async function run(script: string, engine: "legacy" | "am") {
    const given = engine === "legacy" ? { engine: "legacy" as const } : {};
    return runCase(runner, defineCase({ name: "engine-guard", script, given, expect: { outcome: "done" } }), { timeoutMs: 5_000 });
  }

  it.each([
    ["decompileFunction", `c.decompileFunction(function ordinary(a) { return a; }, 0)`],
    ["initStandardObjects", `c.initStandardObjects().Function.prototype.toSource.call(function ordinary(a) { return a; })`],
  ])("legacy: an XMLDecoder-built Context is a tallied fault (%s)", async (_name, use) => {
    expectHarnessFault("engine-api-unmeasured/Context");
    const error = await run(viaXml(use), "legacy").then(() => undefined, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    const response = (error as CaseRunError).response;
    expect(response.emulation?.faults?.map((f: { id: string }) => f.id)).toContain("engine-api-unmeasured/Context");
  }, 30_000);

  it("legacy: an XMLDecoder-built ContextFactory is a tallied fault too", async () => {
    expectHarnessFault("engine-api-unmeasured/ContextFactory");
    const script = viaXml("c.isSealed()").replace("org.mozilla.javascript.Context", "org.mozilla.javascript.ContextFactory");
    const error = await run(script, "legacy").then(() => undefined, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    expect((error as CaseRunError).response.emulation?.faults?.map((f: { id: string }) => f.id)).toContain("engine-api-unmeasured/ContextFactory");
  }, 30_000);

  // Regression for the Wrapper path in EngineGuard.wrap. NOTE: this route also
  // faults without that override (first use of the wrapper re-wraps its target),
  // so it does not discriminate the override; no route that does was found.
  // XMLDecoder can build a NativeJavaObject (already a Scriptable) around a
  // Context, over a real global scope: Rhino passes that straight through wrap(), not wrapAsJavaObject().
  it("legacy: an already-wrapped Context (a Wrapper returned by Java) is a tallied fault", async () => {
    expectHarnessFault("engine-api-unmeasured/Context");
    const xml = '<java><void id="c" class="org.mozilla.javascript.Context"/><object class="org.mozilla.javascript.NativeJavaObject"><object class="org.mozilla.javascript.ScriptRuntime" method="initSafeStandardObjects"><object idref="c"/><null/><boolean>false</boolean></object><object idref="c"/><class>java.lang.Object</class></object></java>';
    const script = viaXml("c.decompileFunction(function ordinary(a) { return a; }, 0)").replace(/var xml = '.*';/, `var xml = '${xml}';`);
    const error = await run(script, "legacy").then(() => undefined, (e: unknown) => e);
    expect(error).toBeInstanceOf(CaseRunError);
    expect((error as CaseRunError).response.emulation?.faults?.map((f: { id: string }) => f.id)).toContain("engine-api-unmeasured/Context");
  }, 30_000);

  it("controls: HashMap, template literal and a caught EcmaError's message record no fault (legacy)", async () => {
    const result = await run(
      `var m = new java.util.HashMap(); m.put("a", 1);
       var msg; try { null.x; } catch (e) { msg = String(e.message); }
       nodeState.putShared("r", m.size() + "|" + \`t\${1 + 1}\` + "|" + msg); outcome = "done";`,
      "legacy"
    );
    expect(String(result.effects.sharedState.final.r)).toMatch(/^1\|t2\|/);
    expect(result.response.emulation?.faults ?? []).toEqual([]);
  });

  it("controls: the same on next-gen records no fault", async () => {
    const result = await run(
      `var msg; try { null.x; } catch (e) { msg = String(e.message); }
       nodeState.putShared("r", \`t\${1 + 1}\` + "|" + msg); action.goTo("true");`,
      "am"
    );
    expect(String(result.effects.sharedState.final.r)).toMatch(/^t2\|/);
    expect(result.response.emulation?.faults ?? []).toEqual([]);
  });
});
