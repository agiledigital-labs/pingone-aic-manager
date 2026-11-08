/**
 * Readiness is event-driven: a lease resolves as soon as the ready line
 * reaches stderr, without waiting on a timer. With setTimeout faked, a
 * polling wait would never wake; this one must.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { RhinoRunner } from "../src/runner.ts";

describe("runner readiness", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("resolves on the ready line, not on a timer tick", async () => {
    const warm = await RhinoRunner.spawn({ lane: "host" });
    await warm.close(); // warm the runner build outside the faked window
    vi.useFakeTimers({ toFake: ["setTimeout"] });
    // A real-clock guard: setImmediate is not faked. Stopped once the race settles.
    let pending: NodeJS.Immediate | undefined;
    let settled = false;
    let runner: RhinoRunner;
    try {
      runner = await Promise.race([
        RhinoRunner.spawn({ lane: "host" }),
        new Promise<never>((_resolve, reject) => {
          const deadline = Date.now() + 20_000;
          const check = (): void => {
            if (settled) return;
            if (Date.now() > deadline) reject(new Error("readiness waited on a faked timer"));
            else pending = setImmediate(check);
          };
          check();
        }),
      ]);
    } finally {
      settled = true;
      if (pending !== undefined) clearImmediate(pending);
    }
    vi.useRealTimers();
    try {
      const response = await runner.eval({ source: "1 + 1", classAllowList: [] });
      expect(response.value).toBe(2);
    } finally {
      await runner.close();
    }
  }, 60_000);
});
