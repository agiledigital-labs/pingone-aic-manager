import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import ts from "typescript";
import { describe, expect, it } from "vitest";
import { generate, generatedDtsName, generatedJsName } from "../src/generate.ts";
import { loadContext } from "../src/load.ts";
import {
  bindingsJsonPath,
  generatedDtsPath,
  generatedJsPath,
} from "../src/paths.ts";
import {
  asRecord,
  loadGenerated,
  readBindingsJson,
  type JsonBinding,
  type JsonDocument,
  type JsonElement,
} from "./load-generated.ts";

describe("completeness against the contexts JSON", () => {
  const doc = readBindingsJson();
  const sandbox = loadGenerated();

  it("names every binding from the JSON on the generated surface", () => {
    expect(missingBindings(doc, sandbox)).toEqual([]);
  });

  it("names every method and nested field from the JSON on the generated surface", () => {
    expect(missingMembers(doc, sandbox)).toEqual([]);
  });

  it("types scalar bindings as their JSON javaScriptType", () => {
    for (const binding of doc.bindings) {
      if (!isScalar(binding.javaScriptType)) {
        continue;
      }
      expect(typeof sandbox[binding.name], binding.name).toBe(binding.javaScriptType);
    }
  });

  it("emits empty-elements bindings as seedable objects", () => {
    for (const binding of doc.bindings) {
      if (!isOpaque(binding)) {
        continue;
      }
      const value = asRecord(sandbox[binding.name], binding.name);
      value.seeded = "by-the-case";
      expect(value.seeded).toBe("by-the-case");
    }
  });

  it("fails when a JSON method is missing from the artefact", () => {
    const broken = loadGenerated();
    delete asRecord(broken.logger, "logger").info;
    expect(missingMembers(doc, broken)).toContain("logger.info");
  });

  it("fails when a nested JSON field is missing from the artefact", () => {
    const broken = loadGenerated();
    delete asRecord(asRecord(broken.utils, "utils").crypto, "utils.crypto").subtle;
    expect(missingMembers(doc, broken)).toContain("utils.crypto.subtle");
  });

  it("fails when a JSON binding is missing from the artefact", () => {
    const broken = loadGenerated();
    delete broken.nodeState;
    expect(missingBindings(doc, broken)).toContain("nodeState");
  });

  it("gives the .d.ts the same unique method-signature count as the JSON", () => {
    const dts = readFileSync(generatedDtsPath, "utf8");
    expect(dtsMethodSignatureCount(dts)).toBe(jsonUniqueSignatureCount(doc));
  });

  it("fails when a JSON overload is dropped from the .d.ts", () => {
    const dts = readFileSync(generatedDtsPath, "utf8");
    const stripped = dts.replace("  info(msg: string): void;\n", "");
    expect(stripped).not.toBe(dts);
    expect(dtsMethodSignatureCount(stripped)).toBeLessThan(jsonUniqueSignatureCount(doc));
  });

  it("matches the committed artefacts (forgot-to-regenerate check)", () => {
    const dir = mkdtempSync(join(tmpdir(), "rhino-local-"));
    try {
      generate(bindingsJsonPath, dir);
      expect(readFileSync(join(dir, generatedJsName), "utf8")).toBe(
        readFileSync(generatedJsPath, "utf8")
      );
      expect(readFileSync(join(dir, generatedDtsName), "utf8")).toBe(
        readFileSync(generatedDtsPath, "utf8")
      );
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("parser fail-closed", () => {
  it("throws on an unknown elementType", () => {
    const dir = mkdtempSync(join(tmpdir(), "rhino-local-"));
    const path = join(dir, "bad.json");
    try {
      writeFileSync(
        path,
        JSON.stringify({
          _id: "SCRIPTED_DECISION_NODE",
          bindings: [
            {
              name: "logger",
              javaScriptType: "object",
              elements: [{ elementType: "property", name: "info" }],
            },
          ],
        })
      );
      expect(() => loadContext(path)).toThrow(/unknown elementType/);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

function isScalar(jsType: string): boolean {
  return jsType === "string" || jsType === "number" || jsType === "boolean";
}

function isOpaque(binding: JsonBinding): boolean {
  return !isScalar(binding.javaScriptType) && (binding.elements ?? []).length === 0;
}

function missingBindings(
  doc: JsonDocument,
  sandbox: Record<string, unknown>
): string[] {
  return doc.bindings
    .map((binding) => binding.name)
    .filter((name) => !Object.hasOwn(sandbox, name));
}

function missingMembers(
  doc: JsonDocument,
  sandbox: Record<string, unknown>
): string[] {
  const missing: string[] = [];
  for (const binding of doc.bindings) {
    if (isScalar(binding.javaScriptType) || isOpaque(binding)) {
      continue;
    }
    if (!Object.hasOwn(sandbox, binding.name)) {
      missing.push(binding.name);
      continue;
    }
    walkMembers(binding.elements ?? [], sandbox[binding.name], binding.name, missing);
  }
  return [...new Set(missing)];
}

function walkMembers(
  elements: JsonElement[],
  holder: unknown,
  path: string,
  missing: string[]
): void {
  const record =
    typeof holder === "object" && holder !== null
      ? (holder as Record<string, unknown>)
      : undefined;
  for (const element of elements) {
    const childPath = `${path}.${element.name}`;
    if (element.elementType === "method") {
      if (typeof record?.[element.name] !== "function") {
        missing.push(childPath);
      }
      continue;
    }
    if (element.elementType === "field") {
      const child = record?.[element.name];
      if (typeof child !== "object" || child === null) {
        missing.push(childPath);
        continue;
      }
      walkMembers(element.elements ?? [], child, childPath, missing);
      continue;
    }
    missing.push(`${childPath} (unknown elementType ${element.elementType})`);
  }
}

function jsonUniqueSignatureCount(doc: JsonDocument): number {
  const seen = new Set<string>();
  function walk(elements: JsonElement[], path: string): void {
    for (const element of elements) {
      if (element.elementType === "method") {
        const params = (element.parameters ?? [])
          .map((parameter) => `${parameter.name}:${parameter.javaScriptType}`)
          .join(",");
        seen.add(`${path}.${element.name}(${params})->${element.returnType ?? ""}`);
        continue;
      }
      if (element.elementType === "field") {
        walk(element.elements ?? [], `${path}.${element.name}`);
      }
    }
  }
  for (const binding of doc.bindings) {
    walk(binding.elements ?? [], binding.name);
  }
  return seen.size;
}

function dtsMethodSignatureCount(dts: string): number {
  const source = ts.createSourceFile(
    "scripted-decision-mocks.d.ts",
    dts,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS
  );
  let count = 0;
  function visit(node: ts.Node): void {
    if (ts.isMethodSignature(node)) {
      count += 1;
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
  return count;
}
