/**
 * One AIC-lane run's transaction ids, parked for the vitest hook to pick up
 * if the test fails. Local-only tests never write one of these.
 */
export interface AicTrace {
    stem: string;
    passIds: string[];
    tenantName: string;
}
export declare function noteAicTrace(trace: AicTrace): void;
export declare function appendAicPass(id: string): void;
export declare function peekAicTrace(): AicTrace | undefined;
export declare function takeAicTrace(): AicTrace | undefined;
export declare function clearAicTrace(): void;
/** Subject chain: numbered passes under one stem. */
export declare function beginTrace(tenantName: string, stem?: string): {
    stem: string;
    next: () => string;
};
/**
 * A one-off authenticate that is not a numbered subject pass (the session
 * minter). Published so a mint failure is still look-up-able; the subject
 * trace replaces it if minting succeeds.
 */
export declare function beginSingleTrace(tenantName: string, id?: string): string;
