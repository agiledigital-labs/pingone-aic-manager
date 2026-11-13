import { expect, it } from "vitest";
import { runChild } from "./emulation-child.ts";

const modes = ["forks", "threads"].flatMap((pool) => [true, false].map((isolate) => ({ pool, isolate })));
it.each(modes.flatMap((mode) => ["missing-second", "broad", "substring", "all", "tenant"].map((scenario) => ({ ...mode, scenario }))))(
  "R5 cause coverage $scenario in $pool isolate=$isolate", async ({ scenario, pool, isolate }) => {
    const result = await runChild(scenario, pool, isolate);
    expect(result.code, result.output + result.report).toBe(scenario === "all" || scenario === "tenant" ? 0 : 1);
    if (scenario === "missing-second" || scenario === "broad") expect(result.output + result.report).toContain("spawned thread outlived its job");
    if (scenario === "substring") expect(result.output + result.report).toContain("prefix-require-suffix");
  }, 30_000,
);

it.each(modes.flatMap((mode) => ["import", "unowned-outstanding", "fails", "retry", "skip", ...(mode.isolate ? [] : ["late"])].map((scenario) => ({ ...mode, scenario }))))(
  "R5 run backstop $scenario in $pool isolate=$isolate", async ({ scenario, pool, isolate }) => {
    const result = await runChild(scenario, pool, isolate);
    expect(result.code, result.output + result.report).toBe(1);
    expect(result.output + result.report).toContain(scenario === "unowned-outstanding" ? "outstanding work" : `${scenario} missing`);
    const report = JSON.parse(result.report) as { testResults: Array<{ status: string; assertionResults: Array<{ title: string; status: string }> }> };
    if (scenario === "import" || scenario === "unowned-outstanding") {
      expect(report.testResults[0]?.status).toBe("failed");
      expect(report.testResults[0]?.assertionResults[0]?.status).toBe("passed");
    }
    if (scenario === "fails" || scenario === "retry" || scenario === "skip") {
      expect(report.testResults[0]?.assertionResults[0]?.status).toBe(scenario === "skip" ? "skipped" : "failed");
    }
    if (scenario === "late") {
      expect(result.output + result.report).toContain("original late owner");
      expect(report.testResults.flatMap((file) => file.assertionResults).find((test) => test.title === "clean later file")?.status).toBe("passed");
    }
  }, 30_000,
);

it.each(modes)("R5 main reporter enforces discarded worker verdict in $pool isolate=$isolate", async ({ pool, isolate }) => {
  const result = await runChild("fails", pool, isolate, { R5_REPORTER_ONLY: "true" });
  expect(result.code, result.output + result.report).toBe(1);
  expect(result.output).toContain("Emulation run backstop: expected-fail cannot invert emulation");
  expect(result.output).toContain("fails missing");
  const report = JSON.parse(result.report) as { testResults: Array<{ assertionResults: Array<{ status: string }> }> };
  expect(report.testResults[0]?.assertionResults[0]?.status).toBe("passed");
}, 30_000);
// The worker's run backstop is deferred to Vitest's final flush. When no flush
// send carries it the worker's throw is lost, and the run must still fail with
// the message. The message is the main reporter's, from its own audit copy; the
// exit code is not shown to depend on it (E19 review: it survived with the
// reporter removed), so this pins the outcome, not which part produces it.
it.each(modes)("R9 deferred backstop survives a dropped flush in $pool isolate=$isolate", async ({ pool, isolate }) => {
  const result = await runChild("fails", pool, isolate, { R9_FLUSH_DROPPED: "true" });
  expect(result.code, result.output + result.report).toBe(1);
  expect(result.output).toContain("Emulation run backstop: expected-fail cannot invert emulation");
}, 30_000);
it("R5 setup refuses missing run enforcement", async () => {
  const result = await runChild("tenant", "forks", true, { R5_NO_RUNNER: "true" });
  expect(result.code, result.output + result.report).toBe(1);
  expect(result.output + result.report).toContain("requires the vitest-runner");
}, 30_000);
