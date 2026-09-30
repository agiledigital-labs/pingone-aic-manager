import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "session-property-effects",
  script: [
    'function t(f) { try { return String(f()); } catch (e) { return "threw:" + e; } }',
    "if (callbacks.isEmpty()) {",
    '  nodeState.putShared("putResult", t(function () { action.putSessionProperty("probe", "after"); return "ok"; }));',
    '  nodeState.putShared("removeResult", t(function () { action.removeSessionProperty("before"); return "ok"; }));',
    '  callbacksBuilder.nameCallback("Continue");',
    "} else {",
    '  var sessionType = t(function () { return typeof existingSession; });',
    '  var probeDot = t(function () { return existingSession.probe; });',
    '  var probeGet = t(function () { return existingSession.get("probe"); });',
    '  var beforeDot = t(function () { return existingSession.before; });',
    '  var beforeGet = t(function () { return existingSession.get("before"); });',
    '  var probePresent = t(function () { return existingSession.containsKey("probe"); });',
    '  var beforePresent = t(function () { return existingSession.containsKey("before"); });',
    '  var putResult = t(function () { return nodeState.get("putResult"); });',
    '  var removeResult = t(function () { return nodeState.get("removeResult"); });',
    '  nodeState.putShared("observedSessionType", sessionType);',
    '  nodeState.putShared("observedProbeDot", probeDot);',
    '  nodeState.putShared("observedProbeGet", probeGet);',
    '  nodeState.putShared("observedBeforeDot", beforeDot);',
    '  nodeState.putShared("observedBeforeGet", beforeGet);',
    '  nodeState.putShared("observedProbePresent", probePresent);',
    '  nodeState.putShared("observedBeforePresent", beforePresent);',
    '  nodeState.putShared("observedPut", putResult);',
    '  nodeState.putShared("observedRemove", removeResult);',
    '  action.goTo(sessionType === "object" && probeDot === "after" && probeGet === "after" && beforeDot === "undefined" && beforeGet === "null" && probePresent === "true" && beforePresent === "false" && putResult === "ok" && removeResult === "ok" ? "match" : "mismatch");',
    "}",
  ].join("\n"),
  outcomes: ["match", "mismatch"],
  always: { session: { before: "initial" } },
});

describe("session property effects", () => {
  const lease = useLease(suite, aicWhenEnabled("live-session-properties"));

  it("judges put/remove and reads the updated session on the next pass", async () => {
    const run = await lease.run().step({
      expect: {
        callbacks: [{ type: "NameCallback", prompt: "Continue" }],
        sessionProperties: { added: { probe: "after" }, removed: ["before"] },
        sharedState: { added: { putResult: /^.*$/, removeResult: /^.*$/ } },
      },
      reply: [{ type: "NameCallback", value: "go" }],
    }).expect({
      outcome: "match",
      sharedState: { added: {
        observedSessionType: "object",
        observedProbeDot: "after",
        observedProbeGet: "after",
        observedBeforeDot: "undefined",
        observedBeforeGet: "null",
        observedProbePresent: "true",
        observedBeforePresent: "false",
        observedPut: "ok",
        observedRemove: "ok",
      } },
    });
    expect(run.verdict.pass).toBe(true);
  });
});
