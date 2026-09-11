import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
import { RhinoRunner } from "../../src/runner.ts";
import {
  blockedCases,
  realCases,
  runnableCases,
} from "../../cases/real/index.ts";

describe("real scripted-decision corpus", () => {
  let runner: RhinoRunner;

  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);

  afterAll(async () => {
    await runner.close();
  }, 30_000);

  it("registers every copied probe as a case", () => {
    expect(realCases.length).toBeGreaterThan(40);
    expect(runnableCases.length + blockedCases.length).toBe(realCases.length);
  });

  it("every blocked case names the missing method and the throw", () => {
    for (const entry of blockedCases) {
      expect(entry.blocked?.method, entry.kase.name).toBeTruthy();
      expect(entry.blocked?.throw, entry.kase.name).toBeTruthy();
    }
  });

  describe("cases that can produce a verdict today", () => {
    if (runnableCases.length === 0) {
      it("none — every real script is blocked on a missing binding or a parse error", () => {
        expect(runnableCases).toEqual([]);
      });
    }
    for (const entry of runnableCases) {
      it(entry.kase.name, async () => {
        const result = await runCase(runner, entry.kase, {
          sourceName: entry.origin,
          timeoutMs: 5_000,
        });
        expect(result.verdict.summary, result.verdict.summary).toBe("");
        expect(result.verdict.pass).toBe(true);
      });
    }
  });

  describe("blocked cases still throw the named gap", () => {
    for (const entry of blockedCases) {
      const blocked = entry.blocked;
      if (blocked === undefined) {
        continue;
      }
      it(`${entry.kase.name} — ${blocked.method}`, async () => {
        await expect(
          runCase(runner, entry.kase, {
            sourceName: entry.kase.name,
            timeoutMs: 5_000,
          })
        ).rejects.toThrow(blocked.throw);
      });
    }
  });
});
