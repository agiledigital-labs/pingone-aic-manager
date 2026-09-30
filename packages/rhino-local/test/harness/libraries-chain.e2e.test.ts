import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "three-library chain",
  script: 'nodeState.putShared("value", require("r5ChainA").value); action.goTo("done");',
  outcomes: ["done"],
  libraries: {
    r5ChainA: 'exports.value = require("r5ChainB").value;',
    r5ChainB: 'exports.value = require("r5ChainC").value;',
    r5ChainC: 'exports.value = "from-c";',
  },
});

describe("three-library chain through defineSuite", () => {
  const lease = useLease(suite, aicWhenEnabled("r5-library-chain"));

  it("resolves A through B to C on each enabled lane", async () => {
    const run = await lease.run().expect({
      outcome: "done",
      sharedState: { added: { value: "from-c" } },
    });
    expect(run.verdict.summary).toBe("");
    expect(run.kase.given.libraries).toEqual(suite.spec.libraries);
  });
});
