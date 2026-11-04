import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    root: new URL("..", import.meta.url).pathname,
    runner: "src/harness/vitest-runner.ts",
    reporters: ["default", "src/harness/vitest-reporter.ts"],
    setupFiles: ["src/harness/vitest-setup.ts"],
    include: ["bench/cases/**/*.bench.test.ts"],
  },
});
