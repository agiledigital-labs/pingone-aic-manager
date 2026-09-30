import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "request-cookie-channels",
  script: [
    'action.goTo(requestCookies.get("suite") === "one" && requestCookies.get("test") === "two" && requestCookies.get("hook") === "three" && requestCookies.get("missing") === null ? "match" : "mismatch");',
  ].join("\n"),
  outcomes: ["match", "mismatch"],
  always: { cookies: { suite: "one" } },
  beforeRun: ({ request }) => {
    request.cookies.hook = "three";
  },
});

describe("request cookies", () => {
  const lease = useLease(suite, aicWhenEnabled("live-request-cookies"));

  it("sends merged cookies to the subject", async () => {
    const run = await lease.run().cookies({ test: "two" }).expect({ outcome: "match" });
    expect(run.kase.given.requestCookies).toEqual({ suite: "one", test: "two", hook: "three" });
    expect(run.verdict.pass).toBe(true);
  });
});
