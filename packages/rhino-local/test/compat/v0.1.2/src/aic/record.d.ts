import type { CallbackEffect, Expect, Given, JsonObject, RecordedEffects } from "../case/types.ts";
export interface SubjectDump {
    outcome: string;
    before: JsonObject;
    final: JsonObject;
    leaseDigest?: string;
    invocationNonce?: string;
    subjectDigest?: string;
}
/**
 * Parse the result-script payload. The script records `outcome` and the
 * unified `nodeState` map; this module classifies keys into shared/transient/
 * secure using `given`, so the dump does not reimplement the verdict rule.
 */
export declare function parseSubjectDump(raw: unknown): SubjectDump;
export declare function assembleEffects(args: {
    given: Given;
    dump?: SubjectDump;
    callbacks: CallbackEffect[];
    priorExpectations?: readonly Expect[];
}): RecordedEffects;
/**
 * Classify only what the AIC wrapper actually established. The snapshots use
 * unified `nodeState.keys/get`, so a changed or added value is observable but
 * its bucket is not. Declared seeds remain exact only while unchanged; a seed
 * missing afterwards is an exact removal only when it existed in one bucket.
 * State present before the subject but absent from `given` is ambient.
 */
export declare function classifyFinal(given: Given, before: JsonObject, final: JsonObject, priorExpectations?: readonly Expect[]): Pick<Required<RecordedEffects>, "sharedState" | "transientState" | "secureState" | "evidence">;
