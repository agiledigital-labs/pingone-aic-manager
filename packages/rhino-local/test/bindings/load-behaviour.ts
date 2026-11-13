import vm from "node:vm";
import { expect } from "vitest";
import { parseHarvest } from "../../src/bindings/harvest.ts";
import {
  mockPreamble,
  withHarvest,
  type MockPreambleOptions,
} from "../../src/bindings/preamble.ts";
import type { Given, RecordedEffects } from "../../src/case/types.ts";
import type { HarnessFault } from "../../src/protocol.ts";

const faultsKey = Symbol("harness faults");

/**
 * The runtime in a `node:vm` context, with the same fault recorder contract as
 * the Java runner (java/HarnessFaults.java): a function named
 * `__rhinoLocalHarnessFault` that exists while the preamble runs and is gone
 * before the script does. The runtime's own code captures it.
 */
export function loadBehaviour(
  given: Given = {},
  options: MockPreambleOptions = {}
): Record<string, unknown> {
  const faults: HarnessFault[] = [];
  const sandbox: Record<string | symbol, unknown> = {
    __rhinoLocalHarnessFault: (id: unknown, message: unknown) => {
      faults.push({ id: String(id), message: String(message) });
    },
    [faultsKey]: faults,
  };
  vm.createContext(sandbox);
  vm.runInContext(mockPreamble(given, options), sandbox);
  delete sandbox.__rhinoLocalHarnessFault;
  return sandbox;
}

/** Faults the runtime recorded in a sandbox from {@link loadBehaviour}. */
export function sandboxFaults(sandbox: Record<string, unknown>): HarnessFault[] {
  return (sandbox as Record<symbol, HarnessFault[]>)[faultsKey] ?? [];
}

/**
 * Run a script and return its effects plus every harness fault it hit, caught
 * or not. For tests that assert what a refusal left behind; the fault itself
 * must be asserted too.
 */
export function runScriptWithFaults(
  script: string,
  given: Given = {},
  options: MockPreambleOptions = {}
): { effects: RecordedEffects; faults: HarnessFault[] } {
  const sandbox = loadBehaviour(given, options);
  const value = vm.runInContext(withHarvest(script), sandbox);
  if (typeof value !== "string") {
    throw new Error(`expected harvest string, got ${typeof value}`);
  }
  return { effects: parseHarvest(value), faults: sandboxFaults(sandbox) };
}

/** As the runner would: a run that hit a harness fault fails, caught or not. */
export function runScript(
  script: string,
  given: Given = {},
  options: MockPreambleOptions = {}
): RecordedEffects {
  const { effects, faults } = runScriptWithFaults(script, given, options);
  if (faults.length > 0) {
    throw new Error(`harness fault ${faults.map((fault) => fault.id).join(", ")}: ${faults[0]!.message}`);
  }
  return effects;
}

export function asRecord(value: unknown, path: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null) {
    throw new Error(`expected object at ${path}, got ${typeof value}`);
  }
  return value as Record<string, unknown>;
}

/**
 * The script caught a refusal and kept its text in shared state `error` (the
 * tests' catch idiom). Asserts that refusal was a harness fault with `id`, and
 * that it is the only fault the run recorded — so the run would fail under the
 * runner however the script handled it.
 */
export function expectCaughtFault(
  run: { effects: RecordedEffects; faults: HarnessFault[] },
  id: string
): void {
  expect([...new Set(run.faults.map((fault) => fault.id))]).toEqual([id]);
  expect(String(run.effects.sharedState.final.error)).toContain(run.faults[0]!.message);
}
