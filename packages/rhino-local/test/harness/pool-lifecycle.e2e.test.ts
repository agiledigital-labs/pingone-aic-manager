import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, it, vi } from "vitest";
import { ensureRhinoJar, ensureRunnerClasses, JAVA_HOME_ENV, javaTool } from "../../src/jvm.ts";
import { JvmPool } from "../../src/pool.ts";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { TIMING_ENV } from "../../src/timing.ts";
import { runChild } from "./emulation-child.ts";
import { PID_PROBE } from "./isolation-scripts.ts";

it("compiles one cold build for four slots and reuses all four JVMs", async () => {
  const scratch = mkdtempSync(join(tmpdir(), "rhino-pool-cold-"));
  const cache = join(scratch, "cache");
  const resolved = await ensureRhinoJar();
  const priorSlot = process.env.VITEST_POOL_ID;
  const priorOverride = process.env.AIC_SCRIPT_TESTER_RHINO_JAR;
  const pool = await JvmPool.start({
    // Do a real cold build even when npm pack has left compiled runner classes.
    classes: (dependency, cache) => ensureRunnerClasses(dependency, { cache, prebuilt: join(scratch, "absent") }),
  });
  const runSlots = async (): Promise<unknown[]> => {
    const borrows = [1, 2, 3, 4].map((slot) => {
      process.env.VITEST_POOL_ID = String(slot);
      return RhinoRunner.borrow(pool.address, { lane: "host", cache });
    });
    return Promise.all(borrows.map(async (borrow) => {
      const runner = await borrow;
      try {
        return (await runner.eval(PID_PROBE)).value;
      } finally {
        await runner.close();
      }
    }));
  };
  try {
    process.env.AIC_SCRIPT_TESTER_RHINO_JAR = resolved;
    const first = await runSlots();
    expect(new Set(first).size).toBe(4);
    expect(pool.started).toBe(4);
    const second = await runSlots();
    expect(pool.started).toBe(4);
    expect(second).toEqual(first);
  } finally {
    if (priorSlot === undefined) delete process.env.VITEST_POOL_ID;
    else process.env.VITEST_POOL_ID = priorSlot;
    if (priorOverride === undefined) delete process.env.AIC_SCRIPT_TESTER_RHINO_JAR;
    else process.env.AIC_SCRIPT_TESTER_RHINO_JAR = priorOverride;
    await pool.stop();
    rmSync(scratch, { recursive: true, force: true });
  }
}, 90_000);

/** Run a different command on the first launch, then exec the real JVM. */
async function withHostLauncher(
  firstLaunch: string,
  run: (pool: JvmPool, marker: string, timing: string) => Promise<void>
): Promise<void> {
  const classes = await ensureRunnerClasses(await ensureRhinoJar());
  const directory = mkdtempSync(join(tmpdir(), "rhino-pool-launch-"));
  const pool = await JvmPool.start({ classes: () => Promise.resolve(classes) });
  try {
    const bin = join(directory, "bin");
    const marker = join(directory, "launched");
    const timing = join(directory, "timing.jsonl");
    mkdirSync(bin);
    const java = javaTool("java");
    writeFileSync(join(bin, "java"), `#!/bin/sh
if [ ! -f "$POOL_TEST_LAUNCHED" ]; then
  touch "$POOL_TEST_LAUNCHED"
  ${firstLaunch}
fi
exec "$POOL_TEST_JAVA" "$@"
`, { mode: 0o700 });
    vi.stubEnv(JAVA_HOME_ENV, directory);
    vi.stubEnv("POOL_TEST_JAVA", java);
    vi.stubEnv("POOL_TEST_LAUNCHED", marker);
    vi.stubEnv(TIMING_ENV, timing);
    await run(pool, marker, timing);
  } finally {
    await pool.stop();
    vi.unstubAllEnvs();
    rmSync(directory, { recursive: true, force: true });
  }
}

it.skipIf(process.platform === "win32")("a readiness timeout discards that launch and records the limit", async () => {
  await withHostLauncher("kill -STOP $$", async (pool, _marker, timing) => {
    const limit = 2_000;
    const reason = `host runner did not become ready within ${limit}ms`;
    await expect(RhinoRunner.borrow(pool.address, { lane: "host", spawnTimeoutMs: limit })).rejects.toThrow(reason);
    const replacement = await RhinoRunner.borrow(pool.address, { lane: "host" });
    try {
      expect(await replacement.eval({ source: "6 * 7", classAllowList: [] })).toMatchObject({ outcome: "ok", value: 42 });
    } finally {
      await replacement.close();
    }
    expect(pool.started).toBe(2);
    expect(readFileSync(timing, "utf8").trim().split("\n").map((line) => JSON.parse(line))).toContainEqual(
      expect.objectContaining({ type: "pool-discard", reason })
    );
  });
}, 60_000);

it.skipIf(process.platform === "win32")("a short readiness limit cannot abort a borrower with a longer limit", async () => {
  await withHostLauncher("kill -STOP $$", async (pool, marker) => {
    const borrow = (spawnTimeoutMs: number) => RhinoRunner.borrow(pool.address, {
      lane: "host", shared: true, spawnTimeoutMs,
    });
    const shortLimit = 2_000;
    const short = borrow(shortLimit).catch((error: unknown) => error);
    await expect.poll(() => existsSync(marker), { timeout: 10_000 }).toBe(true);
    const long = await borrow(20_000);
    try {
      expect(await short).toMatchObject({
        message: expect.stringContaining(`did not become ready within ${shortLimit}ms`),
      });
      expect(await long.eval({ source: "6 * 7", classAllowList: [] })).toMatchObject({ outcome: "ok", value: 42 });
      expect(pool.started).toBe(2);
    } finally {
      await long.close();
    }
  });
}, 60_000);

it.skipIf(process.platform === "win32")("the backstop identifies the JVM lost with two active jobs", async () => {
  const { code, output, report } = await runChild("pool-exit", "forks", false, { AIC_SCRIPT_TESTER_JVM_POOL: "1" });
  expect(code, output + report).toBe(1);
  for (const id of ["first-job", "second-job"]) {
    expect(output).toMatch(new RegExp(`Emulation run backstop: .*job ${id}: .*JVM runner exited \\(code=null, signal=SIGKILL\\)`));
  }
  expect(output).not.toContain("outstanding work");
  expect(JSON.parse(report).testResults[0].assertionResults).toContainEqual(
    expect.objectContaining({ title: "the next borrow runs on a replacement JVM", status: "passed" })
  );
}, 60_000);

it("closing a shared borrow cancels its job and preserves an in-flight sibling job", async () => {
  const directory = mkdtempSync(join(tmpdir(), "rhino-pool-close-"));
  const gate = join(directory, "release");
  const pool = await JvmPool.start();
  try {
    const closing = await RhinoRunner.borrow(pool.address, { lane: "host", shared: true, shutdownWaitMs: 150 });
    const sibling = await RhinoRunner.borrow(pool.address, { lane: "host", shared: true });
    try {
      const pid = (await sibling.eval(PID_PROBE)).value;
      expect((await closing.eval(PID_PROBE)).value).toBe(pid);
      expectEmulationFailure(/Error: rhino-local: lease closed with 1 job\(s\) still pending; its jobs cancelled/);
      const doomed = closing.eval({
        id: "doomed",
        source: 'java.lang.System.err.println("closing job started"); for (;;) {}',
        classAllowList: ["java.lang.System", "java.io.PrintStream"],
        timeoutMs: 0,
      }).catch((error: unknown) => error);
      const held = sibling.eval({
        id: "sibling",
        globals: { gate },
        source: `java.lang.System.err.println("sibling job started");
          while (!new java.io.File(gate).exists()) java.lang.Thread.sleep(10);
          42;`,
        classAllowList: ["java.io.File", "java.lang.System", "java.io.PrintStream", "java.lang.Thread"],
        timeoutMs: 0,
      }).catch((error: unknown) => error);
      await expect.poll(() => sibling.stderr.includes("closing job started") && sibling.stderr.includes("sibling job started"),
        { timeout: 15_000 }).toBe(true);
      await closing.close();
      writeFileSync(gate, "");
      expect(await held).toMatchObject({ outcome: "ok", value: 42 });
      expect(await doomed).toMatchObject({ message: expect.stringContaining("its jobs cancelled") });
      expect((await sibling.eval(PID_PROBE)).value).toBe(pid);
      await sibling.close();
      const next = await RhinoRunner.borrow(pool.address, { lane: "host" });
      try {
        // Merely rejecting the owner's JS promise would leave the tight loop
        // active and fail the real pool's idle check when the sibling returns.
        expect((await next.eval(PID_PROBE)).value).toBe(pid);
        expect(pool.started).toBe(1);
      } finally {
        await next.close();
      }
    } finally {
      writeFileSync(gate, "");
      await Promise.all([closing.close(), sibling.close()]);
    }
  } finally {
    await pool.stop();
    rmSync(directory, { recursive: true, force: true });
  }
}, 60_000);

it("a nested useLease can time out during teardown without closing its outer lease", async () => {
  const { code, output, report } = await runChild("pool-close", "forks", false, { AIC_SCRIPT_TESTER_JVM_POOL: "1" });
  expect(code, output + report).toBe(1);
  expect(JSON.parse(report).testResults[0].assertionResults).toContainEqual(
    expect.objectContaining({ title: "the outer lease can run after the inner teardown", status: "passed" })
  );
  expect(output).toContain("its jobs cancelled");
}, 60_000);
