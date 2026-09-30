import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "shared library dependency",
  script: [
    'var left = require("r5DiamondA").value;',
    'var right = require("r5DiamondB").value;',
    'nodeState.putShared("value", left + ":" + right);',
    'action.goTo("done");',
  ].join("\n"),
  outcomes: ["done"],
  libraries: {
    r5DiamondA: 'exports.value = require("r5DiamondC").value;',
    r5DiamondB: 'exports.value = require("r5DiamondC").value;',
    r5DiamondC: 'exports.value = "from-c";',
  },
});

describe("shared library dependency", () => {
  const lease = useLease(suite, aicWhenEnabled("r5-library-diamond"));

  it("resolves C through both A and B on each enabled lane", async () => {
    // AM's evaluation count for C is unmeasured. This live case checks only
    // the resolved values; the runtime counter test checks local caching.
    const run = await lease.run().expect({
      outcome: "done",
      sharedState: { added: { value: "from-c:from-c" } },
    });
    expect(run.verdict.summary).toBe("");
  });
});
