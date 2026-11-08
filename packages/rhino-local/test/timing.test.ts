import { describe, expect, it } from "vitest";
import { formatTimingSummary, summarizeTiming } from "../src/timing.ts";

const job = (roundTripMs: number, extra: Record<string, unknown> = {}) =>
  JSON.stringify({ type: "job", lane: "host", id: "x", roundTripMs, sourcesShipped: 0, at: 1, pid: 2, ...extra });

describe("summarizeTiming", () => {
  it("nearest-rank percentiles, sums, and JVM fields under jvm.", () => {
    // 1..10: nearest rank gives p50 = 5 and p90 = 9 (interpolation would give 5.5 and 9.1).
    const lines = Array.from({ length: 10 }, (_, i) => job(i + 1, { jvm: { totalMs: (i + 1) / 2, cacheHits: 2 } }));
    const s = summarizeTiming(`${lines.join("\n")}\n`);
    const row = (f: string) => s.fields.find((r) => r.field === f);
    expect(row("job.roundTripMs")).toEqual({ field: "job.roundTripMs", n: 10, sum: 55, p50: 5, p90: 9, max: 10 });
    expect(row("jvm.totalMs")).toMatchObject({ n: 10, sum: 27.5, p50: 2.5, max: 5 });
    expect(row("jvm.cacheHits")).toMatchObject({ sum: 20 });
    expect(row("job.at")).toBeUndefined();
    expect(row("job.pid")).toBeUndefined();
  });

  it("counts row types and jobs that shipped source", () => {
    const text = [
      JSON.stringify({ type: "spawn", lane: "host", pooled: true, spawnToReadyMs: 400 }),
      JSON.stringify({ type: "borrow", lane: "host", readyMs: 3 }),
      JSON.stringify({ type: "borrow", lane: "host", readyMs: 2 }),
      job(10, { sourcesShipped: 2 }),
      job(4),
      JSON.stringify({ type: "publish", publishMs: 0.5, failures: 0, outstanding: 1, packs: 2 }),
    ].join("\n");
    const s = summarizeTiming(text);
    expect({ jobs: s.jobs, spawns: s.spawns, borrows: s.borrows, publishes: s.publishes, shipped: s.jobsShippingSources })
      .toEqual({ jobs: 2, spawns: 1, borrows: 2, publishes: 1, shipped: 1 });
    expect(s.fields.find((r) => r.field === "publish.publishMs")).toMatchObject({ n: 1, sum: 0.5 });
    expect(s.fields.find((r) => r.field === "spawn.spawnToReadyMs")).toMatchObject({ n: 1, sum: 400 });
    expect(formatTimingSummary(s)).toMatch(/^jobs 2 \(1 shipped source text\), spawns 1, borrows 2, publishes 1\n/);
  });
});
