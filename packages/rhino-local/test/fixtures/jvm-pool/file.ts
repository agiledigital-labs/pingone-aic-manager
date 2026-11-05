import { appendFileSync } from "node:fs";
import { describe, expect, inject, it } from "vitest";
import { defineSuite, useLease } from "../../../src/harness/index.ts";
import { JVM_POOL_CONTEXT } from "../../../src/pool.ts";
import { RhinoRunner } from "../../../src/runner.ts";
import { FRESH, LIBRARIES, POLLUTER, PROBE } from "../../harness/isolation-scripts.ts";

/** Probe first, then pollute: whichever file runs second probes the scope the first one vandalised. */
const suite = defineSuite({
  name: "pool files",
  script: PROBE.replace('action.goTo("done");', "") + POLLUTER,
  outcomes: ["done"],
  libraries: LIBRARIES,
});

export function poolFile(name: string): void {
  describe(`${name} lease`, () => {
    const lease = useLease(suite);
    for (const round of [1, 2]) {
      it(`starts fresh (case ${round})`, async () => {
        const run = await lease.run().expect({ outcome: "done" });
        const probe = JSON.parse(String(run.effects.sharedState.final.probe)) as unknown;
        expect(probe).toEqual(FRESH);
      });
    }
  });

  // After the lease's afterAll returned its JVM: borrowing again in this file
  // gets the same one, and its pid identifies it to the outer test.
  describe(`${name} pid`, () => {
    it("records the JVM pid", async () => {
      const address = inject(JVM_POOL_CONTEXT);
      expect(address).toBeDefined();
      const runner = await RhinoRunner.borrow(address!);
      try {
        const response = await runner.eval({ source: "Number(java.lang.ProcessHandle.current().pid())" });
        appendFileSync(process.env.POOL_FILES_PIDS!, `${name} ${String(response.value)}\n`);
      } finally {
        await runner.close();
      }
    });
  });
}
