/**
 * Suite-level benchmark: run bench/cases under Vitest and report total
 * wall-clock, per-case time and how many JVMs were started.
 *
 *     node bench/run.ts [--workers=4]
 *
 * JVMs are counted from outside, by test/harness/runner-jvms.ts: the process
 * tree under the Vitest process is sampled every 25 ms and every distinct
 * `java … Runner` pid is recorded.
 */
import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { watchRunnerJvms } from "../test/harness/runner-jvms.ts";

const workersArg = process.argv.find((a) => a.startsWith("--workers="));
const workers = workersArg === undefined ? undefined : Number(workersArg.slice("--workers=".length));

const dir = mkdtempSync(join(tmpdir(), "rhino-bench-"));
const out = join(dir, "cases.jsonl");
const root = new URL("..", import.meta.url).pathname;

const args = ["vitest", "run", "--config", "bench/vitest.config.ts"];
if (workers !== undefined) args.push(`--maxWorkers=${workers}`);
const started = performance.now();
const child = spawn("npx", args, {
  cwd: root,
  env: { ...process.env, BENCH_OUT: out },
  stdio: ["ignore", "inherit", "inherit"],
});
const census = watchRunnerJvms(child.pid as number);
const code: number = await new Promise((resolve) => child.on("exit", (c) => resolve(c ?? 1)));
const javas = census.stop();
const wallMs = performance.now() - started;

const rows = readFileSync(out, "utf8")
  .trim()
  .split("\n")
  .map((l) => JSON.parse(l) as { file: string; variant: string; i: number; ms: number });
rmSync(dir, { recursive: true, force: true });

function stats(xs: number[]): string {
  const s = [...xs].sort((a, b) => a - b);
  const q = (p: number): number => s[Math.min(s.length - 1, Math.floor(p * s.length))] ?? 0;
  return `n=${s.length} median=${q(0.5).toFixed(1)}ms p90=${q(0.9).toFixed(1)}ms max=${(s.at(-1) ?? 0).toFixed(1)}ms`;
}

const report = {
  exit: code,
  wallMs: Math.round(wallMs),
  jvms: javas.size,
  workers: workers ?? "default",
};
console.log(`\nbench: ${JSON.stringify(report)}`);
for (const variant of ["uses-common", "no-require"]) {
  const mine = rows.filter((r) => r.variant === variant);
  console.log(`  ${variant} all:        ${stats(mine.map((r) => r.ms))}`);
  console.log(`  ${variant} after first: ${stats(mine.filter((r) => r.i > 0).map((r) => r.ms))}`);
}
process.exit(code);
