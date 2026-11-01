import { execFile } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { expect, it } from "vitest";

async function runChild(scenario: string, pool: string, isolate: boolean) {
  const directory = await mkdtemp(join(tmpdir(), "rhino-backstop-"));
  const report = join(directory, "report.json");
  const environment = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("VITEST") && !key.startsWith("VITE_")));
  try {
    const result = await promisify(execFile)(process.execPath, [fileURLToPath(new URL("vitest.mjs", import.meta.resolve("vitest/package.json"))), "run", "--config", "test/fixtures/emulation-backstop/config.ts"], {
      cwd: process.cwd(), timeout: 60_000, env: { ...environment, AIC_SCRIPT_TESTER_JVM: "host", NO_COLOR: "1", R5_CASE: scenario, R5_POOL: pool, R5_ISOLATE: String(isolate), R5_REPORT: report },
    }).then(({ stdout, stderr }) => ({ code: 0, output: stdout + stderr }), (error: { code: number; stdout: string; stderr: string }) => ({ code: error.code, output: error.stdout + error.stderr }));
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
