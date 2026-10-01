/** JSON values the runner protocol can carry (job globals and completion values). */
export type JsonValue = null | boolean | number | string | JsonValue[] | {
    [key: string]: JsonValue;
};
/**
 * Machine-readable job outcomes. These are different bugs; the harness must
 * not collapse them into one string.
 *
 * - `ok` — compiled and ran
 * - `compile_error` — failed to compile
 * - `runtime_error` — threw at runtime
 * - `timeout` — instruction observer fired (`Error("Interrupt.")`)
 * - `protocol_error` — the job itself was malformed
 */
export type JobOutcome = "ok" | "compile_error" | "runtime_error" | "timeout" | "protocol_error";
export interface JobRequest {
    /** Correlates the response. Generated if omitted. */
    id?: string;
    source: string;
    /** Author's file path; becomes Rhino's source name (not `<eval>`). */
    sourceName?: string;
    /** Rhino language version. Default 0 (`VERSION_DEFAULT`). */
    languageVersion?: number | string;
    /**
     * Per-job timeout in milliseconds. `0` means no timeout (AM's default).
     * Omitted uses the runner's harness default (10s) so a runaway cannot hang
     * the process.
     */
    timeoutMs?: number;
    /** JSON values placed in ENGINE_SCOPE Bindings before eval. */
    globals?: {
        [key: string]: JsonValue;
    };
    /**
     * Java class allow-list for this job, as a script context's `allowLists`
     * entries (exact names, or a trailing `*` meaning prefix). Supplying one
     * installs AM's class shutter; omitting it leaves every Java name resolvable,
     * which is the pre-shutter behaviour.
     */
    classAllowList?: string[];
    /** Evaluated first, so author line numbers on `source` stay intact. */
    preamble?: string;
    preambleName?: string;
    /** Return this global after evaluation instead of Rhino's completion value. */
    resultGlobal?: string;
}
export interface JobError {
    class: string;
    message: string | null;
    sourceName: string | null;
    line: number;
    column: number;
    lineSource: string | null;
}
export interface JobResponse {
    id: string;
    outcome: JobOutcome;
    valueKind: string;
    value: JsonValue;
    error: JobError | null;
}
export declare function parseJobResponse(raw: unknown, line: string): JobResponse;
