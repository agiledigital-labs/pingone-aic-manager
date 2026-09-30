import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "registered-state-container",
  script: [
    'function t(f) { try { return String(f()); } catch (e) { return "threw:" + e; } }',
    'var attributes = null;',
    'var mapType = t(function () { attributes = nodeState.get("objectAttributes"); return typeof attributes.put; });',
    'var putResult = t(function () { attributes.put("probe", "value"); return "ok"; });',
    'var directRead = t(function () { return attributes.get("probe"); });',
    'var laterRead = t(function () { return nodeState.get("objectAttributes").get("probe"); });',
    'nodeState.putShared("mapType", mapType);',
    'nodeState.putShared("mapPut", putResult);',
    'nodeState.putShared("mapDirectRead", directRead);',
    'nodeState.putShared("mapLaterRead", laterRead);',
    'action.goTo(mapType === "function" && putResult === "ok" && directRead === "value" && laterRead === "value" ? "match" : "mismatch");',
  ].join("\n"),
  outcomes: ["match", "mismatch"],
  always: { state: { shared: { objectAttributes: {} } } },
});

describe("registered state container", () => {
  const lease = useLease(suite, aicWhenEnabled("live-object-attributes"));

  it("persists a map write in shared state", async () => {
    const run = await lease.run().expect({
      outcome: "match",
      sharedState: {
        changed: { objectAttributes: { probe: "value" } },
        added: {
          mapType: "function",
          mapPut: "ok",
          mapDirectRead: "value",
          mapLaterRead: "value",
        },
      },
    });
    expect(run.verdict.pass).toBe(true);
  });
});
