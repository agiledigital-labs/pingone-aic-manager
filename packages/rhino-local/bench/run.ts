/**
 * Suite-level benchmark: run bench/cases under Vitest and report total
 * wall-clock, per-case time and how many JVMs were started.
 *
 *     node bench/run.ts [--workers=4]
 *
 * JVMs are counted from outside: the process tree under the Vitest process is
 * sampled every 25 ms and every distinct `java … Runner` pid is recorded. A runner JVM
 * lives far longer than one sample, so none is missed.
 */
import { spawn } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const workersArg = process.argv.find((a) => a.startsWith("--workers="));
const workers = workersArg === undefined ? undefined : Number(workersArg.slice("--workers=".length));

const dir = mkdtempSync(join(tmpdir(), "rhino-bench-"));
const out = join(dir, "cases.jsonl");
const root = new URL("..", import.meta.url).pathname;

function children(): Map<number, { ppid: number; comm: string }> {
  const table = new Map<number, { ppid: number; comm: string }>();
  for (const entry of readdirSync("/proc")) {
    if (!/^\d+$/.test(entry)) continue;
    try {
      const stat = readFileSync(`/proc/${entry}/stat`, "utf8");
      const close = stat.lastIndexOf(")");
      const comm = stat.slice(stat.indexOf("(") + 1, close);
      const ppid = Number(stat.slice(close + 2).split(" ")[1]);
      table.set(Number(entry), { ppid, comm });
    } catch {
      // exited between readdir and read
    }
  }
  return table;
}

function descendantJavas(rootPid: number, seen: Set<number>): void {
  const table = children();
  const isDescendant = (pid: number): boolean => {
    for (let p = pid, hops = 0; p > 1 && hops < 64; hops += 1) {
      if (p === rootPid) return true;
      const row = table.get(p);
      if (row === undefined) return false;
      p = row.ppid;
    }
    return false;
  };
  for (const [pid, row] of table) {
    if (!row.comm.includes("java") || seen.has(pid) || !isDescendant(pid)) continue;
    try {
      // The runner itself, not javac or a `java -version` probe.
      const argv = readFileSync(`/proc/${pid}/cmdline`, "utf8").split("\0");
      if (argv.includes("Runner")) seen.add(pid);
    } catch {
      // exited
    }
  }
}

const args = ["vitest", "run", "--config", "bench/vitest.config.ts"];
if (workers !== undefined) args.push(`--maxWorkers=${workers}`);
const started = performance.now();
const child = spawn("npx", args, {
  cwd: root,
  env: { ...process.env, BENCH_OUT: out },
  stdio: ["ignore", "inherit", "inherit"],
});
const javas = new Set<number>();
const timer = setInterval(() => {
  if (child.pid !== undefined) descendantJavas(child.pid, javas);
}, 25);
const code: number = await new Promise((resolve) => child.on("exit", (c) => resolve(c ?? 1)));
clearInterval(timer);
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
