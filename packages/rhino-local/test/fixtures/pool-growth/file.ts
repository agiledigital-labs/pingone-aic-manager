import { afterAll, describe, expect, it } from "vitest";
import { defineSuite, managed, useLease } from "../../../src/harness/index.ts";

function checkLease(holder: string, spawnTimeoutMs?: number): () => Promise<void> {
  const lease = useLease(defineSuite({
    name: `pool lease ${holder}`,
    script: `
      var record = openidm.read("managed/alpha_widget/pool-record");
      nodeState.putShared("seenState", nodeState.get("holder"));
      nodeState.putShared("seenFixture", record.label);
      nodeState.putShared("seenLibrary", require("helper").value);
      action.goTo("done");
    `,
    outcomes: ["done"],
    always: { state: { shared: { holder } } },
    // Each live lease owns a different record at the same key.
    fixtures: { widget: managed("managed/alpha_widget", { _id: "pool-record", label: `${holder} fixture` }) },
    libraries: { helper: "exports.value = 42;" },
  }), spawnTimeoutMs === undefined ? {} : { spawnTimeoutMs });
  return async () => {
    const { verdict } = await lease.run().expect({
      outcome: "done",
      allowUndeclared: { openidmReads: true },
      sharedState: { added: { seenState: holder, seenFixture: `${holder} fixture`, seenLibrary: 42 } },
    });
    expect(verdict.pass, verdict.summary).toBe(true);
  };
}

// The outer lease lives across both inner suites. Sibling borrows alone do
// not reproduce the growth: the pool already reuses a returned JVM.
export function growthFile(): void {
  const checkOuter = checkLease("outer");
  it("uses the outer lease", checkOuter);
  for (const name of ["first", "second"]) {
    describe(name, () => {
      // The outer lease is already validated. Its default 60 s startup
      // budget must not prevent a nested lease with a 30 s budget sharing it.
      const checkInner = checkLease(name, 30_000);
      for (const round of [1, 2]) {
        it.concurrent(`runs inner ${round}`, checkInner);
      }
    });
  }
  // Nested teardown must release only its reference, leaving this lease usable.
  afterAll(checkOuter);
}
