import { pathToFileURL } from "node:url";
import { RhinoRunner } from "./runner.ts";

const JOBS = 200;
const STARTUPS = 3;
const TRIVIAL_SOURCE = "1 + 1";

function nowNs(): bigint {
  return process.hrtime.bigint();
}

function nsToMs(ns: bigint): number {
  return Number(ns) / 1_000_000;
}

function percentile(sorted: number[], p: number): number {
  if (sorted.length === 0) {
    return 0;
  }
  const idx = Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1));
  return sorted[idx] ?? 0;
}

function summary(samples: number[]): {
  n: number;
  min: number;
  median: number;
  mean: number;
  p95: number;
  max: number;
} {
  const sorted = [...samples].sort((a, b) => a - b);
  const sum = samples.reduce((acc, n) => acc + n, 0);
  return {
    n: samples.length,
    min: sorted[0] ?? 0,
    median: percentile(sorted, 50),
    mean: samples.length === 0 ? 0 : sum / samples.length,
    p95: percentile(sorted, 95),
    max: sorted[sorted.length - 1] ?? 0,
  };
}

function fmt(n: number): string {
  return n >= 10 ? n.toFixed(1) : n.toFixed(3);
}

export async function measure(): Promise<void> {
  const startups: number[] = [];
  let jobSamples: number[] = [];
  let firstJobMs = 0;

  for (let i = 0; i < STARTUPS; i++) {
    const t0 = nowNs();
    const runner = await RhinoRunner.spawn();
    const startupMs = nsToMs(nowNs() - t0);
    startups.push(startupMs);
    try {
      if (i !== 0) {
        continue;
      }
      const firstStart = nowNs();
      const first = await runner.eval({
        source: TRIVIAL_SOURCE,
        sourceName: "trivial.js",
        timeoutMs: 2_000,
      });
      firstJobMs = nsToMs(nowNs() - firstStart);
      if (first.outcome !== "ok") {
        throw new Error(`warmup job failed: ${JSON.stringify(first)}`);
      }
      jobSamples = [];
      for (let j = 0; j < JOBS; j++) {
        const jobStart = nowNs();
        const response = await runner.eval({
          id: `job-${j}`,
          source: TRIVIAL_SOURCE,
          sourceName: "trivial.js",
          timeoutMs: 2_000,
        });
        jobSamples.push(nsToMs(nowNs() - jobStart));
        if (response.outcome !== "ok") {
          throw new Error(`job ${j} failed: ${JSON.stringify(response)}`);
        }
      }
    } finally {
      await runner.close();
    }
  }

  const start = summary(startups);
  const jobs = summary(jobSamples);
  const lines = [
    `rhino-local runner timings (${new Date().toISOString()})`,
    `startup (spawn until ready), n=${start.n}: min=${fmt(start.min)} median=${fmt(start.median)} mean=${fmt(start.mean)} p95=${fmt(start.p95)} max=${fmt(start.max)} ms`,
    `first job after ready: ${fmt(firstJobMs)} ms`,
    `trivial job (1+1), n=${jobs.n}: min=${fmt(jobs.min)} median=${fmt(jobs.median)} mean=${fmt(jobs.mean)} p95=${fmt(jobs.p95)} max=${fmt(jobs.max)} ms`,
  ];
  process.stdout.write(`${lines.join("\n")}\n`);
}

const entry = process.argv[1];
if (entry && import.meta.url === pathToFileURL(entry).href) {
  measure().catch((error: unknown) => {
    process.stderr.write(`${error instanceof Error ? error.stack ?? error.message : String(error)}\n`);
    process.exit(1);
  });
}
