/**
 * AM's `/monitoring/logs?transactionId=` query is a prefix match
 * (`docs/api/08-logs.md`). Unpadded `-1` silently absorbs `-10`. Two-digit
 * padding keeps each pass separately addressable; the cap is the cost.
 */
export declare const MAX_PASSES = 99;
export declare const PASS_DIGITS = 2;
/** Request header AM accepts; the supplied value wins (`docs/api/02-headers-and-versioning.md`). */
export declare const TX_HEADER = "x-forgerock-transactionid";
export declare function newStem(): string;
export declare function passId(stem: string, step: number): string;
