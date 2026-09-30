import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "first-pass-callbacks",
  script: 'action.goTo(callbacks.isEmpty() ? "empty" : "submitted");',
  outcomes: ["empty", "submitted"],
});

describe("first-pass-callbacks", () => {
  const lease = useLease(suite, aicWhenEnabled("first-pass-callbacks"));

  // Regression: a single-pass lease used to leave callbacks unseeded.
  it("sees an empty submitted list without declaring a step", async () => {
    const run = await lease.run().expect({ outcome: "empty" });
    expect(run.kase.given.callbacks).toEqual([]);
    expect(run.verdict.pass).toBe(true);
  });
});
