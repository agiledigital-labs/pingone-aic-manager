import { afterAll, describe, it } from "vitest";
import { defineSuite, useLease } from "../../../src/harness/index.ts";

const suite = defineSuite({
  name: "overlapping leases",
  script: 'nodeState.putShared("v", require("helper").value); action.goTo("done");',
  outcomes: ["done"],
  libraries: { helper: "exports.value = 42;" },
});

// The outer lease lives across both inner suites. Sibling borrows alone do
// not reproduce the growth: the pool already reuses a returned JVM.
export function growthFile(): void {
  const outer = useLease(suite);
  it("uses the outer lease", async () => {
    await outer.run().expect({ outcome: "done", sharedState: { added: { v: 42 } } });
  });
  for (const name of ["first", "second"]) {
    describe(name, () => {
      const inner = useLease(suite);
      for (const round of [1, 2]) {
        it.concurrent(`runs inner ${round}`, async () => {
          await inner.run().expect({ outcome: "done", sharedState: { added: { v: 42 } } });
        });
      }
    });
  }
  // Nested teardown must release only its reference, leaving this lease usable.
  afterAll(async () => {
    await outer.run().expect({ outcome: "done", sharedState: { added: { v: 42 } } });
  });
}
