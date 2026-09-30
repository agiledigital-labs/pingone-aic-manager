import { describe, expect, it } from "vitest";
import { createLeaseIdentity } from "../../src/aic/lease-identity.ts";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const SCRIPT = [
  'nodeState.putShared("scriptNameSeen", String(scriptName));',
  'nodeState.putShared("loggerNameSeen", String(logger.getName()));',
  'var missing = systemEnv.getProperty("esv.rl.probe.missing");',
  'action.goTo(callbacks.isEmpty() && missing === null ? "done" : "wrong");',
].join("\n");

const suite = defineSuite({
  name: "live-bindings",
  script: SCRIPT,
  outcomes: ["done", "wrong"],
  always: { esv: { "rl.probe.missing": null } },
});

describe("live first-pass bindings", () => {
  const aic = aicWhenEnabled("live-bindings");
  const lease = useLease(suite, aic);

  // Regression: these four first-pass bindings used to diverge between lanes.
  it("agrees on callbacks, a missing ESV, scriptName, and logger.getName", async () => {
    const identity = createLeaseIdentity({
      id: "live-bindings",
      source: SCRIPT,
      outcomes: suite.spec.outcomes,
    });
    const scriptName = aic.aic === undefined ? suite.spec.name : `${identity.treeName}-subject`;
    const scriptId = aic.aic === undefined ? "<unseeded-script-id>" : identity.ids.subjectScript;
    const run = await lease.run().expect({
      outcome: "done",
      sharedState: { added: {
        scriptNameSeen: scriptName,
        loggerNameSeen: `scripts.AUTHENTICATION_TREE_DECISION_NODE.${scriptId}.(${scriptName})`,
      } },
    });
    expect(run.verdict.pass).toBe(true);
    if (aic.aic !== undefined) {
      expect(run.conformance?.disagreements).toEqual([]);
    }
  });
});
