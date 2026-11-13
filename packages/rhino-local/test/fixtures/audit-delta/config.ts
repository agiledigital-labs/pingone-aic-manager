import { defineConfig } from "vitest/config";

// Several files over two fork workers, through the real runner, setup and
// reporter, with faults in most files, a worker that dies mid-file and a
// test that times out with its case still running. Run twice by
// test/harness/audit-delta.e2e.test.ts: once with the delta transport, once
// with AIC_SCRIPT_TESTER_AUDIT_SNAPSHOTS=1, and the two tallies compared.
export default defineConfig({
  test: {
    include: ["test/fixtures/audit-delta/*.fixture.ts"],
    pool: "forks",
    isolate: process.env.AUDIT_DELTA_ISOLATE !== "false",
    maxWorkers: 2,
    sequence: { shuffle: false },
    runner: "src/harness/vitest-runner.ts",
    reporters: ["json", "src/harness/vitest-reporter.ts"],
    outputFile: process.env.AUDIT_DELTA_REPORT!,
    setupFiles: ["src/harness/vitest-setup.ts"],
  },
});
