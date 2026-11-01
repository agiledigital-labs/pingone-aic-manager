import { existsSync } from "node:fs";
import { BaseSequencer, type TestSpecification } from "vitest/node";
class Ordered extends BaseSequencer {
  override async sort(files: TestSpecification[]) { return files.sort((a, b) => a.moduleId.localeCompare(b.moduleId)); }
}
import { defineConfig } from "vitest/config";
export default defineConfig({ test: {
  include: process.env.R5_CASE === "late" ? ["test/fixtures/emulation-backstop/late-*.fixture.ts"] : ["test/fixtures/emulation-backstop/" + (["import", "unowned-outstanding", "fails", "retry", "skip"].includes(process.env.R5_CASE ?? "") ? "lifecycle" : "cause") + ".fixture.ts"],
  sequence: { sequencer: Ordered },
  setupFiles: [process.env.R6_LEGACY_SETUP === "true" ? "test/fixtures/emulation-backstop/legacy-setup.ts" : "src/harness/vitest-setup.ts"],
  pool: process.env.R5_POOL === "threads" ? "threads" : "forks",
  isolate: process.env.R5_ISOLATE !== "false",
  maxWorkers: 1,
  reporters: ["json", ...(existsSync("src/harness/vitest-reporter.ts") ? ["src/harness/vitest-reporter.ts"] : [])],
  outputFile: process.env.R5_REPORT!,
  ...(existsSync("src/harness/vitest-runner.ts") && process.env.R5_NO_RUNNER !== "true" ? { runner: process.env.R5_REPORTER_ONLY === "true" ? "test/fixtures/emulation-backstop/reporter-only-runner.ts" : "src/harness/vitest-runner.ts" } : {}),
} });
