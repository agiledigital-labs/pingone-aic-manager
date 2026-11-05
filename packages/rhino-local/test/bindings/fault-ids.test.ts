import { describe, expect, it } from "vitest";
import { loadBehaviour, sandboxFaults } from "./load-behaviour.ts";

type Sandbox = Record<string, unknown>;
type Methods = Record<string, ((...args: unknown[]) => unknown) | undefined>;

// Fault ids are kebab-case after an optional family prefix (`fixture/`,
// `arity-unknown/`); the tally groups by them, so a rename is a contract change.
describe("harness fault ids", () => {
  it.each([
    ["node-state-merge-non-object", {}, (s: Sandbox) => (s.nodeState as Methods).mergeShared!("x")],
    ["fixture/cache-manager", {}, (s: Sandbox) => (s.cacheManager as Methods).named!("c")],
  ])("%s", (id, given, call) => {
    const sandbox = loadBehaviour(given);
    expect(() => call(sandbox)).toThrow(/^rhino-local: /);
    expect(sandboxFaults(sandbox).map((f) => f.id)).toEqual([id]);
  });
});
