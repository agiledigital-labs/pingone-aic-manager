import vm from "node:vm";
import { describe, expect, it } from "vitest";
import { mockPreamble, withHarvest } from "../../src/bindings/preamble.ts";
import { loadBehaviour, sandboxFaults } from "./load-behaviour.ts";

type Sandbox = Record<string, unknown>;
type Methods = Record<string, ((...args: unknown[]) => unknown) | undefined>;

// Fault ids are kebab-case after an optional family prefix (`fixture/`,
// `arity-unknown/`); the tally groups by them, so a rename is a contract change.
describe("harness fault ids", () => {
  it.each([
    // A string is AIC's measured catchable refusal since 2026-11-06; a function is still unmeasured.
    ["node-state-merge-non-object", {}, (s: Sandbox) => (s.nodeState as Methods).mergeShared!(() => 0)],
    ["fixture/cache-manager", {}, (s: Sandbox) => (s.cacheManager as Methods).named!("c")],
  ])("%s", (id, given, call) => {
    const sandbox = loadBehaviour(given);
    expect(() => call(sandbox)).toThrow(/^rhino-local: /);
    expect(sandboxFaults(sandbox).map((f) => f.id)).toEqual([id]);
  });
});

// Round-3 P3-5, the runtime's own harvest (the JVM lane is HarnessFaults.java,
// covered in undefined-wire.e2e.test.ts): the wording answers for every fault.
// The harvest throws only when no runner recorder was captured, so this
// context has none.
describe("harvest wording over a mixed set of faults", () => {
  const gap = 'try { cacheManager.named("c"); } catch (e) {}';
  const defect =
    'try { httpClient.send("https://service.example.com/x", { method: "POST", body: { d: new Date(0) } }).get(); } catch (e) {}';
  const given = { http: [{ match: { url: "https://service.example.com/x" }, reply: { status: 200 } }] };
  const harvest = (script: string) => () => {
    const context = vm.createContext({});
    vm.runInContext(mockPreamble(given), context);
    return vm.runInContext(withHarvest(script), context);
  };
  it.each([
    ["gap then defect", `${gap}\n${gap}\n${defect}`, /^rhino-local harness fault fixture\/cache-manager: [^(]*\(and 1 more: 1 script defect\(s\)[^)]*and 1 behaviour\(s\) the harness cannot say/],
    ["defect then gap", `${defect}\n${gap}`, /^rhino-local harness fault script-defect\/send-body-js-date: [\s\S]*\(and 1 more: 1 script defect\(s\)[^)]*and 1 behaviour\(s\)/],
    ["gaps only", gap, /\(the script caught this refusal, but the harness cannot say what AIC does there, so the run fails anyway\)$/],
    ["defects only", defect, /\(a script defect: AIC's behaviour here is measured and is not what the script means, so the run fails even though the script caught it\)$/],
  ])("%s", (_name, script, message) => {
    expect(harvest(script)).toThrow(message);
  });
});
