/**
 * Case isolation under compiled-script caching and JVM reuse.
 *
 * Before the cache, every case compiled and ran the whole preamble into a
 * fresh scope, so nothing one script did could reach the next. The runner now
 * shares compiled code across cases (java/ScriptCache.java) and `useLease`
 * borrows JVMs that outlive a test file (src/pool.ts). These tests pin that
 * a case still starts exactly as if fresh: after one that vandalised every
 * kind of state a script can reach, in the same runner and in a later
 * "file" (a release and re-borrow from the pool, which is the path the next
 * file in a worker takes), and after a case that timed out, crashed or retired
 * the JVM.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
import { defineCase } from "../../src/case/index.ts";
import type { Given } from "../../src/case/types.ts";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { cacheDir, ensureRhinoJar, ensureRunnerClasses } from "../../src/jvm.ts";
import { JvmPool, PooledChild } from "../../src/pool.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { TIMING_ENV } from "../../src/timing.ts";
import { FRESH, LIBRARIES, PID_PROBE, POLLUTER, PROBE } from "./isolation-scripts.ts";


async function runScript(runner: RhinoRunner, script: string, given: Given = { libraries: LIBRARIES }) {
  const kase = defineCase({ name: "isolation", script, outcomes: ["done"], given, expect: { outcome: "done" } });
  return (await runCase(runner, kase)).effects;
}

async function pollute(runner: RhinoRunner): Promise<void> {
  const effects = await runScript(runner, POLLUTER);
  // The polluter really ran, to the end, in its own case.
  expect(effects.outcome).toBe("done");
  expect(effects.sharedState.final.polluted).toBe(true);
  expect(effects.logs.map((log) => log.message)).toContain("polluter log");
}

async function probe(runner: RhinoRunner): Promise<void> {
  const effects = await runScript(runner, PROBE);
  expect(JSON.parse(String(effects.sharedState.final.probe))).toEqual(FRESH);
  expect(effects.sharedState.final.polluted).toBeUndefined();
  expect(effects.logs.map((log) => log.message)).not.toContain("polluter log");
}

async function jvmPid(runner: RhinoRunner): Promise<number> {
  const response = await runner.eval(PID_PROBE);
  expect(response.outcome).toBe("ok");
  return response.value as number;
}

describe("case isolation in one runner", () => {
  let runner: RhinoRunner;
  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);
  afterAll(async () => {
    await runner.close();
  });

  it("a probe in a fresh JVM sees fresh state (control)", async () => {
    await probe(runner);
  });

  it("the next case after a polluter starts exactly as if fresh", async () => {
    // Its own JVM, polluting first: anything cached is cached from the
    // polluter's job, which is the order that exposes a shared compiled form.
    const own = await RhinoRunner.spawn();
    try {
      await pollute(own);
      await probe(own);
      // And again: the second probe is served entirely from cached compiled scripts.
      await pollute(own);
      await probe(own);
    } finally {
      await own.close();
    }
  }, 60_000);

  it("concurrent cases do not see each other", async () => {
    await Promise.all([pollute(runner), probe(runner), pollute(runner), probe(runner)]);
  });

  it("a library edited between runs recompiles", async () => {
    const value = async (body: string): Promise<unknown> => {
      const effects = await runScript(
        runner,
        'nodeState.putShared("v", require("edited").v); action.goTo("done");',
        { libraries: { edited: body } }
      );
      return effects.sharedState.final.v;
    };
    expect(await value("exports.v = 1;")).toBe(1);
    expect(await value("exports.v = 2;")).toBe(2);
    expect(await value("exports.v = 1;")).toBe(1);
  });
});

describe("case isolation across files on a pooled JVM", () => {
  let pool: JvmPool;
  beforeAll(async () => {
    pool = await JvmPool.start();
  });
  afterAll(async () => {
    await pool.stop();
  }, 30_000);

  /** One "file": borrow the slot's JVM, use it, give it back. */
  async function file<T>(fn: (runner: RhinoRunner) => Promise<T>): Promise<T> {
    const runner = await RhinoRunner.borrow(pool.address, { lane: "host" });
    try {
      return await fn(runner);
    } finally {
      await runner.close();
    }
  }

  it("the next file reuses the JVM and starts as if fresh", async () => {
    const first = await file(async (runner) => {
      await pollute(runner);
      return jvmPid(runner);
    });
    const second = await file(async (runner) => {
      await probe(runner);
      return jvmPid(runner);
    });
    expect(second).toBe(first);
  });

  it("the next file on the same JVM ships no source text; a replacement JVM gets it again", async () => {
    // Shipped-source bookkeeping is per JVM, not per lease: the JVM's source
    // registry outlives a lease, so a re-borrow names prelude and libraries
    // by hash. A JVM the pool discarded starts empty, and must be sent them.
    const scratch = mkdtempSync(join(tmpdir(), "rl-shipped-"));
    const timing = join(scratch, "timing.jsonl");
    const previous = process.env[TIMING_ENV];
    process.env[TIMING_ENV] = timing;
    const jobs = (): Array<Record<string, unknown>> =>
      readFileSync(timing, "utf8").trim().split("\n").map((l) => JSON.parse(l) as Record<string, unknown>)
        .filter((row) => row.type === "job" && row.sourceName === "isolation");
    try {
      const pid = await file(async (runner) => {
        await runScript(runner, PROBE);
        return jvmPid(runner);
      });
      await file(async (runner) => {
        await runScript(runner, PROBE);
        expect(await jvmPid(runner)).toBe(pid);
      });
      expect(jobs().at(-1)).toMatchObject({ sourcesShipped: 0, sourceBytes: 0 });
      expect(jobs().at(-1)?.resent).toBeUndefined();
      // Retire this JVM: a lease closed with a job pending discards it.
      const doomed = await RhinoRunner.borrow(pool.address, { lane: "host", shutdownWaitMs: 200 });
      expectEmulationFailure(/(?:Error: )?rhino-local: lease closed with 1 job\(s\) still pending; JVM discarded/);
      const pending = doomed.eval({ timeoutMs: 0, source: "for (;;) {}", classAllowList: [] }).catch(() => undefined);
      await new Promise((resolve) => setTimeout(resolve, 100));
      await doomed.close();
      await pending;
      await file(async (runner) => {
        const effects = await runScript(runner, PROBE);
        expect(effects.outcome).toBe("done");
        expect(await jvmPid(runner)).not.toBe(pid);
      });
      // Earlier tests may have warmed the first JVM, so only the fresh one is
      // known to need everything: prelude plus every library the case loads.
      // ...shipped up front, not recovered by the unknown-source resend.
      expect(jobs().at(-1)?.sourcesShipped).toBe(1 + Object.keys(LIBRARIES).length);
      expect(jobs().at(-1)?.resent).toBeUndefined();
    } finally {
      if (previous === undefined) delete process.env[TIMING_ENV];
      else process.env[TIMING_ENV] = previous;
      rmSync(scratch, { recursive: true, force: true });
    }
  }, 60_000);

  it("a timed-out case does not poison the JVM for the next file", async () => {
    const first = await file(async (runner) => {
      expectEmulationFailure("Rhino harness timeout");
      const response = await runner.eval({ source: "for (;;) {}", timeoutMs: 200, classAllowList: [] });
      expect(response.outcome).toBe("timeout");
      return jvmPid(runner);
    });
    const second = await file(async (runner) => {
      await probe(runner);
      return jvmPid(runner);
    });
    expect(second).toBe(first);
  });

  it("a lease closed with a job still running discards that JVM", async () => {
    // timeoutMs 0: nothing inside the JVM ends this job, as with a Vitest
    // test timeout that fires before the harness's own.
    const runner = await RhinoRunner.borrow(pool.address, { lane: "host", shutdownWaitMs: 200 });
    const pid = await jvmPid(runner);
    expectEmulationFailure(/(?:Error: )?rhino-local: lease closed with 1 job\(s\) still pending; JVM discarded/);
    const outcome = runner.eval({ timeoutMs: 0, source: "for (;;) {}", classAllowList: [] }).then(
      () => "resolved",
      (error: unknown) => String(error)
    );
    await new Promise((resolve) => setTimeout(resolve, 100));
    await runner.close();
    expect(await outcome).toMatch(/lease closed with 1 job\(s\) still pending; JVM discarded/);
    expect(await file(jvmPid)).not.toBe(pid);
  });

  it("a JVM handed back with a job still running is not lent again", async () => {
    // A client that vanishes without killing its JVM (a worker that died):
    // only the pool's own status check stands between that job and the next file.
    const child = new PooledChild(pool.address, "host", process.env.VITEST_POOL_ID ?? `pid-${process.pid}`, cacheDir());
    const lines: string[] = [];
    let buffer = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => {
      buffer += chunk;
      for (let nl = buffer.indexOf("\n"); nl !== -1; nl = buffer.indexOf("\n")) {
        lines.push(buffer.slice(0, nl));
        buffer = buffer.slice(nl + 1);
      }
    });
    child.stdin.write(`${JSON.stringify({ id: "pid", ...PID_PROBE })}\n`);
    while (lines.length === 0) await new Promise((resolve) => setTimeout(resolve, 10));
    const pid = (JSON.parse(lines[0] as string) as { value: number }).value;
    child.stdin.write(`${JSON.stringify({ id: "busy", timeoutMs: 0, source: "for (;;) {}", classAllowList: [] })}\n`);
    await new Promise((resolve) => setTimeout(resolve, 100));
    const gone = new Promise((resolve) => child.once("exit", resolve));
    child.release();
    await gone;
    expect(pid).toBeGreaterThan(0);
    expect(await file(jvmPid)).not.toBe(pid);
  });

  it("a JVM is not reused by a client on another runner build", async () => {
    const jar = await ensureRhinoJar();
    const current = await ensureRunnerClasses(jar);
    const dir = mkdtempSync(join(tmpdir(), "rhino-build-"));
    const edited = join(dir, "classes");
    cpSync(current, edited, { recursive: true });
    let build = current;
    const builds = await JvmPool.start({ classes: () => Promise.resolve(build) });
    const pidOn = async (): Promise<number> => {
      const runner = await RhinoRunner.borrow(builds.address, { lane: "host" });
      try {
        return await jvmPid(runner);
      } finally {
        await runner.close();
      }
    };
    try {
      const first = await pidOn();
      // Control: same build, same JVM.
      expect(await pidOn()).toBe(first);
      build = edited;
      const second = await pidOn();
      expect(second).not.toBe(first);
      // A rebuild in place, as `npm run build` does to dist/classes: the same
      // path with a different manifest is another build.
      writeFileSync(join(edited, ".complete"), `${readFileSync(join(edited, ".complete"), "utf8")}rebuilt\n`);
      expect(await pidOn()).not.toBe(second);
    } finally {
      await builds.stop();
      rmSync(dir, { recursive: true, force: true });
    }
  }, 60_000);

  // Each way a job runs with no class shutter. Such a job can change the JVM
  // itself, and a normal finish does not undo it, so the JVM is not lent again.
  const POLLUTE_JVM =
    "(function () {" +
    " java.util.TimeZone.setDefault(java.util.TimeZone.getTimeZone('Australia/Sydney'));" +
    " java.lang.System.setProperty('rhinoLocalLeak', 'from-file-A');" +
    " return String(java.lang.System.getProperty('rhinoLocalLeak')); })()";
  const READ_JVM = "String(java.lang.System.getProperty('rhinoLocalLeak')) + ' ' + java.util.TimeZone.getDefault().getID()";
  const unshuttered: Array<[string, (runner: RhinoRunner) => Promise<unknown>]> = [
    ["a raw eval with no classAllowList", async (runner) => (await runner.eval({ source: POLLUTE_JVM })).value],
    ["the IDM engine", async (runner) => (await runner.eval({ engine: "idm", source: POLLUTE_JVM })).value],
    [
      "a case with classShutter: false",
      async (runner) => {
        const kase = defineCase({ name: "no-shutter", script: `nodeState.putShared("leak", ${POLLUTE_JVM}); action.goTo("done");`, outcomes: ["done"], expect: { outcome: "done" } });
        return (await runCase(runner, kase, { classShutter: false })).effects.sharedState.final.leak;
      },
    ],
  ];
  it.each(unshuttered)("a JVM that ran %s is not lent to the next file", async (_name, polluteJvm) => {
    const first = await file(async (runner) => {
      const pid = await jvmPid(runner);
      // The job really changed the JVM: it reads its own property back.
      expect(await polluteJvm(runner)).toBe("from-file-A");
      return pid;
    });
    const second = await file(async (runner) => {
      const seen = await runner.eval({ source: READ_JVM });
      expect(seen.value).toBe("null Etc/UTC");
      return jvmPid(runner);
    });
    expect(first).toBeGreaterThan(0);
    expect(second).not.toBe(first);
  });

  it("a JVM that crashed is replaced, not lent again", async () => {
    const crashed = await file(async (runner) => {
      const pid = await jvmPid(runner);
      expectEmulationFailure(/RhinoRunnerExitError: rhino-local: JVM runner exited[\s\S]*/);
      process.kill(pid, "SIGKILL");
      await expect(runner.eval({ source: "1" })).rejects.toThrow(/exited/);
      return pid;
    });
    const next = await file(async (runner) => {
      await probe(runner);
      return jvmPid(runner);
    });
    expect(next).not.toBe(crashed);
  });

  // A retiring job is always an IDM job, which also marks the JVM unshuttered,
  // so this passes on either flag; the retire branch alone is pinned by
  // pool-lendable.test.ts against `lendable`.
  it("a JVM whose IDM job retired it (and so ran unshuttered) is replaced, not lent again", async () => {
    const retired = await file(async (runner) => {
      const pid = await jvmPid(runner);
      expectEmulationFailure("spawned thread outlived its job");
      const result = await runner.eval({
        engine: "idm",
        timeoutMs: 2000,
        source: `
          var ready=new java.util.concurrent.CountDownLatch(1), gate=new java.util.concurrent.Semaphore(0);
          spawn(function(){ready.countDown();gate.acquireUninterruptibly()});ready.await();42;
        `,
      });
      expect(result.outcome).not.toBe("ok");
      return pid;
    });
    const next = await file(async (runner) => {
      await probe(runner);
      return jvmPid(runner);
    });
    expect(next).not.toBe(retired);
  });
});
