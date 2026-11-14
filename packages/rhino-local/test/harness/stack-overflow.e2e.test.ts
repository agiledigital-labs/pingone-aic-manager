/**
 * Recursing past the stack is one script fault, not an internal emulator
 * failure. The interpreter says "Exceeded maximum stack depth". A compiled
 * script throws StackOverflowError. A required library keeps its own logger
 * (library-logger.e2e.test.ts); rebinding the caller's `logger` no longer
 * overflows.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { CaseRunError, runCase } from "../../src/bindings/run.ts";
import { defineCase, type Case } from "../../src/case/index.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { RhinoRunner } from "../../src/runner.ts";

const FAULT = "script-defect/stack-overflow";

describe("stack overflow is a script fault", () => {
  let runner: RhinoRunner;
  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);
  afterAll(async () => {
    await runner.close();
  }, 30_000);

  async function fail(kase: Case): Promise<CaseRunError> {
    expectHarnessFault(FAULT);
    try {
      await runCase(runner, kase);
    } catch (error) {
      expect(error).toBeInstanceOf(CaseRunError);
      const run = error as CaseRunError;
      expect(run.response.outcome).toBe("runtime_error");
      expect(run.response.emulation?.kind).toBe("missing");
      expect(run.response.emulation?.faults).toEqual([
        { id: FAULT, message: "the script exceeded the maximum stack depth" },
      ]);
      return run;
    }
    throw new Error("the case completed");
  }

  it("names a self-recursive subject", async () => {
    const run = await fail(defineCase({
      name: "self recursion",
      script: "function f() { f(); }\nf();",
      outcomes: ["done"],
      expect: { outcome: "done" },
    }));
    expect(run.message).toMatch(/runtime_error: Exceeded maximum stack depth/);
  }, 60_000);

  it("names a compiled script's StackOverflowError", async () => {
    expectHarnessFault(FAULT);
    const response = await runner.eval({
      engine: "idm",
      source: "function f() { f(); }\nf();",
      sourceName: "scripts/widget.cjs",
    });
    expect(response.outcome).toBe("runtime_error");
    expect(response.error?.class).toBe("java.lang.StackOverflowError");
    expect(response.emulation?.kind).toBe("missing");
    expect(response.emulation?.faults).toEqual([
      { id: FAULT, message: "the script exceeded the maximum stack depth" },
    ]);
  }, 60_000);
});
