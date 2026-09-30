import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, managed, useLease } from "../../src/harness/index.ts";

const ID = "00000000-0000-4000-8000-0000000000da";
const RESOURCE = `managed/alpha_user/${ID}`;

const suite = defineSuite({
  name: "real-openidm-error-shape",
  script: [
    `try { openidm.patch("${RESOURCE}", "0", [{ operation: "replace", field: "sn", value: "changed" }]); }`,
    "catch (error) {",
    '  nodeState.putShared("adapter", String(error).indexOf("ResourceExceptionScriptAdapter") !== -1);',
    '  nodeState.putShared("codeType", typeof error.code);',
    '  action.goTo("caught");',
    "}",
  ].join("\n"),
  outcomes: ["caught"],
  fixtures: {
    subject: managed("managed/alpha_user", {
      _id: ID, userName: "error-shape-probe", givenName: "Error",
      sn: "Original", mail: "error-shape-probe@example.com",
    }),
  },
  cleanup: async (idm) => {
    const record = await idm.read(RESOURCE);
    expect(record?.sn).toBe("Original");
    await idm.delete(RESOURCE);
  },
});

describe("real openidm error shape", () => {
  const lease = useLease(suite, aicWhenEnabled("live-openidm-error-shape"));

  it("records the next-gen exception family and code property type", async () => {
    const run = await lease.run().expect({
      outcome: "caught",
      sharedState: { added: { adapter: true, codeType: "undefined" } },
      allowUndeclared: { openidmWrites: true },
    });
    expect(run.verdict.pass).toBe(true);
  });
});
