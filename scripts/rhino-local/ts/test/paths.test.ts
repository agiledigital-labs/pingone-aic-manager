import { existsSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { amEslintConfigPath, bindingsJsonPath } from "../src/paths.ts";

describe("paths", () => {
  it("resolves the captured scripted-decision contexts JSON", () => {
    expect(existsSync(bindingsJsonPath)).toBe(true);
    const parsed: unknown = JSON.parse(readFileSync(bindingsJsonPath, "utf8"));
    expect(parsed).toMatchObject({ _id: "SCRIPTED_DECISION_NODE" });
  });

  it("resolves the project's AM ESLint config", () => {
    expect(existsSync(amEslintConfigPath)).toBe(true);
  });
});
