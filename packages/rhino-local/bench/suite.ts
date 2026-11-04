import { appendFileSync } from "node:fs";
import { describe, it } from "vitest";
import { defineSuite, useLease } from "../src/harness/index.ts";
import { benchLibraries, COMMON_SUM, NO_REQUIRE, USES_COMMON } from "./fixtures.ts";

/** Cases per variant per file. */
export const CASES_PER_VARIANT = 6;

const libraries = benchLibraries();

/**
 * One benchmark file: a `useLease` per variant, like a client file holding one
 * suite, each run CASES_PER_VARIANT times. Every case's wall time from
 * `run()` to verdict is appended to `$BENCH_OUT` as JSONL.
 */
export function benchFile(file: string): void {
  for (const [variant, script, expected] of [
    ["uses-common", USES_COMMON, COMMON_SUM],
    ["no-require", NO_REQUIRE, 1],
  ] as const) {
    describe(`${file} ${variant}`, () => {
      const suite = defineSuite({ name: `bench ${variant}`, script, outcomes: ["done"], libraries });
      const lease = useLease(suite);
      for (let i = 0; i < CASES_PER_VARIANT; i += 1) {
        it(`case ${i}`, async () => {
          const started = performance.now();
          await lease.run().expect({ outcome: "done", sharedState: { added: { n: expected } } });
          const ms = performance.now() - started;
          const out = process.env.BENCH_OUT;
          if (out !== undefined && out !== "") {
            appendFileSync(out, `${JSON.stringify({ file, variant, i, ms, pid: process.pid })}\n`);
          }
        });
      }
    });
  }
}
