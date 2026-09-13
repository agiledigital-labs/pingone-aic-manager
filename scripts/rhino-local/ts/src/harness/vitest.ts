import { afterAll, afterEach, beforeAll, expect } from "vitest";
import type { z } from "zod";
import { RhinoRunner } from "../runner.ts";
import { Lease } from "./lease.ts";
import type { LeaseOptions } from "./lease.ts";
import type { Suite } from "./lease.ts";

export interface UseLeaseOptions extends Partial<Omit<LeaseOptions, "runner">> {
  spawnTimeoutMs?: number;
}

/**
 * Take a lease for one test file, and register its own lifecycle.
 *
 * The hooks are registered here rather than left to the author because
 * forgetting teardown is the one mistake that leaks tenant state, and it
 * leaks silently — the suite still passes. Owning the hooks makes the leak
 * impossible to cause by omission.
 */
export function useLease<TSchema extends z.ZodType>(
  suite: Suite<TSchema>,
  options: UseLeaseOptions = {}
): Lease<TSchema> {
  let runner: RhinoRunner | undefined;
  const lease = new Lease(suite.spec, {
    get runner(): RhinoRunner {
      if (runner === undefined) {
        throw new Error(
          "rhino-local: lease used before beforeAll ran — call useLease() at describe scope, not inside a test"
        );
      }
      return runner;
    },
    ...(options.timeoutMs !== undefined ? { timeoutMs: options.timeoutMs } : {}),
    ...(options.allowResidue !== undefined ? { allowResidue: options.allowResidue } : {}),
    testName: () => expect.getState().currentTestName ?? suite.spec.name,
  } as LeaseOptions);

  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
    lease.open();
  }, options.spawnTimeoutMs ?? 60_000);

  afterEach(async () => {
    await lease.endTest();
  });

  afterAll(async () => {
    await lease.close();
    await runner?.close();
  }, 30_000);

  return lease;
}
