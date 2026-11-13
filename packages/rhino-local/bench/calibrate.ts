/**
 * Phase mix of one warm JVM running the decision-node workload.
 *
 *     AIC_SCRIPT_TESTER_TIMING=/tmp/bench-cal.jsonl node bench/calibrate.ts
 *
 * Prints each phase as a share of JVM time, next to the shape this workload
 * is aiming at. Deletes nothing; the timing file is the one named above.
 */
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runCase } from "../src/bindings/run.ts";
import type { Case } from "../src/case/types.ts";
import { RhinoRunner } from "../src/runner.ts";
import { readFileSync } from "node:fs";
import {
  benchLibraries,
  buildHeaders,
  buildParams,
  givenEsv,
  expectedKeyCount,
  expectedN,
  makeUsers,
  subjectScript,
} from "./workload.ts";

const ROUNDS = 20;
const WARMUP = 4;

const timing = process.env.AIC_SCRIPT_TESTER_TIMING
  ?? join(mkdtempSync(join(tmpdir(), "bench-cal-")), "timing.jsonl");
process.env.AIC_SCRIPT_TESTER_TIMING = timing;

function kase(shape: "full" | "bare" | "headers" | "store"): Case {
  const full = shape === "full";
  const given: Case["given"] = {
    libraries: full || shape === "bare" ? benchLibraries() : undefined,
    callbacks: [],
    esv: givenEsv(),
  };
  if (shape === "full" || shape === "store") given.managed = { "managed/alpha_thing": makeUsers() };
  if (shape === "full" || shape === "headers") {
    given.requestHeaders = buildHeaders();
    given.requestParameters = buildParams();
  }
  const script = shape === "full"
    ? subjectScript()
    : 'nodeState.putShared("n", 1);\nnodeState.putShared("keyCount", 1);\naction.goTo("done");';
  return {
    name: "bench-cal",
    script,
    outcomes: ["done"],
    given,
    expect: shape === "full"
      ? { outcome: "done", sharedState: { added: { n: expectedN(false), keyCount: expectedKeyCount("ordinary") } } }
      : { outcome: "done", sharedState: { added: { n: 1, keyCount: 1 } } },
  };
}

const shapes = ["bare", "headers", "store", "full"] as const;

const runner = await RhinoRunner.spawn();
try {
  for (let i = 0; i < WARMUP; i += 1) {
    const run = await runCase(runner, kase("full"), { sourceName: "bench/cal.js", attributionName: `warm-${i}` });
    if (run.verdict.summary !== "") {
      console.error(run.verdict.summary);
      process.exit(1);
    }
  }
  for (const shape of shapes) {
    for (let i = 0; i < ROUNDS; i += 1) {
      const run = await runCase(runner, kase(shape), { sourceName: `bench/${shape}.js`, attributionName: `${shape}-${i}` });
      if (run.verdict.summary !== "") {
        console.error(shape, run.verdict.summary);
        process.exit(1);
      }
    }
  }
} finally {
  await runner.close();
}

interface Job {
  sourceName?: string;
  jvm?: Record<string, number>;
  transportMs?: number;
  stringifyMs?: number;
  roundTripMs?: number;
  lineBytes?: number;
}

const rows = readFileSync(timing, "utf8").trim().split("\n").map((line) => JSON.parse(line) as Job & { type?: string });
const jobs = rows.filter((row) => row.type === "job" && row.jvm !== undefined);

function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)] ?? 0;
}

function num(job: Job, key: string): number {
  const value = job.jvm?.[key];
  return typeof value === "number" ? value : 0;
}

console.log(`timing ${timing}`);
const fields = ["totalMs", "subjectOwnMs", "subjectExecMs", "moduleBodyMs", "harvestMs", "preludeCompileMs", "preludeExecMs", "setupMs", "preambleCompileMs", "preambleExecMs", "subjectCompileMs", "libraryCompileMs", "readerDecodeMs", "hostWaitMs"];
for (const shape of ["warm", ...shapes]) {
  const mine = jobs.filter((job) => String(job.sourceName).startsWith(shape === "warm" ? "warm-" : `${shape}-`));
  if (mine.length === 0) continue;
  const total = median(mine.map((job) => num(job, "totalMs")));
  console.log(`\n${shape}  n=${mine.length}  median jvm ${total.toFixed(2)} ms  line ${median(mine.map((job) => job.lineBytes ?? 0)).toFixed(0)} B`);
  for (const field of fields) {
    const value = median(mine.map((job) => num(job, field)));
    const pct = total === 0 ? 0 : (100 * value) / total;
    console.log(`  ${field.padEnd(22)} ${value.toFixed(2).padStart(8)} ms  ${pct.toFixed(1).padStart(5)}%`);
  }
  const preludeScope = median(mine.map((job) => num(job, "preludeCompileMs") + num(job, "preludeExecMs") + num(job, "setupMs")));
  console.log(`  ${"prelude+scope".padEnd(22)} ${preludeScope.toFixed(2).padStart(8)} ms  ${(total === 0 ? 0 : (100 * preludeScope) / total).toFixed(1).padStart(5)}%  aim 17.4`);
  console.log(`  transport ${median(mine.map((job) => job.transportMs ?? 0)).toFixed(2)} ms  stringify ${median(mine.map((job) => job.stringifyMs ?? 0)).toFixed(2)} ms  roundTrip ${median(mine.map((job) => job.roundTripMs ?? 0)).toFixed(2)} ms`);
}
