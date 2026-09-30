import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "nested libraries",
  script: 'nodeState.putShared("value", require("outer").value); action.goTo("done");',
  outcomes: ["done"],
  libraries: {
    inner: 'exports.value = "from library";',
    outer: 'exports.value = require("inner").value;',
  },
});

describe("suite libraries", () => {
  const lease = useLease(suite, aicWhenEnabled("nested-libraries"));

  it("loads nested libraries through the public suite API", async () => {
    // Regression: direct runner options worked, but a suite could not reach them.
    const run = await lease.run().expect({
      outcome: "done",
      sharedState: { added: { value: "from library" } },
    });
    expect(run.verdict.summary).toBe("");
    expect(run.kase.given.libraries).toEqual(suite.spec.libraries);
  });
});
