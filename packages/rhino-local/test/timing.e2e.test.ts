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
import type { Case, Given } from "../src/case/types.ts";
import { expectEmulationFailure, expectHarnessFault } from "../src/harness/emulation-guard.ts";

const dir = mkdtempSync(join(tmpdir(), "rl-timing-"));
const kase = {
  name: "timed",
  outcomes: ["true"],
  script: 'var lib = require("lib-timed"); if (lib.value === 7) action.goTo("true");',
  given: { libraries: { "lib-timed": "exports.value = 7;" } },
  expect: { outcome: "true" },
};
// The package's Vitest runner appends its own `publish` rows while a test has the variable set.
const rows = (file: string): Array<Record<string, unknown>> =>
  readFileSync(file, "utf8").trim().split("\n").map((l) => JSON.parse(l) as Record<string, unknown>)
    .filter((r) => r.type !== "publish");

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
    // This file runs under the package's Vitest runner, which publishes its audit at job start.
    const publishes = readFileSync(file, "utf8").trim().split("\n").map((l) => JSON.parse(l) as Record<string, unknown>)
      .filter((r) => r.type === "publish");
    expect(publishes.length).toBeGreaterThan(0);
    for (const p of publishes) for (const k of ["publishMs", "failures", "outstanding", "packs"]) expect(typeof p[k], k).toBe("number");
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
    // First case: prelude and library cross the pipe, and they and the subject compile.
    expect(first).toMatchObject({ sourcesShipped: 2 });
    expect(first?.sourceBytes as number).toBeGreaterThan(100_000);
    expect(first?.jvm).toMatchObject({ cacheMisses: 3, subjectCacheMisses: 1 });
    expect((first?.jvm as Record<string, unknown>).cacheHits).toBeUndefined();
    // Second: named by hash only, all three compiled forms reused.
    expect(second).toMatchObject({ sourcesShipped: 0, sourceBytes: 0 });
    expect(second?.lineBytes as number).toBeLessThan((first?.lineBytes as number) - 100_000);
    expect(second?.jvm).toMatchObject({ cacheHits: 3, subjectCacheHits: 1 });
    expect((second?.jvm as Record<string, unknown>).cacheMisses).toBeUndefined();
  }, 60_000);

  it.each([
    ["a throwing subject", { source: 'var t = Date.now(); while (Date.now() - t < 40) {} throw new Error("x");' }, "runtime_error", ["subjectCompileMs", "subjectExecMs"]],
    ["a timed-out subject", { source: "for (;;) {}", timeoutMs: 40 }, "timeout", ["subjectCompileMs", "subjectExecMs"]],
    ["a subject that does not compile", { source: "var = ;" }, "compile_error", ["subjectCompileMs"]],
    ["a throwing preamble", { preamble: 'var t = Date.now(); while (Date.now() - t < 40) {} throw new Error("p");', source: "1" }, "runtime_error", ["preambleCompileMs", "preambleExecMs"]],
  ])("records the phases of %s, not only of successful ones", async (_name, job, outcome, phases) => {
    const file = join(dir, `fail-${String(outcome)}-${phases.length}.jsonl`);
    rmSync(file, { force: true });
    process.env[TIMING_ENV] = file;
    const runner = await RhinoRunner.spawn({ lane: "host" });
    try {
      if (outcome === "timeout") expectEmulationFailure("Rhino harness timeout");
      const response = await runner.eval({ classAllowList: [], ...job });
      expect(response.outcome).toBe(outcome);
    } finally {
      await runner.close();
    }
    const jvm = rows(file).find((r) => r.type === "job")?.jvm as Record<string, number>;
    for (const phase of phases) expect(jvm[phase], phase).toBeGreaterThanOrEqual(0);
    // The 40 ms busy-wait lands in exec (a timeout counts from job start, so its exec share varies).
    if (outcome === "runtime_error") expect(jvm[phases[1] as string]).toBeGreaterThanOrEqual(30);
  }, 60_000);

  it("records a library's compile time when its syntax error is caught", async () => {
    const file = join(dir, "lib-syntax.jsonl");
    process.env[TIMING_ENV] = file;
    const runner = await RhinoRunner.spawn({ lane: "host" });
    try {
      const result = await runCase(runner, {
        name: "timed",
        outcomes: ["true"],
        script: 'try { require("lib-bad"); } catch (e) { action.goTo("true"); }',
        given: { libraries: { "lib-bad": "exports.value = ;" } },
        expect: { outcome: "true" },
      });
      expect(result.effects.outcome).toBe("true");
    } finally {
      await runner.close();
    }
    const jvm = rows(file).find((r) => r.type === "job")?.jvm as Record<string, number>;
    expect(jvm).toMatchObject({ libraryLoads: 1 });
    expect(jvm.libraryCompileMs).toBeGreaterThanOrEqual(0);
  }, 60_000);

  it("brackets module bodies (self time per library) and the harvest inside subject execution", async () => {
    const file = join(dir, "bodies.jsonl");
    process.env[TIMING_ENV] = file;
    const busy = (ms: number): string => `var t = Date.now(); while (Date.now() - t < ${ms}) {}`;
    const runner = await RhinoRunner.spawn({ lane: "host" });
    try {
      await runCase(runner, {
        name: "bodies",
        outcomes: ["true"],
        script: `var outer = require("lib-outer"); require("lib-outer"); ${busy(20)} if (outer.inner === 3) action.goTo("true");`,
        given: { libraries: {
          "lib-outer": `${busy(30)} exports.inner = require("lib-inner").value;`,
          "lib-inner": `${busy(40)} exports.value = 3;`,
        } },
        expect: { outcome: "true" },
      });
    } finally {
      await runner.close();
    }
    const job = rows(file).find((r) => r.type === "job");
    expect(job).toMatchObject({ outcome: "ok", emulationFault: false });
    expect(job?.faultIds).toBeUndefined();
    const jvm = job?.jvm as Record<string, number> & { moduleBodies: Record<string, number> };
    // Each body ran once (the second require is the cache); the inner nests in the outer.
    expect(Object.keys(jvm.moduleBodies)).toEqual(["lib-inner", "lib-outer"]);
    expect(jvm.moduleBodyRuns).toBe(2);
    expect(jvm.moduleBodies["lib-inner"]).toBeGreaterThanOrEqual(35);
    // Self time: the outer's 30 ms, not its 70 ms including the inner.
    expect(jvm.moduleBodies["lib-outer"]).toBeGreaterThanOrEqual(25);
    expect(jvm.moduleBodies["lib-outer"]).toBeLessThan(60);
    // Outermost only: the inner is not counted twice.
    expect(jvm.moduleBodyMs).toBeCloseTo((jvm.moduleBodies["lib-inner"] ?? 0) + (jvm.moduleBodies["lib-outer"] ?? 0), 1);
    expect(jvm.harvestRuns).toBe(1);
    expect(jvm.harvestMs).toBeGreaterThan(0);
    // What is left of subject execution is the subject's own 20 ms busy-wait.
    expect(jvm.subjectOwnMs).toBeGreaterThanOrEqual(15);
    expect(jvm.subjectOwnMs).toBeCloseTo((jvm.subjectExecMs ?? 0) - (jvm.moduleBodyMs ?? 0) - (jvm.harvestMs ?? 0), 1);
  }, 60_000);

  it("flags a run whose script caught a harness fault, though its outcome is ok", async () => {
    const file = join(dir, "fault.jsonl");
    process.env[TIMING_ENV] = file;
    const runner = await RhinoRunner.spawn({ lane: "host" });
    try {
      expectHarnessFault("openidm-failure-detail");
      const given: Given = { managed: { "managed/widget": [] },
        openidmFailures: [{ match: { method: "delete", resource: "managed/widget/w1", ordinal: 1 }, reply: { code: 404 } }] };
      await runCase(runner, {
        name: "faulted",
        outcomes: ["true"],
        script: 'try { openidm.delete("managed/widget/w1", null); } catch (e) { try { e.javaException.getDetail(); } catch (x) {} } action.goTo("true");',
        given,
        expect: { outcome: "true" },
      }).catch(() => undefined);
    } finally {
      await runner.close();
    }
    const job = rows(file).find((r) => r.type === "job");
    expect(job).toMatchObject({ outcome: "ok", emulationFault: true, faultIds: ["openidm-failure-detail"] });
  }, 60_000);

  it("gives an untimed job no timer: the runtime's bracket is null", async () => {
    delete process.env[TIMING_ENV];
    const runner = await RhinoRunner.spawn({ lane: "host" });
    try {
      const probe: Case = { name: "untimed", outcomes: ["true"], script: 'if (__rhinoLocalPhase === null && typeof __rhinoLocalTimer === "undefined") action.goTo("true");', given: {}, expect: { outcome: "true" } };
      expect((await runCase(runner, probe)).effects.outcome).toBe("true");
      process.env[TIMING_ENV] = join(dir, "timer.jsonl");
      // Timed, the bracket is captured, and the hook itself is still gone before the script runs.
      const timed = { ...probe, script: 'if (__rhinoLocalPhase !== null && typeof __rhinoLocalTimer === "undefined") action.goTo("true");' };
      expect((await runCase(runner, timed)).effects.outcome).toBe("true");
    } finally {
      await runner.close();
    }
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
