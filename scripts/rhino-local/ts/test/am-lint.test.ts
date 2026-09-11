import { readFileSync } from "node:fs";
import { join } from "node:path";
import { ESLint } from "eslint";
import { describe, expect, it } from "vitest";
import {
  amEslintConfigPath,
  amRhinoEslintConfigPath,
  bindingsRuntimePath,
  casesDir,
  generatedJsPath,
  packageRoot,
} from "../src/paths.ts";

describe("AM script lint", () => {
  it("accepts the generated mock", async () => {
    const results = await lintFiles([generatedJsPath]);
    expect(formatResults(results)).toEqual([]);
  });

  it("accepts the handwritten behaviour overlay", async () => {
    const results = await lintFiles([bindingsRuntimePath]);
    expect(formatResults(results)).toEqual([]);
  });

  it("accepts the scripted-decision case scripts", async () => {
    const results = await lintFiles([
      join(casesDir, "decide-from-state.cjs"),
      join(casesDir, "write-state.cjs"),
      join(casesDir, "openidm-read.cjs"),
    ]);
    expect(formatResults(results)).toEqual([]);
  });

  it("rejects `let`, proving the Rhino rules are actually loaded", async () => {
    const results = await lintText("var ok = 1;\nlet banned = 2;\n", "generated/let-probe.cjs");
    const messages = results.flatMap((result) => result.messages.map((message) => message.message));
    expect(messages.some((message) => message.includes("'let' is a parse error"))).toBe(true);
  });

  it("stays in lockstep with the project's AM ESLint config", () => {
    const am = readFileSync(amEslintConfigPath, "utf8");
    const ours = readFileSync(amRhinoEslintConfigPath, "utf8");

    const amSelectors = [...am.matchAll(/selector:\s*"([^"]+)"/g)].map((match) => match[1]);
    expect(amSelectors.length).toBeGreaterThan(0);
    for (const selector of amSelectors) {
      expect(ours, selector).toContain(selector);
    }

    for (const name of ["Map", "WeakMap", "Set", "WeakSet", "Symbol", "Promise", "Proxy", "Reflect"]) {
      expect(am).toContain(`"${name}"`);
      expect(ours).toContain(`"${name}"`);
    }

    for (const needle of [
      "rhino/no-dup-const",
      "rhino/no-const-in-loop-body",
      "'const' inside a for/for-in/for-of/while/do-while loop body",
      "Top-level 'const' parses but reads back as undefined",
      "'{{name}}' const is re-declared in this function",
    ]) {
      expect(am).toContain(needle);
      expect(ours).toContain(needle);
    }
  });
});

function amEslint(): ESLint {
  return new ESLint({
    cwd: packageRoot,
    overrideConfigFile: amRhinoEslintConfigPath,
  });
}

function lintFiles(files: string[]): Promise<ESLint.LintResult[]> {
  return amEslint().lintFiles(files);
}

function lintText(code: string, filePath: string): Promise<ESLint.LintResult[]> {
  return amEslint().lintText(code, { filePath: join(packageRoot, filePath) });
}

function formatResults(results: ESLint.LintResult[]): string[] {
  const lines: string[] = [];
  for (const result of results) {
    for (const message of result.messages) {
      lines.push(`${result.filePath}:${message.line} ${message.message}`);
    }
  }
  return lines;
}
