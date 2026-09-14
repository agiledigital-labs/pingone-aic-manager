/**
 * Framework-free local lease and the narrow lane port used by AIC. Tenant
 * lifecycle and I/O stay in `aic/file-lease.ts`; this module only hands the
 * already-recorded local run across that boundary.
 */
import type { z } from "zod";
import type { ChainConformanceReport } from "../aic/conform.ts";
import { judge } from "../case/index.ts";
import type {
  Case,
  CallbackEffect,
  Expect,
  JsonObject,
  RecordedEffects,
  Verdict,
} from "../case/types.ts";
import { runCase } from "../bindings/index.ts";
import type { RhinoRunner } from "../runner.ts";
import { ledgerToManaged, localIdmHandle } from "./idm.ts";
import { describeResidue, findResidue } from "./residue.ts";
import {
  applyInputsAndEsv,
  caseWithGiven,
  mergeChannels,
  normaliseWire,
  parseInputs,
  toGiven,
} from "./spec.ts";
import { carryGiven, submittedCallbacks } from "./step.ts";
import type { CallbackReply, StepContext, StepSpec } from "./step.ts";
import type {
  Channels,
  FixtureSpec,
  IdmHandle,
  RequestDraft,
  SuiteSpec,
  WireMap,
} from "./types.ts";

export interface RunResult {
  kase: Case;
  effects: RecordedEffects;
  verdict: Verdict;
  /** Fixture provenance for the AIC lane; author-declared managed data has none. */
  fixtures: readonly FixtureSpec[];
  /**
   * One entry per suspended pass, in order. Empty for a single-pass run.
   * `aic/chainFromRunResult` consumes these recorded cases and submissions;
   * the AIC lane must not reconstruct them from the final case.
   */
  steps: StepResult[];
  /** Present when this lease automatically checked the completed run on AIC. */
  conformance?: ChainConformanceReport;
}

/** One pass of a step chain: what it was asked, what it did, what was sent back. */
export interface StepResult {
  kase: Case;
  effects: RecordedEffects;
  verdict: Verdict;
  /** Exactly what the next pass was handed as `given.callbacks`. */
  submitted: CallbackEffect[];
}

export interface CheckContext<TInput> {
  input: TInput;
  effects: RecordedEffects;
}

export interface LeaseOptions {
  runner: RhinoRunner;
  timeoutMs?: number;
  /** A test that deliberately leaves a record behind and asserts on it. */
  allowResidue?: boolean;
  /**
   * How to name the run in failure messages. Supplied by the vitest adapter
   * so this module stays framework-free — the residue check and the merge
   * logic are worth unit-testing without a test runner in the way.
   */
  testName?: () => string;
  /** Fixed tenant realm; the Vitest AIC adapter supplies it to both lanes. */
  realm?: string;
  /** Cross-feature port implemented by the tenant-aware AIC vertical. */
  lane?: LeaseLane;
}

export interface LeaseLane {
  run(request: LeaseLaneRunRequest): Promise<ChainConformanceReport>;
  endTest(): Promise<void>;
}

export interface LeaseLaneRunRequest {
  result: RunResult;
  source: string;
}

type Check<TInput> = (idm: IdmHandle, ctx: CheckContext<TInput>) => void | Promise<void>;

/**
 * One test's run, assembled lazily.
 *
 * Deliberately NOT a thenable. A builder that is also a promise invites an
 * `await` halfway through the chain, after which `.check()` is appending to a
 * run that has already settled — a bug that reads as working code. Making
 * `.expect()` the only terminal turns that whole class into a type error.
 */
export class RunBuilder<TInput> {
  readonly #lease: Lease<z.ZodType>;
  readonly #testName: string;
  readonly #rawInput: unknown;
  #override: Channels = {};
  readonly #checks: Check<TInput>[] = [];
  readonly #steps: StepSpec<TInput>[] = [];

  constructor(lease: Lease<z.ZodType>, testName: string, rawInput: unknown) {
    this.#lease = lease;
    this.#testName = testName;
    this.#rawInput = rawInput;
  }

  state(state: Channels["state"]): this {
    this.#override = { ...this.#override, state: mergeState(this.#override.state, state) };
    return this;
  }

  esv(esv: Readonly<Record<string, string>>): this {
    this.#override = { ...this.#override, esv: { ...this.#override.esv, ...esv } };
    return this;
  }

  headers(headers: WireMap): this {
    this.#override = { ...this.#override, headers: { ...this.#override.headers, ...headers } };
    return this;
  }

  params(params: WireMap): this {
    this.#override = { ...this.#override, params: { ...this.#override.params, ...params } };
    return this;
  }

  session(session: JsonObject): this {
    this.#override = { ...this.#override, session: { ...this.#override.session, ...session } };
    return this;
  }

  /**
   * Declare one suspended pass: what the script must send, what the client
   * sends back, and what must be true of the world in between.
   *
   * Passes run in declaration order and the terminal `.expect()` judges the
   * one after the last step, so a two-callback journey is two `.step()` calls
   * and one `.expect()`. A step whose expectations fail aborts the chain
   * rather than replying anyway: every later pass is seeded from this one, so
   * continuing would report a cascade of failures that all trace back here.
   */
  step(spec: StepSpec<TInput>): this {
    this.#steps.push(spec);
    return this;
  }

  /** The same, for a chain built from data. */
  steps(specs: readonly StepSpec<TInput>[]): this {
    for (const spec of specs) {
      this.#steps.push(spec);
    }
    return this;
  }

  /**
   * Assert against the local IDM store and raw response. The AIC lane does not
   * yet have a tenant-backed `IdmHandle`; opted-in reports record that explicit
   * observation gap instead of pretending this closure ran remotely. Fail by
   * throwing — vitest's `expect` remains the assertion surface.
   */
  check(fn: Check<TInput>): this {
    this.#checks.push(fn);
    return this;
  }

  /** The only terminal. */
  async expect(expected: Expect): Promise<RunResult> {
    return this.#lease.execute(
      this.#testName,
      this.#rawInput,
      this.#override,
      expected,
      this.#checks as Check<unknown>[],
      this.#steps as StepSpec<unknown>[]
    );
  }
}

export class Lease<TSchema extends z.ZodType> {
  readonly #spec: SuiteSpec<TSchema>;
  readonly #options: LeaseOptions;
  /** Everything the harness created, in creation order. */
  #suiteLedger: FixtureSpec[] = [];
  #testLedger: FixtureSpec[] = [];

  constructor(spec: SuiteSpec<TSchema>, options: LeaseOptions) {
    this.#spec = spec;
    this.#options = options;
  }

  /** Suite-scoped fixtures, created once before the first test. */
  open(): void {
    this.#suiteLedger = [];
    for (const fixture of Object.values(this.#spec.fixtures ?? {})) {
      this.#suiteLedger.push(fixture);
    }
  }

  readonly fixtures = {
    create: (type: string, record: JsonObject | JsonObject[]): Promise<void> => {
      for (const one of Array.isArray(record) ? record : [record]) {
        this.#testLedger.push({ type, record: one });
      }
      return Promise.resolve();
    },
  };

  run(input?: z.input<TSchema>): RunBuilder<z.output<TSchema>> {
    return new RunBuilder(
      this as unknown as Lease<z.ZodType>,
      this.#options.testName?.() ?? this.#spec.name,
      input
    );
  }

  /**
   * Drained after every test. The per-test ledger goes; the suite's stays
   * until close(). Clearing only at teardown would let test 3 see test 1's
   * records, which is the cross-test interference this design exists to
   * remove.
   */
  async endTest(): Promise<void> {
    try {
      await this.#options.lane?.endTest();
    } finally {
      this.#testLedger = [];
    }
  }

  /** Suite teardown: drop what the suite created, alongside the journey. */
  close(): Promise<void> {
    this.#testLedger = [];
    this.#suiteLedger = [];
    return Promise.resolve();
  }

  ledger(): readonly FixtureSpec[] {
    return [...this.#suiteLedger, ...this.#testLedger];
  }

  async execute(
    testName: string,
    rawInput: unknown,
    override: Channels,
    expected: Expect,
    checks: readonly Check<unknown>[],
    steps: readonly StepSpec<unknown>[] = []
  ): Promise<RunResult> {
    const input = parseInputs(this.#spec, rawInput);
    const draft = mergeChannels(this.#spec.always, override);
    if (this.#spec.beforeRun !== undefined) {
      await this.#spec.beforeRun({
        input: input as z.output<TSchema>,
        request: draft,
        fixtures: this.fixtures,
      });
    }
    applyInputsAndEsv(draft, input);
    const ledger = this.ledger();
    let given = toGiven(
      draft,
      {
        managed: ledgerToManaged(ledger),
        ...(this.#options.realm === undefined
          ? {}
          : { realm: this.#options.realm }),
      },
      this.#options.realm
    );
    if (steps.length > 0 && given.callbacks === undefined) {
      // Declaring a step says the script suspends, and a script that suspends
      // reads `callbacks` to tell its first pass from its later ones. AM's
      // first pass carries an empty list, so seed one rather than leaving the
      // binding unseeded and failing on a read the chain guarantees.
      given = { ...given, callbacks: [] };
    }
    const stepResults: StepResult[] = [];
    for (const [index, step] of steps.entries()) {
      given = await this.#runStep(testName, index, step, given, input, stepResults);
    }
    const kase = caseWithGiven(this.#spec, testName, given, expected);
    const effects = await runCase(this.#options.runner, kase, {
      ...(this.#options.timeoutMs !== undefined ? { timeoutMs: this.#options.timeoutMs } : {}),
    });
    const verdict = judge(kase, effects.effects);

    const store = cloneStore(effects.effects.managedStore);
    const idm = localIdmHandle(store ?? {}, () => undefined);
    try {
      for (const check of checks) {
        await check(idm, { input, effects: effects.effects });
      }
    } finally {
      if (this.#spec.cleanup !== undefined) {
        await this.#spec.cleanup(idm, { input: input as z.output<TSchema> });
      }
    }
    // The store is diffed AFTER cleanup ran, which is the only ordering that
    // tests the cleanup rather than the script.
    if (this.#options.allowResidue !== true) {
      const residue = findResidue(store, ledger);
      if (residue.length > 0) {
        throw new Error(`rhino-local: ${kase.name}\n  ${describeResidue(residue)}`);
      }
    }
    const result: RunResult = {
      kase,
      effects: effects.effects,
      verdict,
      fixtures: ledger,
      steps: stepResults,
    };
    if (this.#options.lane !== undefined) {
      const conformance = await this.#options.lane.run({
        result,
        source: this.#spec.script,
      });
      const gap = {
        channel: "openidm" as const,
        path: "checks/cleanup",
        local: "local IdmHandle only",
        aic: "not replayed",
        message:
          "openidm: step/final check() hooks and suite cleanup ran against the local store only; no tenant-backed IdmHandle exists",
      };
      conformance.observationGaps.push(gap);
      const final = conformance.passes[conformance.passes.length - 1];
      final?.observationGaps.push(gap);
      result.conformance = conformance;
    }
    return result;
  }

  /**
   * Run one suspended pass and return the seed for the next.
   *
   * The step's `check` is handed the run's own store rather than a copy, so a
   * record it deletes really is gone from the pass that follows. A copy would
   * let a cleanup written between two halves of a journey look like it worked
   * while the next pass still saw the record.
   */
  async #runStep(
    testName: string,
    index: number,
    step: StepSpec<unknown>,
    given: Case["given"],
    input: Record<string, unknown>,
    results: StepResult[]
  ): Promise<Case["given"]> {
    const kase = caseWithGiven(this.#spec, `${testName} [step ${index + 1}]`, given, {
      ...(step.expect ?? {}),
      outcome: null,
    });
    const run = await runCase(this.#options.runner, kase, {
      ...(this.#options.timeoutMs !== undefined ? { timeoutMs: this.#options.timeoutMs } : {}),
    });
    const verdict = judge(kase, run.effects);
    if (!verdict.pass) {
      throw new Error(`rhino-local: ${kase.name}\n${verdict.summary}`);
    }
    const context: StepContext<unknown> = {
      input,
      step: index + 1,
      callbacks: run.effects.callbacks,
      effects: run.effects,
    };
    if (step.check !== undefined) {
      if (run.effects.managedStore === undefined) {
        run.effects.managedStore = {};
      }
      await step.check(
        localIdmHandle(run.effects.managedStore, () => undefined),
        context
      );
    }
    const replies: readonly CallbackReply[] =
      typeof step.reply === "function" ? step.reply(context) : step.reply;
    const submitted = submittedCallbacks(run.effects.callbacks, replies, kase.name);
    results.push({ kase, effects: run.effects, verdict, submitted });
    return carryGiven(given, run.effects, submitted);
  }
}

export interface Suite<TSchema extends z.ZodType> {
  spec: SuiteSpec<TSchema>;
  lease(options: LeaseOptions): Lease<TSchema>;
}

export function defineSuite<TSchema extends z.ZodType>(
  spec: SuiteSpec<TSchema>
): Suite<TSchema> {
  if (spec.outcomes.length === 0) {
    throw new Error(
      `rhino-local: suite ${JSON.stringify(spec.name)} must declare its outcomes — a tenant answers an undeclared outcome with a bare 401 and no callback`
    );
  }
  return {
    spec,
    lease: (options) => new Lease(spec, options),
  };
}

/** A managed record the suite creates once, for the whole file. */
export function managed(type: string, record: JsonObject): FixtureSpec {
  return { type, record };
}

function mergeState(
  a: Channels["state"],
  b: Channels["state"]
): NonNullable<Channels["state"]> {
  return {
    shared: { ...(a?.shared ?? {}), ...(b?.shared ?? {}) },
    transient: { ...(a?.transient ?? {}), ...(b?.transient ?? {}) },
  };
}

function cloneStore(
  store: Record<string, JsonObject[]> | undefined
): Record<string, JsonObject[]> | undefined {
  return store === undefined
    ? undefined
    : (JSON.parse(JSON.stringify(store)) as Record<string, JsonObject[]>);
}

export { normaliseWire };
export type { RequestDraft, SuiteSpec };
