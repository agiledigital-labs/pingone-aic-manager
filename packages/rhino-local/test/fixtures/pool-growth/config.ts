import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["test/fixtures/pool-growth/*.fixture.ts"],
    pool: process.env.POOL_GROWTH_KIND === "threads" ? "threads" : "forks",
    isolate: false,
    maxWorkers: 4,
    runner: "src/harness/vitest-runner.ts",
    reporters: ["json", "src/harness/vitest-reporter.ts"],
    outputFile: process.env.POOL_GROWTH_REPORT!,
    setupFiles: ["src/harness/vitest-setup.ts"],
  },
});
