import type { Case, RecordedEffects, Verdict } from "../case/types.ts";
import type { JobResponse } from "../protocol.ts";
// compat-accepted: an instance always comes from HEAD, and a class with #private fields is nominal.
import type { RhinoRunner } from "../../../../../src/runner.ts";
import type { EnvProfile } from "../profile/types.ts";
export interface RunCaseOptions {
    timeoutMs?: number;
    sourceName?: string;
    /** AM library script bodies keyed by `require()` id. See mockPreamble. */
    libraries?: Record<string, string>;
    /**
     * Pulled environment schema. Supplying one turns on strict fixture checking
     * and lets a declared-but-unseeded object type read as empty (AIC's `null`)
     * instead of a missing-fixture throw.
     */
    profile?: EnvProfile;
    /**
     * Install AM's Java class shutter. On by default: a scripted-decision case
     * should run behind the same allow-list the tenant enforces, or the harness
     * is more permissive than AIC and green means nothing. Pass `false` only to
     * demonstrate the difference.
     */
    classShutter?: boolean;
}
export interface CaseRun {
    effects: RecordedEffects;
    verdict: Verdict;
    response: JobResponse;
}
/**
 * Eval a case against the local Rhino runner: mocks as preamble, author script
 * as source, harvest appended so line numbers do not shift.
 */
export declare function runCase(runner: RhinoRunner, input: Case, options?: RunCaseOptions): Promise<CaseRun>;
