import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";
import { summarizeTiming } from "../../src/timing.ts";

it.each(["forks", "threads"])("overlapping leases keep four warm JVMs in %s", async (kind) => {
  const dir = await mkdtemp(join(tmpdir(), "rhino-pool-growth-"));
  try {
    const report = join(dir, "report.json");
    const timing = join(dir, "timing.jsonl");
    const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("VITEST") && !key.startsWith("VITE_")));
    delete env.AIC_SCRIPT_TESTER_JVM_POOL;
    const vitest = fileURLToPath(new URL("vitest.mjs", import.meta.resolve("vitest/package.json")));
    const child = spawn(process.execPath, [vitest, "run", "--config", "test/fixtures/pool-growth/config.ts"], {
      cwd: process.cwd(),
      env: { ...env, AIC_SCRIPT_TESTER_JVM: "host", NO_COLOR: "1", POOL_GROWTH_KIND: kind,
        POOL_GROWTH_REPORT: report, AIC_SCRIPT_TESTER_TIMING: timing },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let output = "";
    child.stdout.on("data", (chunk: Buffer) => { output += chunk.toString(); });
    child.stderr.on("data", (chunk: Buffer) => { output += chunk.toString(); });
    const code = await new Promise<number | null>((resolve, reject) => {
      const timer = setTimeout(() => child.kill("SIGKILL"), 90_000);
      child.once("error", reject);
      child.once("exit", (code) => { clearTimeout(timer); resolve(code); });
    });
    expect(code, output).toBe(0);
    expect(JSON.parse(await readFile(report, "utf8"))).toMatchObject({ numPassedTests: 40 });
    const summary = summarizeTiming(await readFile(timing, "utf8"));
    expect(summary.spawns, output).toBe(4);
    expect(summary.borrows).toBe(8);
    expect(summary.jobs).toBe(48);
    // Each worker ships the prelude and library once across all its leases/files.
    expect(summary.fields.find((row) => row.field === "job.sourcesShipped")?.sum).toBe(8);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}, 120_000);
