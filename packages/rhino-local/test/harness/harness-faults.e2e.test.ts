/**
 * E19 through the real stack: a nested Vitest run of two fixture files in one
 * worker on the JVM pool (test/fixtures/harness-faults), with the package's
 * runner, setup and reporter, and the fault tally written where
 * AIC_SCRIPT_TESTER_FAULT_TALLY says.
 *
 * The discriminating pair, in each file:
 * - a script that catches a true refusal (a JS Date in an httpClient.send
 *   body) and then succeeds must FAIL, naming the fault id;
 * - a script that catches AIC's own error for a colliding create, which the
 *   harness emulates, must PASS.
 * A clean case then runs on the same pooled JVM and must pass, with nothing
 * in the tally: a fault must not leak into another case's tally.
 *
 * The tally must count across the file boundary: send-body-js-date is hit by
 * one case in each file (twice in file a's, which still counts once), and
 * state-put-undefined by file b's case only.
 */
import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";
import type { FaultTally } from "../../src/harness/fault-tally.ts";
import { procAvailable, watchRunnerJvms } from "./runner-jvms.ts";

interface JsonReport {
  numTotalTests: number;
  numPassedTests: number;
  numFailedTests: number;
  testResults: Array<{ name: string; assertionResults: Array<{ fullName: string; status: string; failureMessages: string[] }> }>;
}

it("a caught true refusal fails its case, a caught emulated error passes, and the tally counts each across files", async () => {
  const dir = await mkdtemp(join(tmpdir(), "rhino-harness-faults-"));
  try {
    const report = join(dir, "report.json");
    const tallyPath = join(dir, "nested", "tally.json");
    const env = Object.fromEntries(
      Object.entries(process.env).filter(([key]) => !key.startsWith("VITEST") && !key.startsWith("VITE_"))
    );
    delete env.AIC_SCRIPT_TESTER_JVM_POOL;
    const vitest = fileURLToPath(new URL("vitest.mjs", import.meta.resolve("vitest/package.json")));
    const child = spawn(process.execPath, [vitest, "run", "--config", "test/fixtures/harness-faults/config.ts"], {
      cwd: process.cwd(),
      stdio: ["ignore", "pipe", "pipe"],
      env: {
        ...env,
        AIC_SCRIPT_TESTER_JVM: "host",
        NO_COLOR: "1",
        HARNESS_FAULTS_REPORT: report,
        AIC_SCRIPT_TESTER_FAULT_TALLY: tallyPath,
      },
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
    expect(code, output).toBe(1);

    const results = JSON.parse(await readFile(report, "utf8")) as JsonReport;
    // Every case reported: none left "pending" by the run's emulation backstop.
    expect(results, output).toMatchObject({ numTotalTests: 6, numFailedTests: 2, numPassedTests: 4 });
    const status = Object.fromEntries(
      results.testResults.flatMap((file) => file.assertionResults.map((test) => [test.fullName, test]))
    );
    for (const name of ["a", "b"]) {
      const refusal = status[`${name} refusal caught refusal then success`];
      expect(refusal?.status).toBe("failed");
      expect(refusal?.failureMessages.join("\n")).toMatch(
        /rhino-local harness fault send-body-js-date: httpClient\.send body\.d is a JS Date[\s\S]*not emulated: this fails the run even if the script caught it/
      );
      expect(status[`${name} collide caught emulated error`]?.status).toBe("passed");
      expect(status[`${name} clean clean after a fault`]?.status).toBe("passed");
    }

    const tally = JSON.parse(await readFile(tallyPath, "utf8")) as FaultTally;
    const a = "test/fixtures/harness-faults/a.fixture.ts";
    const b = "test/fixtures/harness-faults/b.fixture.ts";
    expect(tally).toEqual({
      version: 1,
      faults: [
        { id: "send-body-js-date", cases: 2, failed: 2, files: [a, b], message: expect.stringContaining("httpClient.send body.d is a JS Date") as unknown },
        { id: "state-put-undefined", cases: 1, failed: 1, files: [b], message: "rhino-local: nodeState.putShared: value is undefined" },
      ],
      unidentified: [],
    });
    expect(output).toContain("rhino-local harness faults");
    expect(output).toMatch(/ {5}2 cases {2}send-body-js-date {2}\(2 files\)/);
    expect(output).toMatch(/ {5}1 case {3}state-put-undefined {2}\(1 file\)/);
    expect(output).toContain(`tally: ${tallyPath}`);

    // The pool was on: both files' leases ran on one runner JVM. Linux only.
    if (procAvailable) expect(jvms.size, output).toBe(1);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}, 90_000);
