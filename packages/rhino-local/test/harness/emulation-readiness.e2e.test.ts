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


// Upgrade canary: Vitest 5.0.5 installs the current collector file synchronously
// before framework imports, even while onTestModuleQueued awaits 300 ms. Check
// each module's first evaluation against its OWN readiness, not a previous file.
it.each(["forks", "threads"])("R8 three-file readiness upgrade canary in %s", async (pool) => {
  const result = await runChild("tenant", pool, false, { R8_CANARY: "true" }, native);
  expect(result.code, result.output + result.report).toBe(0);
  const report = JSON.parse(result.report) as { numPassedTests: number; testResults: Array<{
    name: string; status: string; assertionResults: Array<{ status: string; meta: { readinessCanary?: {
      file: string; setupReadyFile: string; testReadyFile: string; readyFiles: string[];
      helperImports: number; helper: string; worker: string;
    } } }>;
  }> };
  expect(report.numPassedTests).toBe(3);
  expect(report.testResults).toHaveLength(3);
  const workers = new Set<string>();
  const readyFiles = new Set<string>();
  for (const file of report.testResults) {
    expect(file.status).toBe("passed");
    expect(file.assertionResults).toHaveLength(1);
    const assertion = file.assertionResults[0]!;
    expect(assertion.status).toBe("passed");
    const observed = assertion.meta.readinessCanary!;
    expect(observed.file).toBe(file.name);
    expect(observed.setupReadyFile).toBe(file.name);
    expect(observed.testReadyFile).toBe(file.name);
    expect(observed.helper).toBe("initialized");
    expect(observed.helperImports).toBeGreaterThan(0);
    workers.add(observed.worker);
    for (const ready of observed.readyFiles) readyFiles.add(ready);
  }
  expect(workers.size).toBe(1);
  expect([...readyFiles].sort()).toEqual(report.testResults.map((file) => file.name).sort());
}, 30_000);
