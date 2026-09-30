import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "session-property-effects",
  script: [
    "if (callbacks.isEmpty()) {",
    '  action.putSessionProperty("probe", "after");',
    '  action.removeSessionProperty("before");',
    '  callbacksBuilder.nameCallback("Continue");',
    "} else {",
    '  action.goTo(existingSession && existingSession.probe === "after" && existingSession.before === undefined ? "match" : "mismatch");',
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
      },
      reply: [{ type: "NameCallback", value: "go" }],
    }).expect({ outcome: "match" });
    expect(run.verdict.pass).toBe(true);
  });
});
