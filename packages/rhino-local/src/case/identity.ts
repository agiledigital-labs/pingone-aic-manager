/** Canonicalise the 0.2.0 bag workaround before profile checking or Rhino seeding. */
import { assertDenseArray, isPlainObject } from "./util.ts";
import { identityPolicy } from "./identity-policy.ts";
import { deepEqual } from "./equal.ts";
import type { Given, JsonObject } from "./types.ts";

export function normaliseIdentitySeeds(given: Given): Given {
  // A declared layout overrides the measured bag, including its seed contract.
  const managed =
    given.identityAttributes?.["fr-idm-custom-attrs"] !== undefined
      ? given.managed
      : given.managed === undefined
        ? undefined
        : Object.fromEntries(
            Object.entries(given.managed).map(([collection, rows]) => [
              collection,
              rows.map((record) => normaliseRecord(record, collection)),
            ]),
          );
  const out = { ...given, ...(managed === undefined ? {} : { managed }) };
  if (given.identityCustomAttrs !== undefined) {
    out.identityCustomAttrs = parseIdentityCustomAttrs(
      given.identityCustomAttrs,
      managed,
      "given.identityCustomAttrs",
    );
  }
  if (given.identityCustomAttrsOwnedKeys !== undefined) {
    out.identityCustomAttrsOwnedKeys = parseIdentityCustomAttrsOwnedKeys(
      given.identityCustomAttrsOwnedKeys,
      managed,
      out.identityCustomAttrs,
      "given.identityCustomAttrsOwnedKeys",
    );
  }
  return out;
}

function normaliseRecord(record: JsonObject, collection: string): JsonObject {
  const bag = "fr-idm-custom-attrs";
  if (!Object.hasOwn(record, bag)) return record;
  const derived = Object.fromEntries(
    Object.entries(record).filter(([key]) => key.startsWith("custom_")),
  );
  const explicit = record[bag];
  const text =
    Array.isArray(explicit) && explicit.length === 1 ? explicit[0] : explicit;
  let parsed: unknown;
  if (typeof text === "string") {
    try {
      parsed = JSON.parse(text) as unknown;
    } catch {
      /* Invalid workaround fails with the same remedy below. */
    }
  }
  if (!deepEqual(parsed, derived)) {
    throw new Error(
      `rhino-local: given.managed.${collection}: "fr-idm-custom-attrs" disagrees with the derived custom_* bag; seed custom_* properties instead of an explicit AM bag value`,
    );
  }
  // The fact lives once in the canonical IDM record; equivalent legacy seeds
  // remain accepted, including different JSON key ordering.
  const out = { ...record };
  delete out[bag];
  return out;
}

/** Validate resource-keyed AM values and their IDM projection before trusting provenance. */
export function parseIdentityCustomAttrs(
  raw: unknown,
  managed: Record<string, JsonObject[]> | undefined,
  path: string,
): Record<string, string[]> {
  if (!isPlainObject(raw))
    throw new Error(`rhino-local: ${path} is not an object`);
  const out: Record<string, string[]> = {};
  for (const [resource, values] of Object.entries(raw)) {
    const record = identityRecord(managed, resource);
    if (record === undefined)
      throw new Error(
        `rhino-local: ${path}.${resource} does not identify a record in the managed store`,
      );
    if (
      !Array.isArray(values) ||
      values.some((value) => typeof value !== "string")
    ) {
      throw new Error(
        `rhino-local: ${path}.${resource} must be an array of strings`,
      );
    }
    assertDenseArray(values, `${path}.${resource}`);
    if (values.length > 1)
      throw new Error(
        `rhino-local: ${path}.${resource} must contain at most one persisted bag value`,
      );
    let parsed: unknown = {};
    if (values.length === 1) {
      try {
        parsed = JSON.parse(values[0] as string) as unknown;
      } catch {
        throw new Error(
          `rhino-local: ${path}.${resource} is not persisted JSON`,
        );
      }
    }
    const projected = isPlainObject(parsed) ? parsed : {};
    for (const [key, value] of Object.entries(projected)) {
      checkBagKey(key, `${path}.${resource}`);
      if (!Object.hasOwn(record, key) || !deepEqual(record[key], value)) {
        throw new Error(
          `rhino-local: ${path}.${resource} disagrees with managed property ${JSON.stringify(key)}`,
        );
      }
    }
    if (
      Object.keys(record).some(
        (key) => key.startsWith("custom_") && !Object.hasOwn(projected, key),
      )
    ) {
      throw new Error(
        `rhino-local: ${path}.${resource} omits a managed custom_* property`,
      );
    }
    out[resource] = values.slice() as string[];
  }
  return out;
}

/** Historical ownership survives clears; every currently present owned key must project from the bag. */
export function parseIdentityCustomAttrsOwnedKeys(
  raw: unknown,
  managed: Record<string, JsonObject[]> | undefined,
  bags: Record<string, string[]> = {},
  path: string,
): Record<string, string[]> {
  if (!isPlainObject(raw))
    throw new Error(`rhino-local: ${path} is not an object`);
  const out: Record<string, string[]> = {};
  for (const [resource, keys] of Object.entries(raw)) {
    const record = identityRecord(managed, resource);
    if (record === undefined)
      throw new Error(
        `rhino-local: ${path}.${resource} does not identify a record in the managed store`,
      );
    if (!Array.isArray(keys) || keys.some((key) => typeof key !== "string"))
      throw new Error(
        `rhino-local: ${path}.${resource} must be an array of strings`,
      );
    assertDenseArray(keys, `${path}.${resource}`);
    if (new Set(keys).size !== keys.length)
      throw new Error(
        `rhino-local: ${path}.${resource} must contain unique keys`,
      );
    const values = bags[resource];
    const parsed: unknown =
      values === undefined
        ? Object.fromEntries(
            Object.entries(record).filter(([key]) => key.startsWith("custom_")),
          )
        : values.length === 0
          ? {}
          : JSON.parse(values[0] as string);
    const projected = isPlainObject(parsed) ? parsed : {};
    for (const key of Object.keys(projected)) {
      if (!keys.includes(key))
        throw new Error(
          `rhino-local: ${path}.${resource} omits a current bag key ${JSON.stringify(key)}`,
        );
    }
    for (const key of keys as string[]) {
      checkBagKey(key, `${path}.${resource}`);
      if (Object.hasOwn(record, key) && !Object.hasOwn(projected, key))
        throw new Error(
          `rhino-local: ${path}.${resource} owns managed property ${JSON.stringify(key)} absent from the current bag`,
        );
    }
    out[resource] = (keys as string[]).slice().sort();
  }
  return out;
}

function identityRecord(
  managed: Record<string, JsonObject[]> | undefined,
  resource: string,
): JsonObject | undefined {
  if (!/^managed\/[^/]+\/[^/]+$/.test(resource)) return undefined;
  return Object.entries(managed ?? {}).flatMap(([collection, rows]) =>
    rows.filter((row) => `${collection}/${String(row._id)}` === resource),
  )[0];
}

function checkBagKey(key: string, path: string): void {
  const reason = identityPolicy.collisionReason(key);
  if (reason !== null) {
    throw new Error(
      `rhino-local: ${path}: fr-idm-custom-attrs key ${JSON.stringify(key)} collides with ${reason}; this identity mapping is unmeasured; use non-overlapping bag keys and identityAttributes fields`,
    );
  }
}
