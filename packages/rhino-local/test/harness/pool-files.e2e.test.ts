/**
 * The JVM pool across real Vitest files. isolation.e2e.test.ts simulates a
 * file boundary with release and re-borrow; this runs two files in one
 * worker under `isolate: true`, so it also covers the wiring that simulation
 * skips: the reporter starting the pool in `onInit`, `provide`/`inject`
 * carrying its address, and `VITEST_POOL_ID` choosing the slot.
 *
 * The reuse control is a count of the runner JVMs the nested run started:
 * exactly one means both files' leases (and their pid borrows) ran on the
 * same pooled JVM. Equal pids alone do not show that — they come from a bare
 * `borrow`, which stays pooled even if `useLease` silently spawns.
 */
import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";
import { procAvailable, watchRunnerJvms } from "./runner-jvms.ts";

it("two files in one worker share one JVM and each starts from a clean scope", async () => {
  const dir = await mkdtemp(join(tmpdir(), "rhino-pool-files-"));
  try {
    const pids = join(dir, "pids");
    const report = join(dir, "report.json");
    const env = Object.fromEntries(
      Object.entries(process.env).filter(([key]) => !key.startsWith("VITEST") && !key.startsWith("VITE_"))
    );
    delete env.AIC_SCRIPT_TESTER_JVM_POOL;
    const vitest = fileURLToPath(new URL("vitest.mjs", import.meta.resolve("vitest/package.json")));
    const child = spawn(process.execPath, [vitest, "run", "--config", "test/fixtures/jvm-pool/config.ts"], {
      cwd: process.cwd(),
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...env, AIC_SCRIPT_TESTER_JVM: "host", NO_COLOR: "1", POOL_FILES_PIDS: pids, POOL_FILES_REPORT: report },
    });
    const census = watchRunnerJvms(child.pid as number);
    let output = "";
    child.stdout.on("data", (chunk: Buffer) => (output += chunk.toString()));
    child.stderr.on("data", (chunk: Buffer) => (output += chunk.toString()));
    const code = await new Promise<number | null>((resolve, reject) => {
      const timer = setTimeout(() => child.kill("SIGKILL"), 60_000);
      child.once("error", reject);
      child.once("exit", (exit) => {
        clearTimeout(timer);
        resolve(exit);
      });
    });
    const jvms = census.stop();
    expect(code, output).toBe(0);
    const results = JSON.parse(await readFile(report, "utf8")) as { numPassedTests: number; numTotalTests: number };
    // Both files' probes passed: each saw a clean scope, the second after the first polluted.
    expect(results).toMatchObject({ numPassedTests: 6, numTotalTests: 6 });
    const lines = (await readFile(pids, "utf8")).trim().split("\n").sort();
    expect(lines.map((line) => line.split(" ")[0])).toEqual(["a", "b"]);
    const [a, b] = lines.map((line) => Number(line.split(" ")[1]));
    // The positive control: the JVM really was reused across the file boundary.
    expect(a).toBeGreaterThan(0);
    expect(b).toBe(a);
    // The lease reuse itself: one runner JVM for the whole run, which both
    // leases and both pid borrows ran on. Linux only (it reads /proc).
    if (procAvailable) expect([...jvms]).toEqual([a]);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}, 90_000);
