import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
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
