import { expect, it } from "vitest";
import { runChild } from "./emulation-child.ts";
const modes = ["forks", "threads"].flatMap((pool) => [true, false].map((isolate) => ({ pool, isolate })));
const native = ["--configLoader", "native", "--no-cache"];
const suppress = [...native, "--dangerouslyIgnoreUnhandledErrors"];
const replace = [...suppress, "--reporter=default"];

it.each(modes.flatMap((mode) => ["fails", "tenant"].map((scenario) => ({ ...mode, scenario }))))(
  "R6 #1 setup rejects replaced reporter for $scenario in $pool isolate=$isolate", async ({ scenario, pool, isolate }) => {
    const result = await runChild(scenario, pool, isolate, {}, replace);
    expect(result.code, result.output).toBe(1);
    expect(result.output).toContain("requires the vitest-reporter");
  }, 30_000,
);
it.each(modes.flatMap((mode) => ["fails", "retry", "skip"].map((scenario) => ({ ...mode, scenario }))))(
  "R6 #1 owned $scenario persists failed file without reporter in $pool isolate=$isolate", async ({ scenario, pool, isolate }) => {
    const result = await runChild(scenario, pool, isolate, { R6_LEGACY_SETUP: "true" }, [...replace, "--reporter=json"]);
    expect(result.code, result.output + result.report).toBe(1);
    expect(result.output + result.report).toContain(`${scenario} missing`);
    const report = JSON.parse(result.report) as { testResults: Array<{ status: string; assertionResults: Array<{ status: string }> }> };
    expect(report.testResults[0]?.status).toBe("failed");
    expect(report.testResults[0]?.assertionResults[0]?.status).toBe(scenario === "skip" ? "skipped" : "failed");
  }, 30_000,
);
it.each(modes.flatMap((mode) => ["fails", "tenant", "all"].map((scenario) => ({ ...mode, scenario }))))(
  "R6 #1 retained reporter control $scenario in $pool isolate=$isolate", async ({ scenario, pool, isolate }) => {
    const result = await runChild(scenario, pool, isolate, {}, [...suppress, "--passWithNoTests", "--bail=1"]);
    expect(result.code, result.output + result.report).toBe(scenario === "fails" ? 1 : 0);
    if (scenario === "fails") expect(result.output).toContain("fails missing");
  }, 30_000,
);
it.each(modes)("R6 #1 reporter rejects discarded worker verdict despite suppression in $pool isolate=$isolate", async ({ pool, isolate }) => {
  const result = await runChild("fails", pool, isolate, { R5_REPORTER_ONLY: "true" }, suppress);
  expect(result.code, result.output + result.report).toBe(1);
  expect(result.output).toContain("Emulation run backstop: expected-fail cannot invert emulation");
}, 30_000);

it.each(["forks", "threads"])("R6 #1 late owned failure persists the original file without reporter in %s", async (pool) => {
  const result = await runChild("late", pool, false, { R6_LEGACY_SETUP: "true" }, [...replace, "--reporter=json"]);
  expect(result.code, result.output + result.report).toBe(1);
  expect(result.output + result.report).toContain("original late owner");
  const report = JSON.parse(result.report) as { testResults: Array<{ name: string; status: string; assertionResults: Array<{ status: string }> }> };
  expect(report.testResults.find((file) => file.name.endsWith("late-a.fixture.ts"))?.status).toBe("failed");
  expect(report.testResults.find((file) => file.name.endsWith("late-a.fixture.ts"))?.assertionResults[0]?.status).toBe("failed");
  expect(report.testResults.find((file) => file.name.endsWith("late-b.fixture.ts"))?.status).toBe("passed");
}, 30_000);
