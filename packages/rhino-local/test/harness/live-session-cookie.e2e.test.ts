import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "session-cookie-in-request-cookies",
  script: [
    'function t(f) { try { return String(f()); } catch (e) { return "threw:" + e; } }',
    'nodeState.putShared("sessionPresent", t(function () { return requestCookies.get(cookieName) !== null; }));',
    'nodeState.putShared("sessionContains", t(function () { return requestCookies.containsKey(cookieName); }));',
    'nodeState.putShared("sessionValueType", t(function () { return typeof requestCookies.get(cookieName); }));',
    'nodeState.putShared("cookieCount", t(function () { return requestCookies.size(); }));',
    'nodeState.putShared("authorCookie", t(function () { return requestCookies.get("testCookie"); }));',
    'action.goTo("done");',
  ].join("\n"),
  outcomes: ["done"],
  always: { session: {}, cookies: { testCookie: "one" } },
});

describe("session cookie", () => {
  const lease = useLease(suite, aicWhenEnabled("live-session-cookie"));

  // Measured 2026-10-01: AM lists the session cookie beside the author's
  // cookies, as a string. Only presence is asserted; the token is per-run.
  it("lists the session cookie in requestCookies", async () => {
    const run = await lease.run().expect({
      outcome: "done",
      sharedState: { added: { sessionPresent: "true", sessionContains: "true", sessionValueType: "string", cookieCount: "2", authorCookie: "one" } },
    });
    expect(run.verdict.pass).toBe(true);
  });
});
