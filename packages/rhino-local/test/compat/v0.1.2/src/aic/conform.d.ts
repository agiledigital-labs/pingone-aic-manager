import type { Case, JsonObject, RecordedEffects, Verdict } from "../case/types.ts";
import type { RunResult } from "../harness/lease.ts";
import { type EffectsDisagreement, type ObservationGap } from "./diff.ts";
import { type ManagedFixture } from "./managed.ts";
import { type AicReply } from "./run.ts";
export type LaneRunner = (args: {
    kase: Case;
    source: string;
}) => Promise<RecordedEffects>;
export interface ConformanceInput {
    kase: Case;
    source: string;
    local?: LaneRunner;
    /** Omit to skip; pass a runner, or `"tenant"` to use `runAicLane`. */
    aic?: LaneRunner | "tenant";
}
export interface LaneResult {
    effects?: RecordedEffects;
    verdict?: Verdict;
    skipped?: string;
    error?: string;
}
export interface ConformanceReport {
    name: string;
    portable: boolean;
    local: LaneResult;
    aic: LaneResult;
    disagreements: EffectsDisagreement[];
    observationGaps: ObservationGap[];
    ambientState: Array<{
        lane: "local" | "aic";
        values: JsonObject;
    }>;
}
/** The local lane's completed chain, ready to be checked by the AIC lane. */
export interface LocalChainResult {
    cases: readonly Case[];
    localEffects: readonly RecordedEffects[];
    replies: readonly (readonly AicReply[])[];
    /** Present only when the local result came through the lease fixture ledger. */
    managedFixtures?: readonly ManagedFixture[];
}
export type AicChainRunner = (args: {
    cases: readonly Case[];
    source: string;
    replies: readonly (readonly AicReply[])[];
    managedFixtures?: readonly ManagedFixture[];
}) => Promise<readonly RecordedEffects[]>;
export interface ChainConformanceInput extends LocalChainResult {
    source: string;
    /** Omit to skip; pass a runner, or `"tenant"` to use `runAicChain`. */
    aic?: AicChainRunner | "tenant";
}
export interface ChainPassReport extends ConformanceReport {
    /** 1-based position in the chain. */
    pass: number;
    final: boolean;
    /** False when the pass suspended before the wrapper could dump its effects. */
    aicObserved: boolean;
}
export interface ChainConformanceReport {
    name: string;
    portable: boolean;
    passes: ChainPassReport[];
    disagreements: EffectsDisagreement[];
    observationGaps: ObservationGap[];
}
/**
 * Run one case on both lanes and diff the two `RecordedEffects`.
 * Both lanes are judged by `judge()` from verdict.ts — a second pass/fail
 * implementation here would make the comparison prove nothing.
 *
 * Either lane may be omitted. A missing local runner is the expected state
 * until the bindings slice lands; a missing AIC runner is used by tests.
 */
export declare function conform(input: ConformanceInput): Promise<ConformanceReport>;
/**
 * Check an already-completed local chain against one AIC journey.
 *
 * The caller supplies every carried case and reply. This function deliberately
 * cannot compute either: making the AIC lane repeat the local carry rule would
 * hide a wrong mock when both lanes made the same guess.
 */
export declare function conformChain(input: ChainConformanceInput): Promise<ChainConformanceReport>;
/**
 * Convert `Lease.execute()` output without re-running or reconstructing it.
 * External one-shot callers may use this too; `useLease()` sends it to the
 * file's already-open `AicFileLease`, not to `runAicChain()`.
 */
export declare function chainFromRunResult(result: RunResult): LocalChainResult;
