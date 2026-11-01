import { expect, it } from "vitest";
import { runChild } from "./emulation-child.ts";
const modes = ["forks", "threads"].flatMap((pool) => [true, false].map((isolate) => ({ pool, isolate })));
const native = ["--configLoader", "native", "--no-cache"];
it.each(modes)("R6 #2 awaited readiness with 300ms queued reporter in $pool isolate=$isolate", async ({ pool, isolate }) => {
  const result = await runChild("tenant", pool, isolate, { R6_QUEUE_DELAY: "true" }, native);
  expect(result.code, result.output + result.report).toBe(0);
  const report = JSON.parse(result.report) as { numPassedTests: number };
  expect(report.numPassedTests).toBe(2);
}, 30_000);

it("R6 #2 missing collection super hook gets a bounded readiness error", async () => {
  const result = await runChild("tenant", "forks", true, { R6_BROKEN_RUNNER: "true" }, native);
  expect(result.code, result.output + result.report).toBe(1);
  expect(result.output + result.report).toContain("Emulation runner collection readiness timed out after 500ms");
}, 30_000);


it.each(modes.flatMap((mode) => ["ours", "plain"].flatMap((runner) => ["setup", "collect"].map((source) => ({ ...mode, runner, source }))))) (
  "R7 precollection $source import under $runner runner in $pool isolate=$isolate", async ({ pool, isolate, runner, source }) => {
    const result = await runChild("tenant", pool, isolate, { R7_PRECOLLECT: runner, R7_IMPORT_SOURCE: source }, native);
    expect(result.code, result.output + result.report).toBe(0);
    const report = JSON.parse(result.report) as { numPassedTests: number };
    expect(report.numPassedTests).toBe(1);
  }, 30_000,
);
