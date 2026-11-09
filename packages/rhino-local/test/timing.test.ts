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
    expect(formatTimingSummary(s)).toMatch(/^jobs 2 \(1 shipped source text, 0 with an emulation fault\), spawns 1, borrows 2, publishes 1\n/);
  });

  it("tallies faulted jobs, library self time and per-source time; --share hashes every name", () => {
    const text = [
      job(10, { sourceName: "secret-script.js", emulationFault: true, faultIds: ["x"], jvm: { totalMs: 8, moduleBodies: { "client-lib": 3, other: 1 } } }),
      job(10, { sourceName: "secret-script.js", emulationFault: false, jvm: { totalMs: 4, moduleBodies: { "client-lib": 2 } } }),
      job(10, { sourceName: "plain.js", jvm: { totalMs: 1 } }),
    ].join("\n");
    const s = summarizeTiming(text);
    expect(s.faultedJobs).toBe(1);
    expect(s.libraries.map((r) => [r.field, r.n, r.sum])).toEqual([["client-lib", 2, 5], ["other", 1, 1]]);
    expect(s.sources.map((r) => [r.field, r.n, r.sum])).toEqual([["secret-script.js", 2, 12], ["plain.js", 1, 1]]);
    // The nested map is not a numeric field.
    expect(s.fields.some((r) => r.field.startsWith("jvm.moduleBodies"))).toBe(false);
    expect(formatTimingSummary(s)).toMatch(/1 with an emulation fault/);

    const shared = summarizeTiming(text, { share: {} });
    const out = `${JSON.stringify(shared)}\n${formatTimingSummary(shared)}`;
    for (const name of ["secret-script", "client-lib", "other", "plain"]) expect(out).not.toContain(name);
    expect(shared.libraries.map((r) => [r.n, r.sum])).toEqual([[2, 5], [1, 1]]);
    expect(shared.libraries[0]?.field).toMatch(/^library-[0-9a-f]{12}$/);
    expect(shared.sources[0]?.field).toMatch(/^source-[0-9a-f]{12}$/);
    // A random salt by default: a guessable name cannot be looked up across summaries.
    expect(summarizeTiming(text, { share: {} }).libraries[0]?.field).not.toBe(shared.libraries[0]?.field);
    // A given salt makes two summaries comparable.
    expect(summarizeTiming(text, { share: { salt: "s" } }).libraries[0]?.field)
      .toBe(summarizeTiming(text, { share: { salt: "s" } }).libraries[0]?.field);
  });
});
