import { relative } from "node:path";
import { afterAll, afterEach, beforeAll, beforeEach, expect } from "vitest";
import type { z } from "zod";
import type { File, Suite as VitestSuite } from "vitest";
import { AicFileLease } from "../aic/file-lease.ts";
import { chainFromRunResult } from "../aic/conform.ts";
import type { AicIo } from "../aic/tenant.ts";
import { clearAicTrace, takeAicTrace } from "../aic/trace.ts";
import { packageRoot } from "../paths.ts";
import { RhinoRunner } from "../runner.ts";
import { recordFailureIfAny } from "./failures.ts";
import {
  Lease,
  type LeaseLane,
  type LeaseOptions,
  type Suite,
} from "./lease.ts";

export interface UseLeaseAicOptions {
  id: string;
  realm?: string;
  tenant?: string;
  project?: string;
  unsupported?: "fail" | "skip";
}

export interface UseLeaseOptions
  extends Partial<Omit<LeaseOptions, "runner" | "lane" | "realm">> {
  spawnTimeoutMs?: number;
  aic?: UseLeaseAicOptions;
  /** Injected fake seam for adapter tests; production uses the default AIC I/O. */
  aicIo?: AicIo;
  /** Injected filesystem seam for adapter tests. */
  aicStateDir?: string;
}

const aicLeaseByFile = new Map<string, string>();

/**
 * Take a lease for one test file, and register its own lifecycle.
 *
 * The hooks are registered here rather than left to the author because
 * forgetting teardown is the one mistake that leaks tenant state, and it
 * leaks silently — the suite still passes. Owning the hooks makes the leak
 * impossible to cause by omission. The same applies to failure records: the
 * author must not have to remember to write the transaction id down.
 */
export function useLease<TSchema extends z.ZodType>(
  suite: Suite<TSchema>,
  options: UseLeaseOptions = {}
): Lease<TSchema> {
  let runner: RhinoRunner | undefined;
  let aicLease: AicFileLease | undefined;
  const lane: LeaseLane | undefined =
    options.aic === undefined
      ? undefined
      : {
          run: ({ result, source }) => {
            if (aicLease === undefined) {
              throw new Error("rhino-local: AIC lease used before beforeAll completed");
            }
            return aicLease.run({ ...chainFromRunResult(result), source });
          },
          endTest: () =>
            aicLease?.endTest() ?? Promise.resolve(),
        };
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
    ...(lane === undefined ? {} : { lane }),
    ...(options.aic === undefined ? {} : { realm: options.aic.realm ?? "alpha" }),
    testName: () => expect.getState().currentTestName ?? suite.spec.name,
  } as LeaseOptions);

  beforeAll(async (scope) => {
    runner = await RhinoRunner.spawn();
    lease.open();
    if (options.aic !== undefined) {
      claimAicLeaseForFile(vitestFilePath(scope), options.aic.id);
      aicLease = new AicFileLease({
        id: options.aic.id,
        suiteName: suite.spec.name,
        source: suite.spec.script,
        outcomes: suite.spec.outcomes,
        ...(options.aic.realm === undefined ? {} : { realm: options.aic.realm }),
        ...(options.aic.tenant === undefined ? {} : { tenant: options.aic.tenant }),
        ...(options.aic.project === undefined ? {} : { project: options.aic.project }),
        ...(options.aic.unsupported === undefined
          ? {}
          : { unsupported: options.aic.unsupported }),
        ...(options.aicIo === undefined ? {} : { io: options.aicIo }),
        ...(options.aicStateDir === undefined
          ? {}
          : { stateDir: options.aicStateDir }),
      });
      await aicLease.open();
    }
  }, options.spawnTimeoutMs ?? 60_000);

  beforeEach((context) => {
    clearAicTrace();
    context.onTestFailed(async () => {
      const trace = takeAicTrace();
      try {
        await recordFailureIfAny({
          trace,
          testName: expect.getState().currentTestName ?? context.task.name,
          file: relative(packageRoot, context.task.file.filepath),
          suite: suite.spec.name,
          timestamp: new Date().toISOString(),
        });
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        process.emitWarning(`rhino-local: could not record failure for show-log: ${message}`);
      }
    });
  });

  afterEach(async () => {
    await lease.endTest();
  });

  afterAll(async () => {
    const errors: unknown[] = [];
    try {
      await aicLease?.close();
    } catch (error) {
      errors.push(error);
    }
    try {
      await lease.close();
    } catch (error) {
      errors.push(error);
    }
    try {
      await runner?.close();
    } catch (error) {
      errors.push(error);
    }
    if (errors.length > 0) {
      throw new AggregateError(errors, "rhino-local lease teardown failed");
    }
  }, 30_000);

  return lease;
}

export function claimAicLeaseForFile(file: string, id: string): void {
  const existing = aicLeaseByFile.get(file);
  if (existing !== undefined) {
    throw new Error(
      `rhino-local: test file already has AIC lease ${JSON.stringify(existing)}; split ${JSON.stringify(id)} into another file`
    );
  }
  aicLeaseByFile.set(file, id);
}

export function releaseAicLeaseForFile(file: string, id?: string): void {
  if (id === undefined || aicLeaseByFile.get(file) === id) {
    aicLeaseByFile.delete(file);
  }
}

function vitestFilePath(scope: Readonly<VitestSuite | File>): string {
  return "filepath" in scope ? scope.filepath : scope.file.filepath;
}
