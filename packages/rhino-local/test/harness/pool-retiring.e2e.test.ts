import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { setTimeout as sleep } from "node:timers/promises";
import { join } from "node:path";
import { expect, it, vi } from "vitest";
import { JvmPool } from "../../src/pool.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { TIMING_ENV } from "../../src/timing.ts";
import { expectEmulationFailure } from "../../src/harness/emulation-guard.ts";
import { PID_PROBE } from "./isolation-scripts.ts";

function discards(timing: string): Array<{ type: string; reason?: string }> {
  return readFileSync(timing, "utf8").trim().split("\n").map((line) => JSON.parse(line))
    .filter((row) => row.type === "pool-discard");
}

function resumeStopped(pid: number): void {
  try {
    process.kill(pid, "SIGCONT");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error;
  }
}

it.skipIf(process.platform === "win32")("a lease arriving during the last cancellation wait gets a replacement JVM", async () => {
  const directory = mkdtempSync(join(tmpdir(), "rhino-pool-handoff-"));
  const timing = join(directory, "timings.jsonl");
  vi.stubEnv(TIMING_ENV, timing);
  const pool = await JvmPool.start();
  const borrowers: RhinoRunner[] = [];
  const borrow = async () => {
    const runner = await RhinoRunner.borrow(pool.address, { lane: "host", shared: true, shutdownWaitMs: 100 });
    borrowers.push(runner);
    return runner;
  };
  let pid = 0;
  let stopped = false;
  try {
    const owner = await borrow();
    const idle = await borrow();
    pid = Number((await owner.eval(PID_PROBE)).value);
    expect(pid).toBeGreaterThan(0);
    expectEmulationFailure("Error: rhino-local: lease closed with 1 job(s) still pending; its jobs cancelled");
    const work = owner.eval({
      id: "stuck-owner", source: 'java.lang.System.err.println("owner started"); for (;;) {}',
      classAllowList: ["java.lang.System", "java.io.PrintStream"], timeoutMs: 0,
    }).catch((error: unknown) => error);
    await expect.poll(() => owner.stderr.includes("owner started"), { timeout: 15_000 }).toBe(true);
    process.kill(pid, "SIGSTOP");
    stopped = true;
    await owner.close();
    expect(await work).toMatchObject({ message: expect.stringContaining("its jobs cancelled") });

    const closing = idle.close();
    await new Promise<void>((resolve) => setImmediate(resolve));
    const next = await borrow();
    // Assert before evaluating so the negative control cannot hang on the paused PID.
    expect(pool.started).toBe(2);
    expect((await next.eval(PID_PROBE)).value).not.toBe(pid);
    expect(await next.eval({ source: "42", classAllowList: [] })).toMatchObject({ outcome: "ok", value: 42 });
    await closing;
    expect(discards(timing)).toEqual([expect.objectContaining({
      reason: expect.stringContaining("did not acknowledge cancellation within 2000ms"),
    })]);
  } finally {
    if (stopped) resumeStopped(pid);
    await Promise.all(borrowers.map((runner) => runner.close()));
    await pool.stop();
    vi.unstubAllEnvs();
    rmSync(directory, { recursive: true, force: true });
  }
}, 30_000);

it.skipIf(process.platform === "win32")("final close discards a stopped JVM with a 1 MiB eval queued", async () => {
  const directory = mkdtempSync(join(tmpdir(), "rhino-pool-backpressure-"));
  const timing = join(directory, "timings.jsonl");
  vi.stubEnv(TIMING_ENV, timing);
  const pool = await JvmPool.start();
  const owner = await RhinoRunner.borrow(pool.address, { lane: "host", shared: true, shutdownWaitMs: 100 });
  const idle = await RhinoRunner.borrow(pool.address, { lane: "host", shared: true });
  let pid = 0;
  let stopped = false;
  try {
    pid = Number((await owner.eval(PID_PROBE)).value);
    expect(pid).toBeGreaterThan(0);
    process.kill(pid, "SIGSTOP");
    stopped = true;
    expectEmulationFailure("Error: rhino-local: lease closed with 1 job(s) still pending; its jobs cancelled");
    const work = owner.eval({ source: "42", globals: { padding: "x".repeat(1024 * 1024) }, classAllowList: [] })
      .catch((error: unknown) => error);
    await owner.close();
    expect(await work).toMatchObject({ message: expect.stringContaining("its jobs cancelled") });
    const finalClose = idle.close();
    const first = await Promise.race([finalClose.then(() => "closed"), sleep(11_000, "watchdog", { ref: false })]);
    expect(first, `starts=${pool.started}, discards=${discards(timing).length}`).toBe("closed");
    const next = await RhinoRunner.borrow(pool.address, { lane: "host", shared: true });
    try {
      expect(pool.started).toBe(2);
      expect((await next.eval(PID_PROBE)).value).not.toBe(pid);
      expect(discards(timing)).toEqual([expect.objectContaining({
        reason: expect.stringContaining("did not acknowledge cancellation within 2000ms"),
      })]);
    } finally {
      await next.close();
    }
  } finally {
    // Resume only for cleanup if the regression fails and leaves the old JVM stopped.
    if (stopped) resumeStopped(pid);
    await Promise.all([owner.close(), idle.close()]);
    await pool.stop();
    vi.unstubAllEnvs();
    rmSync(directory, { recursive: true, force: true });
  }
}, 30_000);
