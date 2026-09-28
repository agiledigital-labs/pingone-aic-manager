import { readFileSync } from "node:fs";
import vm from "node:vm";
import { bindingsJsonPath, generatedJsPath } from "../src/paths.ts";

export interface JsonParameter {
  name: string;
  javaScriptType: string;
}

export interface JsonElement {
  elementType: string;
  name: string;
  javaScriptType?: string;
  returnType?: string;
  parameters?: JsonParameter[];
  elements?: JsonElement[];
}

export interface JsonBinding {
  name: string;
  javaScriptType: string;
  elements?: JsonElement[];
}

export interface JsonDocument {
  _id: string;
  bindings: JsonBinding[];
}

export function readBindingsJson(): JsonDocument {
  const parsed: unknown = JSON.parse(readFileSync(bindingsJsonPath, "utf8"));
  if (typeof parsed !== "object" || parsed === null || !("bindings" in parsed)) {
    throw new Error("bindings JSON is not an object with bindings[]");
  }
  return parsed as JsonDocument;
}

export function loadGenerated(
  js = readFileSync(generatedJsPath, "utf8")
): Record<string, unknown> {
  const sandbox: Record<string, unknown> = {};
  vm.createContext(sandbox);
  vm.runInContext(js, sandbox, { filename: generatedJsPath });
  return sandbox;
}

export function asRecord(value: unknown, path: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null) {
    throw new Error(`expected object at ${path}, got ${typeof value}`);
  }
  return value as Record<string, unknown>;
}
