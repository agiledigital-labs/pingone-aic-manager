import { randomUUID } from "node:crypto";

/**
 * AM's `/monitoring/logs?transactionId=` query is a prefix match
 * (`docs/api/08-logs.md`). Unpadded `-1` silently absorbs `-10`. Two-digit
 * padding keeps each pass separately addressable; the cap is the cost.
 */
export const MAX_PASSES = 99;

export const PASS_DIGITS = 2;

/** Request header AM accepts; the supplied value wins (`docs/api/02-headers-and-versioning.md`). */
export const TX_HEADER = "x-forgerock-transactionid";

export function newStem(): string {
  return randomUUID();
}

export function passId(stem: string, step: number): string {
  if (stem.length === 0) {
    throw new Error("rhino-local: transaction stem must be non-empty");
  }
  if (!Number.isInteger(step) || step < 1) {
    throw new Error(
      `rhino-local: transaction step must be an integer >= 1, got ${String(step)}`
    );
  }
  if (step > MAX_PASSES) {
    throw new Error(
      `rhino-local: transaction step ${step} exceeds ${MAX_PASSES} — two-digit padding would wrap into an id that is a prefix of another`
    );
  }
  return `${stem}-${String(step).padStart(PASS_DIGITS, "0")}`;
}
