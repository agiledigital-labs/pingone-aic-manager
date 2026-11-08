/**
 * Opt-in phase timing. Set `AIC_SCRIPT_TESTER_TIMING` to a file path and every
 * JVM spawn, lease and eval job appends one JSON line to it; summarise the file
 * with `aic-script-tester-timing <file>`. Off by default: when the variable is
 * unset no job asks the JVM for timing and nothing is written.
 *
 * Rows (`type`):
 * - `spawn`: a JVM process from start to its ready line (`spawnToReadyMs`);
 *   `pooled` says whether the shared pool started it.
 * - `borrow`: a lease connecting to a pooled JVM until it is usable
 *   (`readyMs`). Borrows outnumbering pooled spawns is the pool reusing JVMs.
 * - `job`: one eval on one lane. Node fields: `lineBytes` (the job line as
 *   written), `sourceBytes` and `sourcesShipped` (prelude and library texts
 *   carried on that line rather than named by hash), `stringifyMs`,
 *   `roundTripMs` (write until response), `transportMs` (round trip less the
 *   JVM's own total). JVM fields are under `jvm`: `totalMs`, `readerDecodeMs`,
 *   `setupMs`, `preludeCompileMs`/`preludeExecMs`,
 *   `preambleCompileMs`/`preambleExecMs` (the seed is parsed here),
 *   `libraryCompileMs`/`libraryLoaderExecMs`/`libraryLoads`,
 *   `subjectCompileMs`/`subjectExecMs`, `harvestMs`, `hostWaitMs`/`hostCalls`,
 *   `cacheHits`/`cacheMisses` (compiled-script cache). Exec phases include any
 *   library module bodies and host-call waits that ran inside them. A phase
 *   that started is recorded even when it threw or timed out; one that did
 *   not happen is absent. `harvestMs` covers journey-bridge jobs only: an
 *   ordinary case appends its harvest call to the subject, so that cost is
 *   inside `subjectExecMs`.
 * - `publish`: the emulation runner sending a worker's failure audit to the
 *   main process (on every job start and end, and at file boundaries):
 *   `publishMs`, and the audit's size as `failures`, `outstanding` and
 *   `packs` (task results re-sent with it). Only under the package's Vitest
 *   runner.
 *
 * Every worker appends to the same file; each line is one `appendFileSync`,
 * which Linux and macOS keep whole for lines this size.
 */
import { appendFileSync } from "node:fs";

export const TIMING_ENV = "AIC_SCRIPT_TESTER_TIMING";

let warned = false;

/** The file rows go to, or `undefined` when timing is off. Read per call, so a test can set it. */
export function timingPath(): string | undefined {
  const path = process.env[TIMING_ENV];
  return path === undefined || path === "" ? undefined : path;
}

export function recordTiming(row: { type: "spawn" | "borrow" | "job" | "publish" } & Record<string, unknown>): void {
  const path = timingPath();
  if (path === undefined) return;
  try {
    appendFileSync(path, `${JSON.stringify({ ...row, at: Date.now(), pid: process.pid })}\n`);
  } catch (error) {
    if (!warned) {
      warned = true;
      process.stderr.write(`rhino-local: cannot write ${TIMING_ENV} file ${path}: ${String(error)}\n`);
    }
  }
}

/** Milliseconds since a `performance.now()` reading, to three places. */
export function since(start: number): number {
  return Math.round((performance.now() - start) * 1000) / 1000;
}

export interface TimingSummaryRow {
  field: string;
  n: number;
  sum: number;
  p50: number;
  p90: number;
  max: number;
}

export interface TimingSummary {
  jobs: number;
  spawns: number;
  borrows: number;
  /** Audit publishes by the emulation runner (`publish` rows). */
  publishes: number;
  /** Jobs whose line carried at least one source text. */
  jobsShippingSources: number;
  /** Per numeric field, over the rows that have it (`jvm.*` for JVM phases). */
  fields: TimingSummaryRow[];
}

function percentile(sorted: number[], p: number): number {
  if (sorted.length === 0) return 0;
  return sorted[Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)] as number;
}

/** Summarises the rows of a timing file (one JSON object per line). */
export function summarizeTiming(text: string): TimingSummary {
  const values = new Map<string, number[]>();
  const add = (field: string, value: unknown): void => {
    if (typeof value !== "number" || !Number.isFinite(value)) return;
    const list = values.get(field) ?? [];
    list.push(value);
    values.set(field, list);
  };
  let jobs = 0, spawns = 0, borrows = 0, publishes = 0, jobsShippingSources = 0;
  for (const line of text.split("\n")) {
    if (line.trim() === "") continue;
    const row = JSON.parse(line) as Record<string, unknown>;
    const prefix = `${String(row.type)}.`;
    if (row.type === "spawn") spawns++;
    if (row.type === "borrow") borrows++;
    if (row.type === "publish") publishes++;
    if (row.type === "job") { jobs++; if (typeof row.sourcesShipped === "number" && row.sourcesShipped > 0) jobsShippingSources++; }
    for (const [key, value] of Object.entries(row)) {
      if (key === "at" || key === "pid") continue;
      if (key === "jvm" && value !== null && typeof value === "object") {
        for (const [k, v] of Object.entries(value)) add(`jvm.${k}`, v);
      } else add(prefix + key, value);
    }
  }
  const fields = [...values.entries()]
    .map(([field, list]) => {
      const sorted = [...list].sort((a, b) => a - b);
      const sum = Math.round(list.reduce((a, b) => a + b, 0) * 1000) / 1000;
      return { field, n: list.length, sum, p50: percentile(sorted, 50), p90: percentile(sorted, 90), max: sorted[sorted.length - 1] as number };
    })
    .sort((a, b) => a.field.localeCompare(b.field));
  return { jobs, spawns, borrows, publishes, jobsShippingSources, fields };
}

export function formatTimingSummary(summary: TimingSummary): string {
  const lines = [
    `jobs ${summary.jobs} (${summary.jobsShippingSources} shipped source text), spawns ${summary.spawns}, borrows ${summary.borrows}, publishes ${summary.publishes}`,
    "",
    ["field".padEnd(32), "n".padStart(7), "sum".padStart(12), "p50".padStart(10), "p90".padStart(10), "max".padStart(10)].join(" "),
  ];
  const num = (v: number, w: number): string => (Number.isInteger(v) ? String(v) : v.toFixed(3)).padStart(w);
  for (const r of summary.fields)
    lines.push([r.field.padEnd(32), num(r.n, 7), num(r.sum, 12), num(r.p50, 10), num(r.p90, 10), num(r.max, 10)].join(" "));
  return `${lines.join("\n")}\n`;
}
