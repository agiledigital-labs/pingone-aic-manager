import type { JsonValue } from "../case/types.ts";

/**
 * AM-safe expression that reconstructs `value` via `JSON.parse`.
 * Avoids object shorthand, destructuring, and other Rhino parse errors
 * that a hand-emitted object literal would have to dodge per property.
 */
export function jsonParseCall(value: JsonValue): string {
  return `JSON.parse(${JSON.stringify(JSON.stringify(value))})`;
}

export function jsStringLiteral(value: string): string {
  return JSON.stringify(value);
}
