import { existsSync } from "node:fs";
import { BaseSequencer, type TestSpecification } from "vitest/node";
class Ordered extends BaseSequencer {
  override async sort(files: TestSpecification[]) { return files.sort((a, b) => a.moduleId.localeCompare(b.moduleId)); }
}
import { defineConfig } from "vitest/config";
export default defineConfig({ test: {
  include: process.env.R8_CANARY === "true" ? ["test/fixtures/emulation-backstop/canary-*.fixture.ts"] : process.env.R7_PRECOLLECT ? ["test/fixtures/emulation-backstop/precollect.fixture.ts"] : process.env.R6_QUEUE_DELAY === "true" ? ["test/fixtures/emulation-backstop/queued.fixture.ts"] : process.env.R5_CASE === "late" ? ["test/fixtures/emulation-backstop/late-*.fixture.ts"] : ["test/fixtures/emulation-backstop/" + (["import", "unowned-outstanding", "fails", "retry", "skip"].includes(process.env.R5_CASE ?? "") ? "lifecycle" : "cause") + ".fixture.ts"],
  sequence: { sequencer: Ordered, ...(process.env.R8_CANARY === "true" ? { setupFiles: "list" as const } : {}) },
  setupFiles: process.env.R7_PRECOLLECT === "plain" ? [] : [...(process.env.R8_CANARY === "true" ? ["test/fixtures/emulation-backstop/canary-setup.ts"] : []), process.env.R6_LEGACY_SETUP === "true" ? "test/fixtures/emulation-backstop/legacy-setup.ts" : "src/harness/vitest-setup.ts", ...(process.env.R6_QUEUE_DELAY === "true" ? ["test/fixtures/emulation-backstop/client-setup.ts"] : [])],
  pool: process.env.R5_POOL === "threads" ? "threads" : "forks",
  isolate: process.env.R5_ISOLATE !== "false",
  maxWorkers: 1,
  ...(process.env.R8_CANARY === "true" ? { fileParallelism: false } : {}),
  maxConcurrency: 1,
  ...(process.env.R7_PRECOLLECT ? { hookTimeout: 500 } : {}),
  ...(process.env.R6_BROKEN_RUNNER === "true" ? { hookTimeout: 500 } : {}),
  reporters: [...(process.env.R6_QUEUE_DELAY === "true" || process.env.R8_CANARY === "true" ? ["test/fixtures/emulation-backstop/queued-reporter.ts"] : []), "json", ...(existsSync("src/harness/vitest-reporter.ts") ? ["src/harness/vitest-reporter.ts"] : [])],
  outputFile: process.env.R5_REPORT!,
  ...(existsSync("src/harness/vitest-runner.ts") && process.env.R5_NO_RUNNER !== "true" ? { runner: process.env.R8_CANARY === "true" ? "test/fixtures/emulation-backstop/canary-runner.ts" : process.env.R7_PRECOLLECT === "ours" ? "test/fixtures/emulation-backstop/precollect-runner.ts" : process.env.R7_PRECOLLECT === "plain" ? "test/fixtures/emulation-backstop/precollect-plain-runner.ts" : process.env.R6_BROKEN_RUNNER === "true" ? "test/fixtures/emulation-backstop/broken-runner.ts" : process.env.R5_REPORTER_ONLY === "true" ? "test/fixtures/emulation-backstop/reporter-only-runner.ts" : "src/harness/vitest-runner.ts" } : {}),
} });
