import { spawn } from "node:child_process";
import { mkdtemp, open, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";

async function runChild(scenario: string, pool: string, isolate: boolean, extra: Record<string, string> = {}) {
  const directory = await mkdtemp(join(tmpdir(), "rhino-backstop-"));
  const report = join(directory, "report.json");
  const environment = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("VITEST") && !key.startsWith("VITE_")));
  try {
    const output = join(directory, "output.log");
    const log = await open(output, "w");
    let code: number | null;
    try {
      const child = spawn(process.execPath, [fileURLToPath(new URL("vitest.mjs", import.meta.resolve("vitest/package.json"))), "run", "--config", "test/fixtures/emulation-backstop/config.ts"], {
        cwd: process.cwd(), stdio: ["ignore", log.fd, log.fd],
        env: { ...environment, AIC_SCRIPT_TESTER_JVM: "host", NO_COLOR: "1", R5_CASE: scenario, R5_POOL: pool, R5_ISOLATE: String(isolate), R5_REPORT: report, ...extra },
      });
      const timer = setTimeout(() => child.kill("SIGKILL"), 60_000);
      try { code = await new Promise<number | null>((resolve, reject) => { child.once("error", reject); child.once("exit", resolve); }); }
      finally { clearTimeout(timer); }
    } finally { await log.close(); }
    const result = { code, output: await readFile(output, "utf8") };
    return { ...result, report: await readFile(report, "utf8") };
  } finally { await rm(directory, { recursive: true, force: true }); }
}

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
      expect(report.testResults[0]?.assertionResults[0]?.status).toBe(scenario === "skip" ? "skipped" : "passed");
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
it("R5 setup refuses missing run enforcement", async () => {
  const result = await runChild("tenant", "forks", true, { R5_NO_RUNNER: "true" });
  expect(result.code, result.output + result.report).toBe(1);
  expect(result.output + result.report).toContain("requires the vitest-runner");
}, 30_000);
