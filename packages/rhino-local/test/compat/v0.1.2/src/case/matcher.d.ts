/** Recursive value matchers shared by verdicts, lane diffs, and seed checks. */
import type { ExpectedValue, StandardSchema } from "./types.ts";
export declare function isMatcher(value: unknown): value is RegExp | StandardSchema;
export declare function containsMatcher(value: unknown): boolean;
export declare function matchesValue(expected: ExpectedValue, actual: unknown): boolean;
/** Compare lanes exactly except at declared matcher leaves. */
export declare function equalExceptMatchers(expected: unknown, left: unknown, right: unknown): boolean;
/** The tenant's own carried value must satisfy prior matchers; other leaves stay exact. */
export declare function seedMatches(expected: unknown, local: unknown, tenant: unknown): boolean;
