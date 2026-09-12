import { isPlainObject } from "../case/util.ts";
import type { JsonObject, JsonValue } from "../case/types.ts";
import type { EnvProfile, ObjectSchema } from "./types.ts";

/**
 * CREST metadata every managed record carries regardless of the declared
 * schema. Allowed unconditionally so strict property checking does not reject
 * a record the tenant itself would return.
 */
const CREST_META = new Set(["_id", "_rev", "_meta", "_ref", "_refProperties"]);

export class SchemaViolation extends Error {
  readonly kind: "unknown-type" | "unknown-property" | "missing-required" | "bad-enum";
  readonly resource: string;

  constructor(
    kind: SchemaViolation["kind"],
    resource: string,
    message: string
  ) {
    super(`rhino-local: ${message}`);
    this.name = "SchemaViolation";
    this.kind = kind;
    this.resource = resource;
  }
}

/** `managed/alpha_user/alice` -> `{ type: "managed/alpha_user", id: "alice" }`. */
export function splitResource(resource: string): { type: string; id?: string } {
  const parts = resource.split("/").filter((part) => part.length > 0);
  if (parts.length >= 3) {
    return { type: `${parts[0]}/${parts[1]}`, id: parts.slice(2).join("/") };
  }
  return { type: parts.join("/") };
}

/** Object name for a `managed/<name>` resource, or undefined for other containers. */
export function managedObjectName(resourceType: string): string | undefined {
  const [container, name] = resourceType.split("/");
  return container === "managed" && name !== undefined && name.length > 0
    ? name
    : undefined;
}

/**
 * Does this environment define the object type behind `resource`?
 *
 * This is the whole reason a profile is worth pulling. Without one, the harness
 * cannot tell "the tenant has this type and the record is simply absent"
 * (AIC returns `null`) from "this type does not exist and your fixture has a
 * typo" (AIC also returns `null`, unhelpfully). With one, the two become
 * distinguishable locally, and only the second is worth failing on.
 */
export function knowsType(profile: EnvProfile, resource: string): boolean {
  const { type } = splitResource(resource);
  const name = managedObjectName(type);
  return name !== undefined && Object.hasOwn(profile.objects, name);
}

export function objectFor(
  profile: EnvProfile,
  resource: string
): ObjectSchema | undefined {
  const name = managedObjectName(splitResource(resource).type);
  return name === undefined ? undefined : profile.objects[name];
}

/** Names a type that is not in the profile, listing near misses. */
export function unknownTypeError(
  profile: EnvProfile,
  resource: string
): SchemaViolation {
  const { type } = splitResource(resource);
  const name = managedObjectName(type);
  const known = Object.keys(profile.objects).sort();
  const near = name === undefined ? [] : known.filter((candidate) => close(candidate, name));
  const hint =
    near.length > 0
      ? ` Did you mean ${near.map((item) => `managed/${item}`).join(", ")}?`
      : ` This environment defines ${known.length} managed object${known.length === 1 ? "" : "s"}.`;
  return new SchemaViolation(
    "unknown-type",
    resource,
    `openidm: ${JSON.stringify(type)} is not a managed object in environment ${JSON.stringify(profile.tenant)}.${hint}`
  );
}

/** Cheap near-miss test: shared prefix, or one edit apart in length. */
function close(candidate: string, name: string): boolean {
  const a = candidate.toLowerCase();
  const b = name.toLowerCase();
  if (a === b) {
    return true;
  }
  if (a.startsWith(b) || b.startsWith(a)) {
    return true;
  }
  return Math.abs(a.length - b.length) <= 1 && sharedPrefix(a, b) >= a.length - 2;
}

function sharedPrefix(a: string, b: string): number {
  let index = 0;
  while (index < a.length && index < b.length && a[index] === b[index]) {
    index += 1;
  }
  return index;
}

/**
 * Strict check of a record's properties against the object schema.
 *
 * `mode` picks which half of the rule applies. A seeded fixture is checked for
 * property existence only — it stands in for a record the tenant already holds,
 * and demanding `required` of it would reject every partial fixture. A write is
 * checked for `required` too, because that is what AIC enforces on create.
 */
export function checkRecord(
  profile: EnvProfile,
  resource: string,
  record: JsonObject,
  mode: "seed" | "create" | "patch"
): void {
  const schema = objectFor(profile, resource);
  if (schema === undefined) {
    throw unknownTypeError(profile, resource);
  }
  for (const [key, value] of Object.entries(record)) {
    if (CREST_META.has(key)) {
      continue;
    }
    const property = schema.properties[key];
    if (property === undefined) {
      throw new SchemaViolation(
        "unknown-property",
        resource,
        `openidm: ${JSON.stringify(key)} is not a property of managed/${schema.name} in environment ${JSON.stringify(profile.tenant)}.`
      );
    }
    checkEnum(resource, schema.name, key, property.enum, value);
    if (property.items?.enum !== undefined && Array.isArray(value)) {
      for (const element of value) {
        checkEnum(resource, schema.name, `${key}[]`, property.items.enum, element);
      }
    }
  }
  if (mode === "create") {
    for (const key of schema.required) {
      if (!Object.hasOwn(record, key)) {
        throw new SchemaViolation(
          "missing-required",
          resource,
          `openidm: managed/${schema.name} requires ${JSON.stringify(key)}; AIC rejects a create without it.`
        );
      }
    }
  }
}

function checkEnum(
  resource: string,
  objectName: string,
  key: string,
  allowed: JsonValue[] | undefined,
  value: unknown
): void {
  if (allowed === undefined || value === null || value === undefined) {
    return;
  }
  if (allowed.some((candidate) => candidate === value)) {
    return;
  }
  throw new SchemaViolation(
    "bad-enum",
    resource,
    `openidm: ${JSON.stringify(value)} is not an allowed value for managed/${objectName}.${key} (allowed: ${allowed.map((item) => JSON.stringify(item)).join(", ")}).`
  );
}

/** True when `value` is a plain record we can check. */
export function isRecord(value: unknown): value is JsonObject {
  return isPlainObject(value);
}
