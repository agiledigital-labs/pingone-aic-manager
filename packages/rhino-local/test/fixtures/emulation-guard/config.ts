import { defineConfig } from "vitest/config";
export default defineConfig({ test: { include: ["test/fixtures/emulation-guard/verdict.fixture.ts"] } });
