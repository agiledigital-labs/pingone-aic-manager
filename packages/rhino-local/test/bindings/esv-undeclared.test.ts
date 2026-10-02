import { describe, expect, it } from "vitest";
import { loadBehaviour, runScript } from "./load-behaviour.ts";

describe("undeclared ESV policy (D11)", () => {
  it.each([{}, { esvUndeclared: "error" as const }])("keeps strict reads by default and names both remedies", (given) => {
    expect(() => runScript('systemEnv.getProperty("esv.missing");', given))
      .toThrow(/no given.esv entry.*declare it in given.esv.*given.esvUndeclared: "absent".*\.esvUndeclared\("absent"\)/);
  });

  it.each([
    { args: '"esv.missing"', expected: null },
    { args: '"esv.missing", "fallback"', expected: "fallback" },
    { args: '"esv.missing", null, "not-a-type"', expected: null },
    { args: '"esv.missing", "42", "integer"', expected: 42 },
    { args: '"esv.missing", "false", "boolean"', expected: false },
  ])("matches a declared null for $args", ({ args, expected }) => {
    for (const given of [{ esv: { "esv.missing": null } }, { esvUndeclared: "absent" as const }]) {
      const effects = runScript(`nodeState.putShared("value", systemEnv.getProperty(${args}));`, given);
      expect(effects.sharedState.final.value).toEqual(expected);
    }
  });

  it("preserves declared values under the opt-in", () => {
    const sandbox = loadBehaviour({ esvUndeclared: "absent", esv: { "esv.known": "on" } });
    const systemEnv = sandbox.systemEnv as { getProperty(key: string): unknown };
    expect(systemEnv.getProperty("esv.known")).toBe("on");
    expect(systemEnv.getProperty("esv.other")).toBeNull();
  });
});
