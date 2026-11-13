import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    runner: "src/harness/vitest-runner.ts",
    reporters: ["default", "src/harness/vitest-reporter.ts"],
    setupFiles: ["src/harness/vitest-setup.ts"],
    include: ["test/**/*.test.ts"],
    // Each worker drives a JVM per lane, two under AIC_SCRIPT_TESTER_JVM=both,
    // so vitest's default of one worker per core oversubscribes the machine:
    // on 22 cores the wall-clock tests (journey nodes, timing) timed out in
    // three of five full runs, and passed in two of two at half the cores,
    // which was also faster (82-84 s against 90 s).
    maxWorkers: "50%",
  },
});
