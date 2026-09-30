import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const HEX_ID = /^[0-9a-f]{32}$/;
// The both-JVM runner compares raw job output byte for byte. Its two JVMs
// cannot generate the same random value, so use a shaped constant in that mode.
// The ordinary host and live AIC lanes still exercise independent random IDs.
const hexDigit =
  process.env.AIC_SCRIPT_TESTER_JVM === "both"
    ? '"0123456789abcdef".charAt(i % 16)'
    : "Math.floor(Math.random() * 16).toString(16)";
const suite = defineSuite({
  name: "random tracking id",
  script: [
    "if (callbacks.isEmpty()) {",
    '  var id = "";',
    "  for (var i = 0; i < 32; i += 1) {",
    `    id += ${hexDigit};`,
    "  }",
    '  nodeState.putShared("trackingId", id);',
    '  callbacksBuilder.nameCallback("Name");',
    "} else {",
    '  nodeState.putShared("seen", nodeState.get("trackingId"));',
    '  action.goTo("done");',
    "}",
  ].join("\n"),
  outcomes: ["done"],
});

describe("random state matcher across a callback round trip", () => {
  const lease = useLease(suite, aicWhenEnabled("random-tracking-id"));

  it("validates each lane's hex-shaped seed and carries the tenant's own value", async () => {
    // Regression: byte-exact lane diff and pass-2 seed check rejected random values.
    const run = await lease.run().step({
      expect: {
        sharedState: { added: { trackingId: HEX_ID } },
        callbacks: [{ type: "NameCallback", prompt: "Name" }],
      },
      reply: [{ type: "NameCallback", value: "alice" }],
    }).expect({
      outcome: "done",
      sharedState: { added: { seen: HEX_ID } },
    });
    expect(run.verdict.summary).toBe("");
    expect(run.kase.given.sharedState?.trackingId).toMatch(HEX_ID);
    if (run.conformance !== undefined) {
      expect(run.conformance.disagreements).toEqual([]);
    }
  });
});
