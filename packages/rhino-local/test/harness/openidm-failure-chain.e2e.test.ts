import { describe, expect, it } from "vitest";
import { defineSuite, managed, useLease } from "../../src/harness/index.ts";

const ID = "00000000-0000-4000-8000-0000000000db";
const RESOURCE = `managed/alpha_user/${ID}`;
const PATCH = [{ operation: "replace", field: "sn", value: "retry" }];

// Local only: openidmFailures are AIC-ineligible.
const suite = defineSuite({
  name: "openidm-failure-across-passes",
  script: [
    `var ok = true; try { openidm.patch("${RESOURCE}", null, ${JSON.stringify(PATCH)}); } catch (e) { ok = false; }`,
    "if (callbacks.isEmpty()) {",
    '  nodeState.putShared("firstPass", ok);',
    '  callbacksBuilder.nameCallback("Retry");',
    "} else {",
    '  action.goTo(ok ? "saved" : "failedAgain");',
    "}",
  ].join("\n"),
  outcomes: ["saved", "failedAgain"],
  fixtures: {
    subject: managed("managed/alpha_user", {
      _id: ID, userName: "chain-failure-probe", givenName: "Chain",
      sn: "Probe", mail: "chain-failure-probe@example.com",
    }),
  },
  cleanup: async (idm) => {
    await idm.delete(RESOURCE);
  },
});

describe("openidm failure ordinals on a chain", () => {
  const lease = useLease(suite, { timeoutMs: 10_000 });

  it("fails patch #1 of the journey once, so the retry pass succeeds", async () => {
    // Regression: ordinals restarted on each pass, so the retry failed too.
    const write = { method: "patch" as const, resource: RESOURCE, body: PATCH };
    const run = await lease.run()
      .openidmFailures([{ match: { method: "patch", resource: RESOURCE, ordinal: 1 }, reply: { code: 503 } }])
      .step({
        expect: { callbacks: [{ type: "NameCallback", prompt: "Retry" }], openidm: [write], sharedState: { added: { firstPass: false } } },
        reply: [{ type: "NameCallback", value: "again" }],
      })
      .expect({ outcome: "saved", openidm: [write] });
    expect(run.verdict.pass).toBe(true);
  });
});
