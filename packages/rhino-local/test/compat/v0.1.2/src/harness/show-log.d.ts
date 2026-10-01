import { type FailureRecord } from "./failures.ts";
export interface TxChoice {
    id: string;
    label: string;
}
export interface ShowLogIo {
    readDump: () => Promise<FailureRecord[]>;
    print: (text: string) => void;
    error: (text: string) => void;
    selectTests: (records: FailureRecord[]) => Promise<FailureRecord[]>;
    selectIds: (record: FailureRecord, choices: TxChoice[]) => Promise<string[]>;
    /** Every log event for `id` on the tenant a failure record names. */
    readTransaction: (tenant: string, id: string) => Promise<unknown[]>;
    openEditor: (editor: string, file: string) => Promise<void>;
    writeView: (contents: string) => Promise<string>;
    env: NodeJS.ProcessEnv;
}
export declare function txChoices(record: FailureRecord): TxChoice[];
export declare function resolveLogsEditor(env: NodeJS.ProcessEnv): string | undefined;
/**
 * No tenant column: a tenant's name defaults to its hostname, and this list
 * reaches stdout (and so CI logs) whether or not `--stdout` was asked for.
 */
export declare function formatFailureList(records: readonly FailureRecord[]): string;
export declare function formatChoices(choices: readonly TxChoice[]): string;
/**
 * Empty input selects the first entry (newest test / the stem). `all` selects
 * every entry. Invalid input returns undefined so the caller can re-prompt.
 */
export declare function parseSelection(input: string, count: number, empty?: "first" | "all"): number[] | undefined;
export interface ShowLogOptions {
    /**
     * Print the log bodies instead of writing the view. Off by default: the
     * bodies are tenant data, and the view is a 0600 file in the self-ignoring
     * state directory, where a CI log is neither.
     */
    stdout?: boolean;
}
export declare function runShowLog(io: ShowLogIo, options?: ShowLogOptions): Promise<number>;
export declare function createDefaultIo(options?: {
    env?: NodeJS.ProcessEnv;
    project?: string;
}): ShowLogIo;
