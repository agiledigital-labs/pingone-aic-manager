import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "mutual library require",
  script: 'nodeState.putShared("value", require("r5CycleA").fromB); action.goTo("done");',
  outcomes: ["done"],
  libraries: {
    r5CycleA: 'exports.name = "a"; exports.fromB = require("r5CycleB").sawA;',
    r5CycleB: 'exports.sawA = require("r5CycleA").name;',
  },
});

describe("mutual library require", () => {
  const lease = useLease(suite, aicWhenEnabled("r5-library-cycle"));

  it("reveals whether AM sees the partially built A exports as local Rhino does", async () => {
    // AM's cycle semantics are unmeasured; the live lane establishes whether
    // it agrees with this local result or reports a conformance failure.
    const run = await lease.run().expect({
      outcome: "done",
      sharedState: { added: { value: "a" } },
    });
    expect(run.verdict.summary).toBe("");
  });
});
