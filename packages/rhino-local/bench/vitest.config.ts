import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    root: new URL("..", import.meta.url).pathname,
    runner: "src/harness/vitest-runner.ts",
    reporters: ["default", "src/harness/vitest-reporter.ts"],
    setupFiles: ["src/harness/vitest-setup.ts"],
    include: ["bench/cases/**/*.bench.test.ts"],
    // A consuming suite's settings: one worker reuses its modules, and the
    // worker count is chosen here rather than by the package default.
    isolate: false,
    maxWorkers: 4,
    fileParallelism: true,
    // A journey can span two evals. Let the runner's own per-job deadline
    // give the verdict when cold compilation is slow on a contended CPU.
    testTimeout: 30_000,
  },
});
