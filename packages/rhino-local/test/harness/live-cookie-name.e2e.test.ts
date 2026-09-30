import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "tenant-cookie-name",
  script: [
    'nodeState.putShared("cookieValue", cookieName);',
    'action.goTo(typeof cookieName === "string" && cookieName.length > 0 ? "match" : "mismatch");',
  ].join("\n"),
  outcomes: ["match", "mismatch"],
});

describe("tenant cookie name", () => {
  const lease = useLease(suite, aicWhenEnabled("live-cookie-name"));

  it("uses the same serverinfo value on both lanes", async () => {
    const run = await lease.run().expect({
      outcome: "match",
      allowUndeclared: { sharedState: true },
    });
    expect(run.verdict.pass).toBe(true);
    if (run.conformance !== undefined) {
      expect(run.conformance.disagreements).toEqual([]);
    }
  });
});
