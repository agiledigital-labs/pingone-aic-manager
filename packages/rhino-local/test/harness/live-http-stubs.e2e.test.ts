import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const URL = "https://example.com/api";
const suite = defineSuite({
  name: "http-stub-channel",
  script: [
    `var reply = httpClient.send("${URL}").get();`,
    'action.goTo(reply.status === 503 ? "unavailable" : "other");',
  ].join("\n"),
  outcomes: ["unavailable", "other"],
  always: { http: [{ match: { url: URL }, reply: { status: 200 } }] },
  beforeRun: ({ request }) => {
    request.http.push({ match: { url: "https://example.com/unused" }, reply: { status: 204 } });
  },
});

describe("HTTP stubs", () => {
  const aic = aicWhenEnabled("live-http-stubs");
  const lease = useLease(suite, {
    ...aic,
    ...(aic.aic === undefined ? {} : { aic: { ...aic.aic, unsupported: "skip" as const } }),
  });

  it("uses the per-test reply and reports the AIC observation gap", async () => {
    const run = await lease.run().http([{ match: { url: URL }, reply: { status: 503 } }]).expect({
      outcome: "unavailable",
      http: [{ url: URL, method: "GET" }],
    });
    expect(run.verdict.pass).toBe(true);
    if (aic.aic !== undefined) {
      expect(run.conformance?.passes[0]?.aic.skipped).toMatch(/given\.http/);
      expect(run.conformance?.observationGaps).toEqual(expect.arrayContaining([
        expect.objectContaining({ path: "aic-lane", aic: "ineligible" }),
      ]));
    }
  });
});
