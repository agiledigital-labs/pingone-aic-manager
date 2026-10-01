import type { Case, EvidenceChannel, Expect, RecordedEffects, Verdict } from "../case/types.ts";
export interface EffectsDisagreement {
    channel: EvidenceChannel;
    path: string;
    local: string;
    aic: string;
    message: string;
}
export interface ObservationGap {
    channel: EvidenceChannel;
    path: string;
    local: string;
    aic: string;
    message: string;
}
export interface EffectsComparison {
    disagreements: EffectsDisagreement[];
    observationGaps: ObservationGap[];
}
/** Compare observable effects without turning absent evidence into equality. */
export declare function diffRecordedEffects(local: RecordedEffects, aic: RecordedEffects, expected?: Expect, priorExpectations?: readonly Expect[]): EffectsComparison;
export declare function judgeBoth(kase: Case, local: RecordedEffects, aic: RecordedEffects): {
    local: Verdict;
    aic: Verdict;
};
