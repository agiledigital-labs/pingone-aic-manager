import { afterAll, describe, expect, it } from "vitest";
import { defineSuite, useLease } from "../../../src/harness/index.ts";

const outer = useLease(defineSuite({
  name: "outer lease",
  script: 'action.goTo("done")',
  outcomes: ["done"],
}));

describe("inner lease", () => {
  const inner = useLease(defineSuite({
    name: "inner lease",
    script: "for (;;) {}",
    outcomes: ["done"],
  }), { timeoutMs: 0 });

  it("opens the inner lease", () => {});

  afterAll(() => {
    // This hook runs before the lease's teardown. Its unfinished job must fail
    // the run, but the file-level lease still owns the same JVM.
    void inner.run().expect({ outcome: "done" }).catch(() => undefined);
  });
});

it("the outer lease can run after the inner teardown", async () => {
  const { verdict } = await outer.run().expect({ outcome: "done" });
  expect(verdict.pass, verdict.summary).toBe(true);
});
