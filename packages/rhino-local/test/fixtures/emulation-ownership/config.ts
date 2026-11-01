import { existsSync } from "node:fs";
import { defineConfig } from "vitest/config";
export default defineConfig({ test: {
  include: ["test/fixtures/emulation-ownership/verdict.fixture.ts"],
  // Allows the original implementation's false passes to be measured first.
  setupFiles: existsSync("src/harness/vitest-setup.ts") ? ["src/harness/vitest-setup.ts"] : [],
} });
