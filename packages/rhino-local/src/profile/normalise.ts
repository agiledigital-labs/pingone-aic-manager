import { isPlainObject } from "../case/util.ts";
import type { JsonValue } from "../case/types.ts";
import type { EnvProfile, ObjectSchema, PropertySchema } from "./types.ts";

export class ProfileShapeError extends Error {
  constructor(message: string) {
    super(`rhino-local: ${message}`);
    this.name = "ProfileShapeError";
  }
}

/**
 * Normalise `GET /openidm/config/managed` into an `EnvProfile`.
 *
 * Pure, so the wire shape is testable without a tenant. Unknown keys are
 * dropped rather than carried: this profile is consumed by validation, and a
 * field nothing reads is a field that rots silently.
 */
export function normaliseManagedConfig(
  document: unknown,
  meta: { tenant: string; pulledAt: string; endpoint: string }
): EnvProfile {
  if (!isPlainObject(document)) {
    throw new ProfileShapeError("managed config is not an object");
  }
  const objects = document.objects;
  if (!Array.isArray(objects)) {
    throw new ProfileShapeError("managed config has no `objects` array");
  }
  const out: Record<string, ObjectSchema> = {};
  for (const entry of objects) {
    if (!isPlainObject(entry) || typeof entry.name !== "string") {
      throw new ProfileShapeError("managed object entry has no `name`");
    }
    out[entry.name] = normaliseObject(entry.name, entry.schema);
  }
  return {
    tenant: meta.tenant,
    pulledAt: meta.pulledAt,
    sources: [meta.endpoint],
    objects: out,
  };
}

function normaliseObject(name: string, schema: unknown): ObjectSchema {
  if (!isPlainObject(schema)) {
    // An object with no schema block is real (some shipped types carry none).
    // Record it as existing with no properties rather than dropping it: type
    // EXISTENCE is what distinguishes an AIC `null` from a fixture typo, and
    // that distinction must not depend on how richly the type is described.
    return { name, properties: {}, required: [] };
  }
  const properties: Record<string, PropertySchema> = {};
  if (isPlainObject(schema.properties)) {
    for (const [key, value] of Object.entries(schema.properties)) {
      properties[key] = normaliseProperty(value);
    }
  }
  const required = Array.isArray(schema.required)
    ? schema.required.filter((item): item is string => typeof item === "string")
    : [];
  return { name, properties, required };
}

function normaliseProperty(value: unknown): PropertySchema {
  if (!isPlainObject(value)) {
    return { type: "any" };
  }
  const { type, nullable } = normaliseType(value.type);
  const out: PropertySchema = { type };
  if (nullable) {
    out.nullable = true;
  }
  if (Array.isArray(value.enum)) {
    out.enum = value.enum as JsonValue[];
  }
  if (value.items !== undefined) {
    out.items = normaliseProperty(value.items);
  }
  const targets = normaliseResourceCollection(value.resourceCollection);
  if (targets.length > 0) {
    out.resourceCollection = targets;
  }
  return out;
}

/**
 * IDM spells a nullable scalar as `["string", "null"]`. Collapse that to one
 * type plus a flag so no consumer has to re-derive it.
 */
function normaliseType(raw: unknown): { type: string; nullable: boolean } {
  if (typeof raw === "string") {
    return { type: raw, nullable: false };
  }
  if (Array.isArray(raw)) {
    const names = raw.filter((item): item is string => typeof item === "string");
    const concrete = names.filter((item) => item !== "null");
    return {
      type: concrete[0] ?? "any",
      nullable: names.length !== concrete.length,
    };
  }
  return { type: "any", nullable: false };
}

function normaliseResourceCollection(raw: unknown): string[] {
  if (!Array.isArray(raw)) {
    return [];
  }
  const paths: string[] = [];
  for (const entry of raw) {
    if (isPlainObject(entry) && typeof entry.path === "string") {
      paths.push(entry.path);
    }
  }
  return paths;
}
