import type { HttpEffect, HttpExpect, OpenidmEffect, OpenidmExpect, Verdict } from "./types.ts";
/**
 * Pure function: (case, recorded effects) → pass/fail with a readable
 * explanation of each difference. Does not run a script.
 */
export declare function judge(input: unknown, effects: unknown): Verdict;
/** Also used by the lane diff to identify effects actually declared by a body matcher. */
export declare function openidmMatches(expected: OpenidmExpect, actual: OpenidmEffect): boolean;
/** Also used by the lane diff to identify effects actually declared by a body matcher. */
export declare function httpMatches(expected: HttpExpect, actual: HttpEffect): boolean;
