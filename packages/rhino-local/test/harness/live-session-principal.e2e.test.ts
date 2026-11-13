/**
 * IMPROVEMENTS #9: a session principal separate from shared-state `username`,
 * the same in both lanes. On AIC the mini journey logs in as the principal, so
 * `existingSession.UserId` names it while `nodeState` still holds the
 * suite's `username`.
 */
import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "session-principal",
  script: [
    'var who = String(existingSession.get("UserId"));',
    'var principal = String(existingSession.get("Principal"));',
    'var user = String(nodeState.get("username"));',
    'nodeState.putShared("who", who);',
    'action.goTo(who === "rl-principal" && principal.indexOf("id=rl-principal,ou=user,") === 0 && user === "rl-state-user" ? "match" : "mismatch");',
  ].join("\n"),
  outcomes: ["match", "mismatch"],
  always: { state: { shared: { username: "rl-state-user" } } },
});

describe("session principal", () => {
  const lease = useLease(suite, aicWhenEnabled("live-session-principal"));

  it("mints existingSession for .sessionPrincipal(), leaving state.username alone", async () => {
    const run = await lease.run().sessionPrincipal("rl-principal").expect({ outcome: "match", sharedState: { added: { who: "rl-principal" } } });
    expect(run.verdict.pass).toBe(true);
  });
});
