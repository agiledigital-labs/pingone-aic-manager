import { readFileSync } from "node:fs";
import { join } from "node:path";
import { ESLint } from "eslint";
import { describe, expect, it } from "vitest";
import { realCases } from "../cases/real/index.ts";
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

  // The real corpus runs its origin fixtures directly. These use a construct
  // the AM rules ban, on purpose — that is what they probe — so each must be
  // rejected (keeping this list honest), and every other one must be clean.
  const deliberatelyBanned = new Set([
    "legacy-es2015-globals",
    "const-dup-across-blocks",
    "const-in-do-while-body",
    "const-in-for-in",
    "const-in-for-init",
    "const-in-for-of",
    "const-in-loop-body",
    "const-in-loop-in-function",
    "const-in-nested-loop-block",
    "const-in-while-body",
    "const-top-level",
    "default-params",
    "destructuring-object",
    "es2015-globals",
    "for-each-java-collection",
    "for-of-var",
    "object-shorthand",
    "rhino-let-behaviour",
  ]);

  it("accepts every real-corpus fixture not listed as deliberately banned", async () => {
    const problems: string[] = [];
    for (const entry of realCases) {
      if (deliberatelyBanned.has(entry.kase.name)) continue;
      problems.push(
        ...formatResults(await lintText(entry.kase.script, `cases/real/${entry.kase.name}.cjs`))
      );
    }
    expect(problems).toEqual([]);
  });

  it("rejects every real-corpus fixture listed as deliberately banned", async () => {
    const clean: string[] = [];
    for (const name of deliberatelyBanned) {
      const entry = realCases.find((candidate) => candidate.kase.name === name);
      expect(entry, name).toBeDefined();
      if (entry === undefined) continue;
      const results = await lintText(entry.kase.script, `cases/real/${name}.cjs`);
      if (formatResults(results).length === 0) clean.push(name);
    }
    expect(clean).toEqual([]);
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
