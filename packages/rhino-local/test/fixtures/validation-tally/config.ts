import { defineConfig } from "vitest/config";

// One file, no JVM: a failed validation must become a tally row, and a test
// that catches the same refusal must not. Driven by
// test/harness/validation-tally.e2e.test.ts.
export default defineConfig({
  test: {
    include: ["test/fixtures/validation-tally/*.fixture.ts"],
    pool: "forks",
    isolate: true,
    maxWorkers: 1,
    fileParallelism: false,
    runner: "src/harness/vitest-runner.ts",
    reporters: ["json", "src/harness/vitest-reporter.ts"],
    outputFile: process.env.VALIDATION_TALLY_REPORT!,
    setupFiles: ["src/harness/vitest-setup.ts"],
  },
});
