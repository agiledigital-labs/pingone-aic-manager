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
    'action.goTo("done");',
  ].join("\n"),
  outcomes: ["done"],
});

describe("JavaImporter scope", () => {
  const lease = useLease(suite, aicWhenEnabled("live-java-importer-scope"));

  // Measured 2026-10-01: JsonValue comes through its class or its package,
  // as a Java class (typeof "function"), and through no other importer.
  it("exposes JsonValue only to an importer that names it", async () => {
    const run = await lease.run().expect({
      outcome: "done",
      sharedState: { added: {
        viaClass: "function",
        viaPackage: "function",
        viaPackageObject: "function",
        viaEmpty: "undefined",
        viaUnrelated: "undefined",
      } },
    });
    expect(run.verdict.pass).toBe(true);
  });
});
