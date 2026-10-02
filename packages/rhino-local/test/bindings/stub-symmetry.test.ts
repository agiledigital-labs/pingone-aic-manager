import { describe, expect, it } from "vitest";
import { validateCase } from "../../src/case/index.ts";
import { caseWith } from "../aic/helpers.ts";
import { runScript } from "./load-behaviour.ts";

const stubs = {
  http: { match: { url: "https://example.com/api" }, reply: { status: 200, body: { nested: { value: "seeded" } } } },
  openidmActions: { match: { resource: "endpoint/example", action: "evaluate" }, reply: { body: {} } },
  openidmFailures: { match: { resource: "endpoint/example", method: "action", ordinal: 1 }, reply: { code: 503 } },
};

describe("stub diagnostics and reply isolation (review #5)", () => {
  for (const [channel, stub] of Object.entries(stubs)) {
    it.each(["match", "reply"])(`${channel} names a missing or malformed %s field`, (field) => {
      const missing = { ...stub } as Record<string, unknown>;
      delete missing[field];
      expect(() => validateCase({ ...caseWith(), given: { [channel]: [missing] } }))
        .toThrow(`rhino-local: case.given.${channel}[0].${field} is required`);
      for (const value of [null, [], 1]) {
        expect(() => validateCase({ ...caseWith(), given: { [channel]: [{ ...stub, [field]: value }] } }))
          .toThrow(`rhino-local: case.given.${channel}[0].${field} is not an object`);
      }
    });
    const field = channel === "http" ? "url" : "resource";
    it(`${channel} names a required matcher field`, () => {
      const match = { ...stub.match } as Record<string, unknown>;
      delete match[field];
      expect(() => validateCase({ ...caseWith(), given: { [channel]: [{ ...stub, match }] } }))
        .toThrow(`rhino-local: case.given.${channel}[0].match.${field} is required`);
    });
  }

  it.each([
    { channel: "http", field: "url", call: 'httpClient.send("https://example.com/api");' },
    { channel: "openidmActions", field: "resource", call: 'openidm.action("endpoint/example", "evaluate");' },
    { channel: "openidmFailures", field: "resource", call: 'openidm.action("endpoint/example", "evaluate");' },
  ] as const)("direct runtime callers get a $channel matcher diagnostic", ({ channel, field, call }) => {
    // Raw JS callers can bypass validateCase; diagnostics must name the actual channel.
    const stub = stubs[channel];
    expect(() => runScript(call, { [channel]: [{ ...stub, match: { ...stub.match, [field]: 1 } }] }))
      .toThrow(`rhino-local: given.${channel} match.${field} is not a string or regexp`);
  });

  it("HTTP replies isolate nested bodies and headers between calls", () => {
    const effects = runScript([
      'var first = httpClient.send("https://example.com/api").get();',
      'first.json().nested.value = "changed";',
      'first.headers.changed = "yes";',
      'var second = httpClient.send("https://example.com/api").get();',
      'nodeState.putShared("body", second.json());',
      'nodeState.putShared("headers", second.headers);',
    ].join("\n"), { http: [stubs.http] });
    expect(effects.sharedState.final).toEqual({ body: { nested: { value: "seeded" } }, headers: {} });
  });
});
