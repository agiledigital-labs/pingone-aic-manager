/** Canonicalise the 0.2.0 bag workaround before profile checking or Rhino seeding. */
import { assertDenseArray, isPlainObject } from "./util.ts";
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
  const migrated =
    given.identityCustomAttrsAbsent === undefined
      ? {}
      : migrateIdentityAbsence(
          given.identityCustomAttrsAbsent,
          managed,
          "given.identityCustomAttrsAbsent",
        );
  const values =
    given.identityCustomAttrs === undefined
      ? {}
      : parseIdentityCustomAttrs(
          given.identityCustomAttrs,
          managed,
          "given.identityCustomAttrs",
        );
  for (const resource of Object.keys(migrated)) {
    if (values[resource] !== undefined && values[resource].length !== 0) {
      throw new Error(
        `rhino-local: given.identityCustomAttrs.${resource} conflicts with identityCustomAttrsAbsent`,
      );
    }
  }
  delete out.identityCustomAttrsAbsent;
  if (
    given.identityCustomAttrs !== undefined ||
    given.identityCustomAttrsAbsent !== undefined
  ) {
    out.identityCustomAttrs = { ...migrated, ...values };
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
    const record = Object.entries(managed ?? {}).flatMap(([collection, rows]) =>
      rows.filter(
        (row) =>
          `${collection}/${String(row._id)}` === resource &&
          /^managed\/[^/]+\/[^/]+$/.test(resource),
      ),
    )[0];
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

/** Compatibility input for round 1; canonical storage uses resource-keyed values. */
export function migrateIdentityAbsence(
  raw: unknown,
  managed: Record<string, JsonObject[]> | undefined,
  path: string,
): Record<string, string[]> {
  if (!Array.isArray(raw) || raw.some((value) => typeof value !== "string")) {
    throw new Error(`rhino-local: ${path} must be an array of strings`);
  }
  assertDenseArray(raw, path);
  return parseIdentityCustomAttrs(
    Object.fromEntries(raw.map((resource) => [resource, []])),
    managed,
    path,
  );
}
