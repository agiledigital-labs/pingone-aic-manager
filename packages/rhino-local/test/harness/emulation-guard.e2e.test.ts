import { execFile } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { expect, it } from "vitest";

it("R3 #4 real afterEach guards reject generic REST, runner and lease error assertions", async () => {
  const childEnv = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("VITEST") && !key.startsWith("VITE_")));
  const directory = await mkdtemp(join(tmpdir(), "rhino-verdict-guard-"));
  const report = join(directory, "report.json");
  try {
    const result = await promisify(execFile)(process.execPath, [fileURLToPath(new URL("vitest.mjs", import.meta.resolve("vitest/package.json"))), "run", "--reporter=json", "--reporter=src/harness/vitest-reporter.ts", `--outputFile=${report}`, "--config", "test/fixtures/emulation-guard/config.ts"], {
      cwd: process.cwd(), timeout: 60_000, env: { ...childEnv, AIC_SCRIPT_TESTER_JVM: "host", NO_COLOR: "1" },
    }).then(() => 0, (error: { code: number }) => error.code);
    const output = await readFile(report, "utf8");
    expect(result, output).toBe(1);
    const summary = JSON.parse(output) as { numFailedTests: number; numPassedTests: number };
    expect(summary).toMatchObject({ numFailedTests: 3, numPassedTests: 1 });
    expect(output).toContain("Emulation failure guard");
    expect(output).toContain("require is not emulated yet");
    expect(output).toContain("Rhino harness timeout");
  } finally { await rm(directory, { recursive: true, force: true }); }
}, 65_000);
