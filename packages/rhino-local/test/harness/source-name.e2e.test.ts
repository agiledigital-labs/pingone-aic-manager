/**
 * The lease compiles a subject under the script's source name, not the test
 * or step name. Timing rows and verdicts keep the test or step name. The
 * source name stays in the cache key: the same text under two source names
 * compiles twice.
 */
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { CaseRunError } from "../../src/bindings/index.ts";
import { defineSuite } from "../../src/harness/index.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { TIMING_ENV } from "../../src/timing.ts";

const dir = mkdtempSync(join(tmpdir(), "rl-source-name-"));
const SCRIPT = '/* shared-widget */ action.goTo("done");';
const FILE = "scripts/widget.cjs";
const OTHER = "scripts/other-widget.cjs";

interface JobRow {
  type: string;
  sourceName?: string;
  jvm?: { subjectCacheHits?: number; subjectCacheMisses?: number };
}

describe("stable script source name", () => {
  let previous: string | undefined;
  let runner: RhinoRunner;
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

  async function timed(fn: () => Promise<void>): Promise<JobRow[]> {
    const file = join(dir, `t${++n}.jsonl`);
    process.env[TIMING_ENV] = file;
    try {
      await fn();
    } finally {
      if (previous === undefined) delete process.env[TIMING_ENV];
      else process.env[TIMING_ENV] = previous;
    }
    return readFileSync(file, "utf8")
      .trim()
      .split("\n")
      .map((line) => JSON.parse(line) as JobRow)
      .filter((row) => row.type === "job");
  }

  function cacheOf(row: JobRow): "hit" | "miss" {
    const hits = row.jvm?.subjectCacheHits ?? 0;
    const misses = row.jvm?.subjectCacheMisses ?? 0;
    expect(hits + misses).toBe(1);
    return hits === 1 ? "hit" : "miss";
  }

  async function run(sourceName: string, testName: string, script = SCRIPT): Promise<void> {
    const lease = defineSuite({
      name: "widget-suite",
      sourceName,
      script,
      outcomes: ["done"],
    }).lease({ runner, testName: () => testName });
    const result = await lease.run().expect({ outcome: "done" });
    expect(result.kase.name).toBe(testName);
    expect(result.verdict.pass).toBe(true);
  }

  it("compiles one script once across case names, and twice across source names", async () => {
    const rows = await timed(async () => {
      await run(FILE, "case one");
      await run(OTHER, "case one");
      await run(FILE, "case two");
    });
    expect(rows.map((row) => row.sourceName)).toEqual(["case one", "case one", "case two"]);
    // The second row is the same text and the same case name under a different
    // source name, so it misses. The third reuses the first source name under
    // a different case name, so it hits. A key that ignored names would hit
    // the second row; a key that used the case name would miss the third.
    expect(rows.map(cacheOf)).toEqual(["miss", "miss", "hit"]);
  });

  it("compiles a callback step and the final pass once, and times each by its own name", async () => {
    const rows = await timed(async () => {
      const lease = defineSuite({
        name: "step-suite",
        sourceName: "scripts/step-widget.cjs",
        script:
          '/* step-widget */ if (callbacks.isEmpty()) { callbacksBuilder.nameCallback("Who"); } else { action.goTo("done"); }',
        outcomes: ["done"],
      }).lease({ runner, testName: () => "who" });
      const result = await lease
        .run()
        .step({
          expect: { callbacks: [{ type: "NameCallback", prompt: "Who" }] },
          reply: [{ type: "NameCallback", value: "ada" }],
        })
        .expect({ outcome: "done" });
      expect(result.kase.name).toBe("who");
      expect(result.steps.map((step) => step.kase.name)).toEqual(["who [step 1]"]);
      expect(result.verdict.pass).toBe(true);
    });
    expect(rows.map((row) => row.sourceName)).toEqual(["who [step 1]", "who"]);
    expect(rows.map(cacheOf)).toEqual(["miss", "hit"]);
  });

  it("names a compile diagnostic with the script file", async () => {
    const rows = await timed(async () => {
      const lease = defineSuite({
        name: "parse-suite",
        sourceName: FILE,
        script: "var ok = 1;\nlet banned = 2;\n",
        outcomes: ["done"],
      }).lease({ runner, testName: () => "parse in alpha" });
      try {
        await lease.run().expect({ outcome: "done" });
        expect.fail("the script should fail to compile");
      } catch (error) {
        expect(error).toBeInstanceOf(CaseRunError);
        const run = error as CaseRunError;
        expect(run.message).toContain("parse in alpha");
        expect(run.response.error?.sourceName).toBe(FILE);
        expect(run.response.error?.line).toBe(2);
        expect(run.response.error?.message).toContain(FILE);
      }
    });
    expect(rows.map((row) => row.sourceName)).toEqual(["parse in alpha"]);
  });

  it("uses the suite name when no source name is set", async () => {
    const rows = await timed(async () => {
      const suite = defineSuite({
        name: "plain-widget",
        script: '/* plain-widget */ action.goTo("done");',
        outcomes: ["done"],
      });
      for (const testName of ["left", "right"]) {
        const result = await suite.lease({ runner, testName: () => testName }).run().expect({ outcome: "done" });
        expect(result.kase.name).toBe(testName);
      }
    });
    expect(rows.map((row) => row.sourceName)).toEqual(["left", "right"]);
    expect(rows.map(cacheOf)).toEqual(["miss", "hit"]);
  });

  it("keeps the compile name when the scriptName binding changes", async () => {
    const rows = await timed(async () => {
      for (const binding of ["binding-one", "binding-two"]) {
        const result = await defineSuite({
          name: "binding-suite",
          sourceName: "scripts/binding-widget.cjs",
          scriptName: binding,
          script: '/* binding-widget */ action.goTo("done");',
          outcomes: ["done"],
        })
          .lease({ runner, testName: () => `run ${binding}` })
          .run()
          .expect({ outcome: "done" });
        expect(result.harnessIdentity?.scriptName).toBe(binding);
        expect(result.kase.name).toBe(`run ${binding}`);
      }
    });
    expect(rows.map((row) => row.sourceName)).toEqual(["run binding-one", "run binding-two"]);
    expect(rows.map(cacheOf)).toEqual(["miss", "hit"]);
  });
});
