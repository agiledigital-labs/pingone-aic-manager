import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "java-importer-scope",
  script: [
    'function t(f) { try { return String(f()); } catch (e) { return "threw:" + e; } }',
    'nodeState.putShared("viaClass", t(function () { return typeof JavaImporter(org.forgerock.json.JsonValue).JsonValue; }));',
    'nodeState.putShared("viaPackage", t(function () { return typeof JavaImporter(org.forgerock.json).JsonValue; }));',
    'nodeState.putShared("viaPackageObject", t(function () { return typeof JavaImporter(org.forgerock.json).JsonValue.object; }));',
    'nodeState.putShared("viaEmpty", t(function () { return typeof JavaImporter().JsonValue; }));',
    'nodeState.putShared("viaUnrelated", t(function () { return typeof JavaImporter(java.util).JsonValue; }));',
    'nodeState.putShared("actionViaEmpty", t(function () { return typeof JavaImporter().Action; }));',
    'nodeState.putShared("actionViaUnrelated", t(function () { return typeof JavaImporter(java.util).Action; }));',
    'nodeState.putShared("callbackViaEmpty", t(function () { return typeof JavaImporter().HiddenValueCallback; }));',
    'nodeState.putShared("callbackViaUnrelated", t(function () { return typeof JavaImporter(java.util).HiddenValueCallback; }));',
    'nodeState.putShared("actionViaClass", t(function () { return typeof JavaImporter(org.forgerock.openam.auth.node.api.Action).Action; }));',
    'nodeState.putShared("callbackViaClass", t(function () { return typeof JavaImporter(com.sun.identity.authentication.callbacks.HiddenValueCallback).HiddenValueCallback; }));',
    'action.goTo("done");',
  ].join("\n"),
  outcomes: ["done"],
});

describe("JavaImporter scope", () => {
  const lease = useLease(suite, aicWhenEnabled("live-java-importer-scope"));

  // Measured 2026-10-01: JsonValue comes through its class or its package,
  // as a Java class (typeof "function"), and through no other importer. The
  // legacy result classes Action and HiddenValueCallback are absent on
  // next-gen from every importer, including one that names them.
  it("exposes JsonValue only to an importer that names it", async () => {
    const run = await lease.run().expect({
      outcome: "done",
      sharedState: { added: {
        viaClass: "function",
        viaPackage: "function",
        viaPackageObject: "function",
        viaEmpty: "undefined",
        viaUnrelated: "undefined",
        actionViaEmpty: "undefined",
        actionViaUnrelated: "undefined",
        callbackViaEmpty: "undefined",
        callbackViaUnrelated: "undefined",
        actionViaClass: "undefined",
        callbackViaClass: "undefined",
      } },
    });
    expect(run.verdict.pass).toBe(true);
  });
});
