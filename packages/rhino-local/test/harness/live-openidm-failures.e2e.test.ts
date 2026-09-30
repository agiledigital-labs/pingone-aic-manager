import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, managed, useLease } from "../../src/harness/index.ts";

const RESOURCE = "managed/alpha_user/00000000-0000-4000-8000-0000000000d9";
const first = [{ operation: "replace", field: "sn", value: "first" }];
const second = [{ operation: "replace", field: "sn", value: "second" }];

const suite = defineSuite({
  name: "numbered-openidm-failure",
  script: [
    `openidm.patch("${RESOURCE}", null, ${JSON.stringify(first)});`,
    `try { openidm.patch("${RESOURCE}", null, ${JSON.stringify(second)}); }`,
    'catch (error) { action.goTo(error.code === 409 && String(error).indexOf("ResourceExceptionScriptAdapter") !== -1 ? "caught" : "wrongError"); }',
  ].join("\n"),
  outcomes: ["caught", "wrongError"],
  fixtures: {
    subject: managed("managed/alpha_user", {
      _id: "00000000-0000-4000-8000-0000000000d9", userName: "failure-probe",
      givenName: "Failure", sn: "old", mail: "failure-probe@example.com",
    }),
  },
  always: {
    openidmFailures: [{ match: { method: "read", resource: RESOURCE, ordinal: 1 }, reply: { code: 500 } }],
  },
  beforeRun: ({ request }) => {
    request.openidmFailures.push({ match: { method: "delete", resource: RESOURCE, ordinal: 1 }, reply: { code: 403 } });
  },
  cleanup: async (idm) => {
    await idm.delete(RESOURCE);
  },
});

describe("numbered openidm failures", () => {
  const aic = aicWhenEnabled("live-openidm-failures");
  const lease = useLease(suite, {
    ...aic,
    ...(aic.aic === undefined ? {} : { aic: { ...aic.aic, unsupported: "skip" as const } }),
  });

  it("fails only the second patch and exposes the AIC observation gap", async () => {
    const run = await lease.run().openidmFailures([
      { match: { method: "patch", resource: RESOURCE, ordinal: 2 }, reply: { code: 409 } },
    ]).expect({
      outcome: "caught",
      openidm: [
        { method: "patch", resource: RESOURCE, body: first },
        { method: "patch", resource: RESOURCE, body: second },
      ],
    });
    expect(run.verdict.pass).toBe(true);
    if (aic.aic !== undefined) {
      expect(run.conformance?.passes[0]?.aic.skipped).toMatch(/openidmFailures/);
      expect(run.conformance?.observationGaps).toEqual(expect.arrayContaining([
        expect.objectContaining({ path: "aic-lane", aic: "ineligible" }),
      ]));
    }
  });
});
