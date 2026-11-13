import { defineConfig } from "vitest/config";
export default defineConfig({ test: {
  runner: "src/harness/vitest-runner.ts",
  reporters: ["default", "src/harness/vitest-reporter.ts"], include: ["test/fixtures/emulation-guard/verdict.fixture.ts"] } });
