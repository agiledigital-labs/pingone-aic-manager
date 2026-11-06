import { isPlainObject } from "../case/util.ts";
import type { JsonValue } from "../case/types.ts";
import { type EnvProfile, type ObjectSchema, PROFILE_VERSION, type PropertySchema } from "./types.ts";

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
    out[entry.name] = normaliseObject(entry.name, entry);
  }
  return {
    profileVersion: PROFILE_VERSION,
    tenant: meta.tenant,
    pulledAt: meta.pulledAt,
    sources: [meta.endpoint],
    objects: out,
  };
}

function normaliseObject(name: string, entry: Record<string, unknown>): ObjectSchema {
  const schema = entry.schema;
  const hooks = hookNames(entry);
  if (!isPlainObject(schema)) {
    // An object with no schema block is real (some shipped types carry none).
    // Record it as existing with no properties rather than dropping it: type
    // EXISTENCE is what distinguishes an AIC `null` from a fixture typo, and
    // that distinction must not depend on how richly the type is described.
    return { name, properties: {}, required: [], hooks };
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
  return { name, properties, required, ...(Array.isArray(schema.required) ? {} : { noRequiredArray: true as const }), hooks };
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
  const hooks = hookNames(value);
  if (hooks.length > 0) {
    out.hooks = hooks;
  }
  if (Array.isArray(value.policies) && value.policies.length > 0) {
    out.policies = policyIds(value.policies);
  }
  if (Object.hasOwn(value, "default")) {
    out.default = value.default as JsonValue;
  }
  if (Array.isArray(value.conditionalPolicies) && value.conditionalPolicies.length > 0) {
    // An entry whose policies are unreadable still makes the property conditionally policed.
    out.conditionalPolicies = value.conditionalPolicies.flatMap((entry) =>
      isPlainObject(entry) && Array.isArray(entry.policies) && entry.policies.length > 0 ? policyIds(entry.policies) : ["?"]
    );
  }
  if (Array.isArray(value.fallbackPolicies) && value.fallbackPolicies.length > 0) {
    out.fallbackPolicies = policyIds(value.fallbackPolicies);
  }
  return out;
}

/** A policy entry without a string policyId is still a policy AIC runs. */
function policyIds(policies: unknown[]): string[] {
  return policies.map((policy) => isPlainObject(policy) && typeof policy.policyId === "string" ? policy.policyId : "?");
}

/**
 * Script hook names on a managed object or property, without their bodies. Any `on…`/`post…` key
 * holding an object counts, so a hook name this harness does not know is recorded rather than
 * dropped — the script tester faults writes on it instead of passing them (E15b).
 */
function hookNames(config: Record<string, unknown>): string[] {
  return Object.keys(config)
    .filter((key) => /^(on|post)[A-Z]\w*$/.test(key) && isPlainObject(config[key]))
    .sort();
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
