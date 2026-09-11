import vm from "node:vm";
import { parseHarvest } from "../../src/bindings/harvest.ts";
import {
  mockPreamble,
  withHarvest,
  type MockPreambleOptions,
} from "../../src/bindings/preamble.ts";
import type { Given, RecordedEffects } from "../../src/case/types.ts";

export function loadBehaviour(
  given: Given = {},
  options: MockPreambleOptions = {}
): Record<string, unknown> {
  const sandbox: Record<string, unknown> = {};
  vm.createContext(sandbox);
  vm.runInContext(mockPreamble(given, options), sandbox);
  return sandbox;
}

export function runScript(
  script: string,
  given: Given = {},
  options: MockPreambleOptions = {}
): RecordedEffects {
  const sandbox = loadBehaviour(given, options);
  const value = vm.runInContext(withHarvest(script), sandbox);
  if (typeof value !== "string") {
    throw new Error(`expected harvest string, got ${typeof value}`);
  }
  return parseHarvest(value);
}

export function asRecord(value: unknown, path: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null) {
    throw new Error(`expected object at ${path}, got ${typeof value}`);
  }
  return value as Record<string, unknown>;
}
