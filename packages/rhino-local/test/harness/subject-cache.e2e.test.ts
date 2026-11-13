/**
 * The subject script's compiled form is cached like the prelude's and the
 * libraries' (java/ScriptCache.java): identical jobs compile it once, any
 * change to what the compiler sees misses, and a cached script still runs
 * into a fresh scope. Read through the timing rows' subjectCache* counts.
 */
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import type { JobRequest } from "../../src/protocol.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { TIMING_ENV } from "../../src/timing.ts";

const dir = mkdtempSync(join(tmpdir(), "rl-subject-cache-"));

describe("subject compile cache", () => {
  let previous: string | undefined;
  let runner: RhinoRunner;
  let file: string;
  let n = 0;
  beforeAll(async () => {
    previous = process.env[TIMING_ENV];
    runner = await RhinoRunner.spawn({ lane: "host" });
  }, 60_000);
  afterAll(async () => {
    await runner?.close();
    if (previous === undefined) delete process.env[TIMING_ENV];
    else process.env[TIMING_ENV] = previous;
    rmSync(dir, { recursive: true, force: true });
  }, 30_000);

  /** Runs the jobs in order with timing on; returns each job's subject hit (true) or miss (false). */
  async function hits(jobs: JobRequest[]): Promise<boolean[]> {
    file = join(dir, `t${++n}.jsonl`);
    process.env[TIMING_ENV] = file;
    try {
      for (const job of jobs) expect((await runner.eval({ classAllowList: [], ...job })).outcome).toBe("ok");
    } finally {
      delete process.env[TIMING_ENV];
    }
    return readFileSync(file, "utf8").trim().split("\n").map((l) => JSON.parse(l) as Record<string, unknown>)
      .filter((r) => r.type === "job")
      .map((r) => {
        const jvm = r.jvm as Record<string, number | undefined>;
        expect((jvm.subjectCacheHits ?? 0) + (jvm.subjectCacheMisses ?? 0)).toBe(1);
        return jvm.subjectCacheHits === 1;
      });
  }

  it("compiles 25 identical jobs once", async () => {
    const job = { source: "var unique25 = 1; unique25 + 1", sourceName: "same" };
    expect(await hits(Array.from({ length: 25 }, () => job))).toEqual([false, ...Array.from({ length: 24 }, () => true)]);
  });

  it.each<[string, JobRequest]>([
    ["the source", { source: "var k = 2; k", sourceName: "base" }],
    ["the source name", { source: "var k = 1; k", sourceName: "other" }],
    ["the language version", { source: "var k = 1; k", sourceName: "base", languageVersion: 170 }],
  ])("misses when %s changes", async (_what, changed) => {
    const base: JobRequest = { source: "var k = 1; k", sourceName: "base" };
    // A fresh base per case, so each change is judged against a warm entry.
    const tag = `/*${n}*/`;
    const warm = { ...base, source: tag + String(base.source) };
    const variant = { ...changed, source: tag + String(changed.source) };
    expect(await hits([warm, warm, variant, variant])).toEqual([false, true, false, true]);
  });

  it("a cached subject still runs into a fresh scope", async () => {
    const job = {
      source: 'var seen = typeof leaked === "undefined" && typeof Array.prototype.vandal === "undefined"; leaked = 1; Array.prototype.vandal = 1; seen',
      sourceName: "scope",
    };
    file = join(dir, "scope.jsonl");
    process.env[TIMING_ENV] = file;
    try {
      for (let i = 0; i < 3; i++) expect((await runner.eval({ classAllowList: [], ...job })).value).toBe(true);
    } finally {
      delete process.env[TIMING_ENV];
    }
    const rows = readFileSync(file, "utf8").trim().split("\n").map((l) => JSON.parse(l) as { type: string; jvm: Record<string, number> })
      .filter((r) => r.type === "job");
    expect(rows.map((r) => r.jvm.subjectCacheHits ?? 0)).toEqual([0, 1, 1]);
  });

  it("a template-literal subject is compiled per job, never shared", async () => {
    const job = { source: "function tag(s) { return s.raw[0]; } tag`x`", sourceName: "template", languageVersion: 200 };
    file = join(dir, "template.jsonl");
    process.env[TIMING_ENV] = file;
    try {
      for (let i = 0; i < 2; i++) expect((await runner.eval({ classAllowList: [], ...job })).value).toBe("x");
    } finally {
      delete process.env[TIMING_ENV];
    }
    const rows = readFileSync(file, "utf8").trim().split("\n").map((l) => JSON.parse(l) as { type: string; jvm: Record<string, number> })
      .filter((r) => r.type === "job");
    expect(rows.map((r) => r.jvm.subjectCacheMisses)).toEqual([1, 1]);
  });
});
