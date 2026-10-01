/**
 * Framework-free local lease and the narrow lane port used by AIC. Tenant
 * lifecycle and I/O stay in `aic/file-lease.ts`; this module hands the
 * recorded local run and its bound check/cleanup hooks across that boundary.
 */
import type { z } from "zod";
import type { ChainConformanceReport } from "../aic/conform.ts";
import type { Case, CallbackEffect, Expect, JsonObject, RecordedEffects, Verdict } from "../case/types.ts";
// compat-accepted: an instance always comes from HEAD, and a class with #private fields is nominal.
import type { RhinoRunner } from "../../../../../src/runner.ts";
import type { Lease as HeadLease } from "../../../../../src/harness/lease.ts";
import { normaliseWire } from "./spec.ts";
import type { StepSpec } from "./step.ts";
import type { Channels, FixtureSpec, IdmHandle, RequestDraft, SuiteSpec, WireMap } from "./types.ts";
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
    hooks: LeaseLaneHooks;
}
export type Check<TInput> = (idm: IdmHandle, ctx: CheckContext<TInput>) => void | Promise<void>;
export type LeaseLaneCheck = (idm: IdmHandle, effects: RecordedEffects) => void | Promise<void>;
/** Hooks bound to parsed input but replayed with each lane's own effects/IDM. */
export interface LeaseLaneHooks {
    stepChecks: readonly (LeaseLaneCheck | undefined)[];
    finalChecks: readonly LeaseLaneCheck[];
    cleanup?: (idm: IdmHandle) => void | Promise<void>;
}
/**
 * One test's run, assembled lazily.
 *
 * Deliberately NOT a thenable. A builder that is also a promise invites an
 * `await` halfway through the chain, after which `.check()` is appending to a
 * run that has already settled — a bug that reads as working code. Making
 * `.expect()` the only terminal turns that whole class into a type error.
 */
export declare class RunBuilder<TInput> {
    #private;
    constructor(lease: HeadLease<z.ZodType>, testName: string, rawInput: unknown);
    state(state: Channels["state"]): this;
    esv(esv: Readonly<Record<string, string>>): this;
    headers(headers: WireMap): this;
    params(params: WireMap): this;
    session(session: JsonObject): this;
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
    step(spec: StepSpec<TInput>): this;
    /** The same, for a chain built from data. */
    steps(specs: readonly StepSpec<TInput>[]): this;
    /** Assert against each lane's IDM store and response. Fail by throwing. */
    check(fn: Check<TInput>): this;
    /** The only terminal. */
    expect(expected: Expect): Promise<RunResult>;
}
export declare class Lease<TSchema extends z.ZodType> {
    #private;
    constructor(spec: SuiteSpec<TSchema>, options: LeaseOptions);
    /** Suite-scoped fixtures, created once before the first test. */
    open(): void;
    readonly fixtures: {
        create: (type: string, record: JsonObject | JsonObject[]) => Promise<void>;
    };
    run(input?: z.input<TSchema>): RunBuilder<z.output<TSchema>>;
    /**
     * Drained after every test. The per-test ledger goes; the suite's stays
     * until close(). Clearing only at teardown would let test 3 see test 1's
     * records, which is the cross-test interference this design exists to
     * remove.
     */
    endTest(): Promise<void>;
    /** Suite teardown: drop what the suite created, alongside the journey. */
    close(): Promise<void>;
    ledger(): readonly FixtureSpec[];
    execute(testName: string, rawInput: unknown, override: Channels, expected: Expect, checks: readonly Check<unknown>[], steps?: readonly StepSpec<unknown>[]): Promise<RunResult>;
}
export interface Suite<TSchema extends z.ZodType> {
    spec: SuiteSpec<TSchema>;
    /** compat-accepted: an instance always comes from HEAD (see the import above). */
    lease(options: LeaseOptions): HeadLease<TSchema>;
}
export declare function defineSuite<TSchema extends z.ZodType>(spec: SuiteSpec<TSchema>): Suite<TSchema>;
/** A managed record the suite creates once, for the whole file. */
export declare function managed(type: string, record: JsonObject): FixtureSpec;
export { normaliseWire };
export type { RequestDraft, SuiteSpec };
