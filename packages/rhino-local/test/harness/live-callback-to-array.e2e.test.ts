import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "callback-array-shape",
  script: [
    "var names = callbacks.getNameCallbacks().toArray();",
    "var strings = callbacks.getStringAttributeInputCallbacks().toArray();",
    'action.goTo(!Array.isArray(names) && names.length === 0 && !Array.isArray(strings) && strings.length === 0 ? "match" : "mismatch");',
  ].join("\n"),
  outcomes: ["match", "mismatch"],
});

describe("callback array shape", () => {
  const lease = useLease(suite, aicWhenEnabled("live-callback-to-array"));

  it("returns Java arrays for empty getter lists", async () => {
    const run = await lease.run().expect({ outcome: "match" });
    expect(run.verdict.pass).toBe(true);
  });
});
