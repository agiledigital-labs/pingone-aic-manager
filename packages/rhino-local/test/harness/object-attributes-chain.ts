import { expect } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

/**
 * Pass 1 changes the registered `objectAttributes` map; pass 2 reports what
 * `nodeState.get("objectAttributes")` holds. One lease per file, so each
 * variant lives in its own `live-object-attributes-*.e2e.test.ts`.
 */
export function registeredMapChain(
  name: string,
  firstPass: string,
  second: { type: string; value: string }
): () => Promise<void> {
  const suite = defineSuite({
    name,
    script: [
      'function t(f) { try { return String(f()); } catch (e) { return "threw:" + e; } }',
      "if (callbacks.isEmpty()) {",
      `  ${firstPass}`,
      '  callbacksBuilder.nameCallback("Continue");',
      "} else {",
      '  nodeState.putShared("secondType", t(function () { var v = nodeState.get("objectAttributes"); return v === null ? "null" : typeof v; }));',
      '  nodeState.putShared("secondValue", t(function () { var v = nodeState.get("objectAttributes"); return typeof v === "object" && v !== null && typeof v.get === "function" ? "map:" + v.get("probe") : String(v); }));',
      '  action.goTo("done");',
      "}",
    ].join("\n"),
    outcomes: ["done"],
    always: { registeredObjectAttributes: { probe: "seeded" } },
  });
  const lease = useLease(suite, aicWhenEnabled(`live-${name}`));
  return async () => {
    const run = await lease.run().step({
      expect: {
        callbacks: [{ type: "NameCallback", prompt: "Continue" }],
        allowUndeclared: { sharedState: true },
      },
      reply: [{ type: "NameCallback", value: "continue" }],
    }).expect({
      outcome: "done",
      sharedState: { added: { secondType: second.type, secondValue: second.value } },
      allowUndeclared: { sharedState: true },
    });
    expect(run.verdict.pass).toBe(true);
  };
}
