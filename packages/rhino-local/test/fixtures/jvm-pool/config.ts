import { defineConfig } from "vitest/config";

// Two symmetric files in one worker: the second must get the first's JVM, through
// the real reporter → provide → inject → VITEST_POOL_ID path (isolate: true).
export default defineConfig({
  test: {
    include: ["test/fixtures/jvm-pool/*.fixture.ts"],
    pool: "forks",
    isolate: true,
    maxWorkers: 1,
    fileParallelism: false,
    runner: "src/harness/vitest-runner.ts",
    reporters: ["json", "src/harness/vitest-reporter.ts"],
    outputFile: process.env.POOL_FILES_REPORT!,
    setupFiles: ["src/harness/vitest-setup.ts"],
  },
});
