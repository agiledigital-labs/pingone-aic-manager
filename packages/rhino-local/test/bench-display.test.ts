import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";

test("benchmark driver prints transport bytes as a size", () => {
  const dir = mkdtempSync(join(tmpdir(), "rhino-bench-display-"));
  try {
    const bin = join(dir, "bin");
    mkdirSync(bin);
    // Replace only the workload process; exercise the real benchmark driver
    // and timing summary with deterministic transport and time measurements.
    writeFileSync(join(bin, "npx"), `#!/usr/bin/env node
const fs = require("node:fs");
fs.writeFileSync(process.env.AIC_SCRIPT_TESTER_TIMING, JSON.stringify({
  type: "job", lineBytes: 9180474, jvm: { totalMs: 100 }
}) + "\\n");
`, { mode: 0o755 });
    const run = spawnSync(process.execPath, [new URL("../bench/run.ts", import.meta.url).pathname], {
      env: { ...process.env, PATH: `${bin}:${process.env.PATH ?? ""}`, BENCH_TIMING: join(dir, "timing.jsonl") },
      encoding: "utf8",
    });
    expect(run.status, run.stderr).toBe(0);
    const line = run.stdout.split("\n").find((row) => row.trimStart().startsWith("line bytes"));
    expect(line).toMatch(/line bytes\s+9180474 bytes$/);
    expect(line).not.toMatch(/ms|%/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
