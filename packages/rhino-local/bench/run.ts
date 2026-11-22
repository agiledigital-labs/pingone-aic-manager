/**
 * Suite-level benchmark: the decision-node files under bench/cases, with
 * phase timing.
 *
 *     node bench/run.ts [--workers=4]
 *
 * Prints wall-clock, JVM count, load average, and the phase sums from
 * AIC_SCRIPT_TESTER_TIMING. The timing file is kept when BENCH_TIMING is
 * set; otherwise it lives in a temporary directory and is removed after the
 * summary is printed.
 */
import { spawn } from "node:child_process";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { loadavg, tmpdir } from "node:os";
import { join } from "node:path";
import { buildIdentity, packageVersion } from "../src/paths.ts";
import { summarizeTiming } from "../src/timing.ts";
import { watchRunnerJvms } from "../test/harness/runner-jvms.ts";

const workersArg = process.argv.find((a) => a.startsWith("--workers="));
const workers = workersArg === undefined ? 4 : Number(workersArg.slice("--workers=".length));

function count(args: readonly string[]): number {
  try {
    const out = execFileSync("pgrep", [...args], { encoding: "utf8" });
    return out.split("\n").filter((line) => line.trim() !== "").length;
  } catch {
    return 0;
  }
}

function checkoutIdentity(): string | undefined {
  try {
    execFileSync("git", ["ls-files", "--error-unmatch", "--", "package.json"], {
      cwd: new URL("..", import.meta.url), stdio: "ignore",
    });
    const commit = execFileSync("git", ["rev-parse", "--short=12", "HEAD"], {
      cwd: new URL("..", import.meta.url), encoding: "utf8",
    }).trim();
    const dirty = execFileSync("git", ["status", "--porcelain", "--untracked-files=normal", "--", "."], {
      cwd: new URL("..", import.meta.url), encoding: "utf8",
    }).trim() !== "";
    return `${packageVersion} (${commit}${dirty ? "-dirty" : ""} checkout)`;
  } catch {
    return undefined;
  }
}

// os.loadavg() is portable; it reads zeros on Windows, where there is no load average.
const load = loadavg().map((n) => n.toFixed(2)).join(" ");
const quiet = {
  load,
  java: count(["-x", "java"]),
  cargo: count(["-x", "cargo"]),
  codex: count(["-af", "[c]odex exec"]),
};
console.log(`bench quiet: ${JSON.stringify(quiet)}`);

const keep = process.env.BENCH_TIMING;
const dir = keep === undefined || keep === "" ? mkdtempSync(join(tmpdir(), "rhino-bench-")) : undefined;
const timing = keep !== undefined && keep !== "" ? keep : join(dir as string, "timing.jsonl");
const root = new URL("..", import.meta.url).pathname;

const args = ["vitest", "run", "--config", "bench/vitest.config.ts", `--maxWorkers=${workers}`];
const env: NodeJS.ProcessEnv = { ...process.env, AIC_SCRIPT_TESTER_TIMING: timing };
delete env.CI;

const started = performance.now();
const child = spawn("npx", args, {
  cwd: root,
  env,
  stdio: ["ignore", "inherit", "inherit"],
});
const census = watchRunnerJvms(child.pid as number);
const code: number = await new Promise((resolve) => child.on("exit", (c) => resolve(c ?? 1)));
const javas = census.stop();
const wallMs = performance.now() - started;
const harnessIdentity = checkoutIdentity() ?? buildIdentity;

const summary = summarizeTiming(readFileSync(timing, "utf8"));
const sum = (field: string): number => summary.fields.find((row) => row.field === field)?.sum ?? 0;
const jvm = sum("jvm.totalMs");
const line = (label: string, value: number): void => {
  const pct = jvm === 0 ? 0 : (100 * value) / jvm;
  console.log(`  ${label.padEnd(22)} ${value.toFixed(1).padStart(9)} ms  ${pct.toFixed(1).padStart(5)}% of jvm`);
};

console.log(`\nbench: ${JSON.stringify({
  harness: harnessIdentity,
  exit: code,
  wallMs: Math.round(wallMs),
  jvms: javas === undefined ? "unavailable (no /proc)" : javas.size,
  workers,
  jobs: summary.jobs,
  faultedJobs: summary.faultedJobs,
  spawns: summary.spawns,
  borrows: summary.borrows,
  publishes: summary.publishes,
  timing,
})}`);
line("jvm total", jvm);
line("subject own", sum("jvm.subjectOwnMs"));
line("subject exec", sum("jvm.subjectExecMs"));
line("module body", sum("jvm.moduleBodyMs"));
line("harvest", sum("jvm.harvestMs"));
line("prelude", sum("jvm.preludeCompileMs") + sum("jvm.preludeExecMs"));
line("scope setup", sum("jvm.setupMs"));
line("prelude+scope", sum("jvm.preludeCompileMs") + sum("jvm.preludeExecMs") + sum("jvm.setupMs"));
line("seed exec", sum("jvm.preambleExecMs"));
line("seed compile", sum("jvm.preambleCompileMs"));
line("subject compile", sum("jvm.subjectCompileMs"));
line("library compile", sum("jvm.libraryCompileMs"));
line("reader decode", sum("jvm.readerDecodeMs"));
line("host wait", sum("jvm.hostWaitMs"));
line("transport", sum("job.transportMs"));
line("stringify", sum("job.stringifyMs"));
line("round trip", sum("job.roundTripMs"));
console.log(`  ${"line bytes".padEnd(22)} ${sum("job.lineBytes").toFixed(0).padStart(9)} bytes`);
line("spawn to ready", sum("spawn.spawnToReadyMs"));
line("audit publish", sum("publish.publishMs"));
console.log(`  cache hits ${sum("jvm.cacheHits")}  misses ${sum("jvm.cacheMisses")}  subject hits ${sum("jvm.subjectCacheHits")} misses ${sum("jvm.subjectCacheMisses")}`);

if (dir !== undefined) rmSync(dir, { recursive: true, force: true });
process.exit(code);
