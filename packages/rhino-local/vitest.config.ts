import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    runner: "src/harness/vitest-runner.ts",
    reporters: ["default", "src/harness/vitest-reporter.ts"],
    setupFiles: ["src/harness/vitest-setup.ts"],
    include: ["test/**/*.test.ts"],
  },
});
