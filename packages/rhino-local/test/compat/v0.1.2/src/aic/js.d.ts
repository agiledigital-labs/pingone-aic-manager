import type { JsonValue } from "../case/types.ts";
/**
 * AM-safe expression that reconstructs `value` via `JSON.parse`.
 * Avoids object shorthand, destructuring, and other Rhino parse errors
 * that a hand-emitted object literal would have to dodge per property.
 */
export declare function jsonParseCall(value: JsonValue): string;
export declare function jsStringLiteral(value: string): string;
