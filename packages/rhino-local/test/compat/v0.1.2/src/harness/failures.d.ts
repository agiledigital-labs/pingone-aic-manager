import type { AicTrace } from "../aic/trace.ts";
export interface FailureRecord {
    testName: string;
    stem: string;
    passIds: string[];
    file: string;
    suite: string;
    timestamp: string;
    tenant: string;
}
/**
 * A local-only failure has no transaction id, and must not produce a record
 * that would send `show-log` off to fetch nothing.
 */
export declare function failureRecordFor(input: {
    trace: AicTrace | undefined;
    testName: string;
    file: string;
    suite: string;
    timestamp: string;
}): FailureRecord | undefined;
export declare function appendFailure(record: FailureRecord, path?: string): Promise<void>;
export declare function recordFailureIfAny(input: Parameters<typeof failureRecordFor>[0], path?: string): Promise<FailureRecord | undefined>;
export declare function readFailures(path?: string): Promise<FailureRecord[]>;
export declare function parseFailures(text: string): FailureRecord[];
export declare function sortNewestFirst(records: readonly FailureRecord[]): FailureRecord[];
