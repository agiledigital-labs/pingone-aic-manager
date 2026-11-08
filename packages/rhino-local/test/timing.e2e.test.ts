/**
 * AIC_SCRIPT_TESTER_TIMING: rows reach the file with the phases the JVM ran,
 * the second identical case ships no source text and hits the compile cache,
 * and the response a test sees is the same with timing on as off.
 */
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../src/bindings/run.ts";
import { RhinoRunner } from "../src/runner.ts";
import { TIMING_ENV } from "../src/timing.ts";

const dir = mkdtempSync(join(tmpdir(), "rl-timing-"));
const kase = {
  name: "timed",
  outcomes: ["true"],
  script: 'var lib = require("lib-timed"); if (lib.value === 7) action.goTo("true");',
  given: { libraries: { "lib-timed": "exports.value = 7;" } },
  expect: { outcome: "true" },
};
const rows = (file: string): Array<Record<string, unknown>> =>
  readFileSync(file, "utf8").trim().split("\n").map((l) => JSON.parse(l) as Record<string, unknown>);

describe("phase timing", () => {
  let previous: string | undefined;
  beforeAll(() => { previous = process.env[TIMING_ENV]; });
  afterEach(() => {
    if (previous === undefined) delete process.env[TIMING_ENV];
    else process.env[TIMING_ENV] = previous;
  });
  afterAll(() => { rmSync(dir, { recursive: true, force: true }); });

  it("records a spawn row and per-job phases; the repeat case ships nothing and hits the cache", async () => {
    const file = join(dir, "on.jsonl");
    process.env[TIMING_ENV] = file;
    const runner = await RhinoRunner.spawn({ lane: "host" });
    try {
      await runCase(runner, kase);
      await runCase(runner, kase);
    } finally {
      await runner.close();
    }
    const [spawn, first, second, ...rest] = rows(file);
    expect(rest).toEqual([]);
    expect(spawn).toMatchObject({ type: "spawn", lane: "host", pooled: false });
    expect(spawn?.spawnToReadyMs).toBeGreaterThan(0);
    for (const job of [first, second]) {
      expect(job).toMatchObject({ type: "job", lane: "host", sourceName: "timed", engine: "am", outcome: "ok" });
      const jvm = job?.jvm as Record<string, number>;
      for (const phase of ["totalMs", "readerDecodeMs", "setupMs", "preludeCompileMs", "preludeExecMs", "preambleCompileMs",
        "preambleExecMs", "libraryCompileMs", "libraryLoaderExecMs", "subjectCompileMs", "subjectExecMs"])
        expect(jvm[phase], phase).toBeGreaterThanOrEqual(0);
      expect(jvm.libraryLoads).toBe(1);
      expect(job?.roundTripMs as number).toBeGreaterThanOrEqual(jvm.totalMs as number);
    }
    // First case: prelude and library cross the pipe and compile.
    expect(first).toMatchObject({ sourcesShipped: 2 });
    expect(first?.sourceBytes as number).toBeGreaterThan(100_000);
    expect(first?.jvm).toMatchObject({ cacheMisses: 2 });
    expect((first?.jvm as Record<string, unknown>).cacheHits).toBeUndefined();
    // Second: named by hash only, both compiled forms reused.
    expect(second).toMatchObject({ sourcesShipped: 0, sourceBytes: 0 });
    expect(second?.lineBytes as number).toBeLessThan((first?.lineBytes as number) - 100_000);
    expect(second?.jvm).toMatchObject({ cacheHits: 2 });
    expect((second?.jvm as Record<string, unknown>).cacheMisses).toBeUndefined();
  }, 60_000);

  it("leaves the response unchanged and writes nothing when off", async () => {
    const file = join(dir, "off.jsonl");
    delete process.env[TIMING_ENV];
    const runner = await RhinoRunner.spawn({ lane: "host" });
    let off, on;
    try {
      off = (await runCase(runner, kase)).response;
      process.env[TIMING_ENV] = join(dir, "compare.jsonl");
      on = (await runCase(runner, kase)).response;
    } finally {
      await runner.close();
    }
    expect(Object.keys(on)).not.toContain("timing");
    const { id: _a, ...offRest } = off;
    const { id: _b, ...onRest } = on;
    expect(onRest).toEqual(offRest);
    expect(() => readFileSync(file)).toThrow();
    expect(rows(join(dir, "compare.jsonl")).map((r) => r.type)).toEqual(["job"]);
  }, 60_000);
});
