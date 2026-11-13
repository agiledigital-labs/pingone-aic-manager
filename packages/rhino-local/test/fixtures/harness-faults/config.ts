import { defineConfig } from "vitest/config";

// Two files in one worker on the JVM pool, through the real runner, setup and
// reporter: the reporter's fault tally must count across the file boundary,
// and the case that runs after a faulting one on the same JVM must not
// inherit its fault. Driven by test/harness/harness-faults.e2e.test.ts.
export default defineConfig({
  test: {
    include: ["test/fixtures/harness-faults/*.fixture.ts"],
    pool: "forks",
    isolate: true,
    maxWorkers: 1,
    fileParallelism: false,
    sequence: { shuffle: false },
    runner: "src/harness/vitest-runner.ts",
    reporters: ["json", "src/harness/vitest-reporter.ts"],
    outputFile: process.env.HARNESS_FAULTS_REPORT!,
    setupFiles: ["src/harness/vitest-setup.ts"],
  },
});
