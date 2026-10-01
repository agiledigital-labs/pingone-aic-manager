/**
 * Next-gen scripted-decision source that dumps final `nodeState` into a
 * HiddenValueCallback. `outcome` is baked in because the tree routes each
 * subject-node outcome to a dedicated result node — the result script cannot
 * see which connection brought it here.
 *
 * Does not compare against `expect`. The runner builds `RecordedEffects`
 * from the dump; `judge()` in verdict.ts is the only pass/fail implementation.
 */
export declare function emitResultScript(outcome: string, snapshotKey: string, leaseDigest?: string): string;
