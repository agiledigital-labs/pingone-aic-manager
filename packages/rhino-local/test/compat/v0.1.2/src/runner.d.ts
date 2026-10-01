import { type JvmLane, type RunnerEnvironment } from "./jvm.ts";
import { type JobRequest, type JobResponse } from "./protocol.ts";
export declare class RhinoRunnerExitError extends Error {
    readonly exitCode: number | null;
    readonly signal: NodeJS.Signals | null;
    readonly stderr: string;
    constructor(exitCode: number | null, signal: NodeJS.Signals | null, stderr: string);
}
/**
 * The host JVM and the AM image's JVM answered one job differently. This is
 * the whole reason the `both` lane exists, so it fails the test rather than
 * picking a winner.
 */
export declare class LaneDivergenceError extends Error {
    readonly job: string;
    readonly host: JobResponse;
    readonly container: JobResponse;
    constructor(job: string, host: JobResponse, container: JobResponse);
}
export interface SpawnRunnerOptions {
    /** Default: `AIC_SCRIPT_TESTER_JVM`, else `host`. */
    lane?: JvmLane;
    /** Jar and compiled-class cache. Default: `AIC_SCRIPT_TESTER_CACHE`, else `~/.cache/aic-script-tester`. */
    cache?: string;
    spawnTimeoutMs?: number;
}
export declare class RhinoRunner {
    #private;
    readonly lane: JvmLane;
    private constructor();
    static spawn(options?: SpawnRunnerOptions): Promise<RhinoRunner>;
    /** What the (primary) JVM reported at startup. */
    get environment(): RunnerEnvironment;
    eval(job: JobRequest): Promise<JobResponse>;
    close(): Promise<void>;
    /** Forcibly stop every JVM. Every pending eval rejects. Used by crash tests. */
    kill(): Promise<void>;
    get stderr(): string;
}
