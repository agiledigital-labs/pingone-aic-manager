/** Canonicalise the 0.2.0 bag workaround before profile checking or Rhino seeding. */
import { deepEqual } from "./equal.ts";
import type { Given, JsonObject } from "./types.ts";

export function normaliseIdentitySeeds(given: Given): Given {
  if (given.managed === undefined || given.identityAttributes?.["fr-idm-custom-attrs"] !== undefined) {
    return given;
  }
  const managed = Object.fromEntries(Object.entries(given.managed).map(([collection, rows]) => [
    collection,
    rows.map((record) => normaliseRecord(record, collection)),
  ]));
  return { ...given, managed };
}

function normaliseRecord(record: JsonObject, collection: string): JsonObject {
  const bag = "fr-idm-custom-attrs";
  if (!Object.hasOwn(record, bag)) return record;
  const derived = Object.fromEntries(Object.entries(record).filter(([key]) => key.startsWith("custom_")));
  const explicit = record[bag];
  const text = Array.isArray(explicit) && explicit.length === 1 ? explicit[0] : explicit;
  let parsed: unknown;
  if (typeof text === "string") {
    try { parsed = JSON.parse(text) as unknown; } catch { /* Invalid workaround fails with the same remedy below. */ }
  }
  if (!deepEqual(parsed, derived)) {
    throw new Error(
      `rhino-local: given.managed.${collection}: "fr-idm-custom-attrs" disagrees with the derived custom_* bag; seed custom_* properties instead of an explicit AM bag value`
    );
  }
  // The fact lives once in the canonical IDM record; equivalent legacy seeds
  // remain accepted, including different JSON key ordering.
  const out = { ...record };
  delete out[bag];
  return out;
}
