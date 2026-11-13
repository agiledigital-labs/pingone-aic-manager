/**
 * The reporter turns a failed case-validation into a tally row. A test that
 * catches the same throw stays green and adds nothing.
 */
import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";
import type { FaultTally } from "../../src/harness/fault-tally.ts";
import { CASE_VALIDATION_FAULTS } from "../../src/case/validation-fault.ts";

it("a refused case is a tally row and a caught refusal is not", async () => {
  const dir = await mkdtemp(join(tmpdir(), "rl-validation-tally-"));
  try {
    const report = join(dir, "report.json");
    const tallyPath = join(dir, "tally.json");
    const env = Object.fromEntries(
      Object.entries(process.env).filter(([key]) => !key.startsWith("VITEST") && !key.startsWith("VITE_"))
    );
    const vitest = fileURLToPath(new URL("vitest.mjs", import.meta.resolve("vitest/package.json")));
    const child = spawn(process.execPath, [vitest, "run", "--config", "test/fixtures/validation-tally/config.ts"], {
      cwd: process.cwd(),
      stdio: ["ignore", "pipe", "pipe"],
      env: {
        ...env,
        AIC_SCRIPT_TESTER_JVM_POOL: "0",
        NO_COLOR: "1",
        VALIDATION_TALLY_REPORT: report,
        AIC_SCRIPT_TESTER_FAULT_TALLY: tallyPath,
      },
    });
    let output = "";
    child.stdout.on("data", (chunk: Buffer) => { output += chunk.toString(); });
    child.stderr.on("data", (chunk: Buffer) => { output += chunk.toString(); });
    const code = await new Promise<number | null>((resolve, reject) => {
      const timer = setTimeout(() => child.kill("SIGKILL"), 60_000);
      child.once("error", reject);
      child.once("exit", (exit) => {
        clearTimeout(timer);
        resolve(exit);
      });
    });
    expect(code, output).toBe(1);
    const tally = JSON.parse(await readFile(tallyPath, "utf8")) as FaultTally;
    expect(tally.faults.map((row) => [row.id, row.cases, row.failed, row.message])).toEqual([
      ["case-invalid/engine-legacy", 1, 1, CASE_VALIDATION_FAULTS["case-invalid/engine-legacy"]],
      ["case-invalid/unknown-key", 1, 1, CASE_VALIDATION_FAULTS["case-invalid/unknown-key"]],
    ]);
    expect(JSON.stringify(tally)).not.toContain("widget");
    expect(output).toContain("case-invalid/engine-legacy");
    expect(output).toContain("case-invalid/unknown-key");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}, 60_000);
