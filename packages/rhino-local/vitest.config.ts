import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    setupFiles: ["src/harness/vitest-setup.ts"],
    include: ["test/**/*.test.ts"],
  },
});
