import { existsSync } from "node:fs";
import { defineConfig } from "vitest/config";
export default defineConfig({ test: {
  include: ["test/fixtures/emulation-backstop/cause.fixture.ts"],
  setupFiles: ["src/harness/vitest-setup.ts"],
  pool: process.env.R5_POOL === "threads" ? "threads" : "forks",
  isolate: process.env.R5_ISOLATE !== "false",
  maxWorkers: 1,
  reporters: ["json", ...(existsSync("src/harness/vitest-reporter.ts") ? ["src/harness/vitest-reporter.ts"] : [])],
  outputFile: process.env.R5_REPORT!,
  ...(existsSync("src/harness/vitest-runner.ts") ? { runner: "src/harness/vitest-runner.ts" } : {}),
} });
