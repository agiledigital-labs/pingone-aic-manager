import { describe, expect, it } from "vitest";
import { defineSuite, useLease } from "../../../src/harness/index.ts";
import { expectHarnessFault } from "../../../src/harness/emulation-guard.ts";

// Invented fixture: example.com only.
export const SEND_DATE =
  'httpClient.send("https://service.example.com/x", { method: "POST", body: { d: new Date(0) } }).get();';
export const PUT_UNDEFINED = 'nodeState.putShared("u", undefined);';

/**
 * One describe per case: `script` runs inside a try, so the refusal is caught
 * and the case then succeeds — which fails the test unless it declared the
 * fault with `declare`.
 */
export function faultCase(name: string, script: string, options: { declare?: string } = {}): void {
  const suite = defineSuite({ name, script: `try { ${script} } catch (e) {}\naction.goTo("done");`, outcomes: ["done"] });
  describe(name, () => {
    const lease = useLease(suite);
    it("runs", async () => {
      const run = lease.run().expect({ outcome: "done" });
      if (options.declare === undefined) return run;
      // A declared fault still fails the run call; the test owns it and passes.
      expectHarnessFault(options.declare);
      await expect(run).rejects.toThrow(options.declare);
    });
  });
}

export function cleanCase(name: string): void {
  const suite = defineSuite({ name, script: 'action.goTo("done");', outcomes: ["done"] });
  describe(name, () => {
    const lease = useLease(suite);
    it("runs", async () => {
      await lease.run().expect({ outcome: "done" });
    });
  });
}
