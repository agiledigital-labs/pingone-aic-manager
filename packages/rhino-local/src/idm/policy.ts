/**
 * IDM's implicit schema policies, for a write the caller supplied no `PolicyEvaluator` for (E15a).
 *
 * On AIC every managed write runs `policy.js`, which builds implicit policies from the schema:
 * `not-null` for a property with a non-null `default`, `valid-type` from `type`, and the explicit
 * `required`/`enum`/`policies` that `checkCapabilities` already refuses without an evaluator. The
 * emulator does not run `policy.js` (it is not supplied, and it reads `config/policy`, which the
 * emulator does not take), so this module emulates what was measured and faults the rest.
 *
 * Measured on a create of a new record (docs/api/10-managed-objects.md; the 2026-11-08 probe over
 * REST and from an IDM script, plus 2026-08-05 and 2026-11-04 for `NOT_NULL`):
 *
 * - **Only the top-level property's `type` is checked.** A top-level mismatch is
 *   `403 Policy validation failed`, `VALID_TYPE` with `{invalidType, validTypes:[type]}`, measured
 *   for declared `string`, `object` and `array`. Schemas under `properties` and `items` are stored
 *   and **not** enforced: `{box:{tags:[2]}}` and `{rows:[{inner:"bad"}]}` were 201, stored as sent.
 * - **A null passes the type check only when `schema.required` exists and does not name the
 *   property.** `required: []` (or a list omitting it) and an undefaulted property: 201, stored and
 *   read back as `null`. No `required` key: `VALID_TYPE` with `invalidType "null"`.
 * - **A null on a property with a non-null `default` is `NOT_NULL`**, plus a separate `VALID_TYPE`
 *   entry, after it, when the null fails the type check too. Measured only on defaulted properties
 *   (the probe's was a boolean); whether `NOT_NULL` keys on the default or on the type is not
 *   separated, so a property gets it here only with a default.
 * - **A PATCH `remove` of an undefaulted, non-required property is accepted** (200), the key gone.
 *
 * Faulted, because unmeasured: a mismatch against any other declared type (`number`, `boolean`,
 * `integer`, `relationship`, …) or on any write but a new record's create (`idm-policy/valid-type`);
 * a value or null for a property with no `type`; a null on an undefaulted property outside a create
 * when the schema has no `required` array; any defaulted null outside a create
 * (`idm-policy/not-null`); a refusal naming more than one property, whose order is unmeasured; a
 * PATCH removing a property that declares a `default` (`idm-policy/required`); and an item of a
 * top-level array whose JSON type is not `items.type` (`idm-policy/valid-array-items`): only
 * *nested* `items` were measured unenforced, never a top-level array's.
 *
 * Assumed, with its reason: a null on an undefaulted property outside a create, when the schema has
 * a `required` array not naming it, is accepted as on create (the probe measured create only).
 *
 * Pure: the caller decides what an outcome does to the request.
 */
import { isDeepStrictEqual } from "node:util";
import type { JsonValue } from "../protocol.ts";
import { isDocument } from "./document.ts";
import { pointerParts } from "./filter.ts";
import type { Document, ManagedType, PropertyConfig } from "./types.ts";

export type PolicyFaultId = "idm-policy/not-null" | "idm-policy/valid-type" | "idm-policy/valid-array-items" | "idm-policy/required";

/** One `failedPolicyRequirements` entry, keys in the order AIC serialises them. */
export type FailedRequirement = {
  readonly policyRequirements: readonly [
    { readonly params?: { readonly invalidType: string; readonly validTypes: readonly string[] }; readonly policyRequirement: "NOT_NULL" | "VALID_TYPE" },
  ];
  readonly property: string;
};

export type ImplicitPolicyVerdict =
  /** The write passes. `measured`/`assumed` name the policy outcomes it relied on. */
  | { readonly kind: "pass"; readonly measured: readonly string[]; readonly assumed: readonly string[] }
  /** The measured refusal: emulate it with {@link policyRefusalDetail}. */
  | { readonly kind: "refuse"; readonly failed: readonly FailedRequirement[]; readonly measured: readonly string[] }
  | { readonly kind: "fault"; readonly id: PolicyFaultId; readonly reason: string };

export type PolicyWrite = {
  /** `create` only when no record exists at the id: an upsert over one is not the measured case. */
  readonly creating: boolean;
  /** The PATCH's previous record. PATCH policy covers only what it writes (doc 10, enum table). */
  readonly patchedFrom?: Document;
  /** The PATCH's operations, for the properties they name even where the value is unchanged. */
  readonly operations?: JsonValue;
};

/** True: with no evaluator, `checkCapabilities` has already refused every explicit policy. */
export const ASSUMED_POLICY = "policy.js not run: no config/policy resource policies apply to this write";

/** Evidence lines, one per measured outcome (source: {@link POLICY_SOURCE}). */
export const POLICY_MEASURED = {
  topLevelType: "policy checks a top-level property's type on create: a mismatch is 403 VALID_TYPE",
  nestedUnchecked: "schemas under properties and items are stored and not enforced",
  nullAccepted: "a null on an undefaulted property is accepted and stored as null when schema.required exists and omits it",
  nullRefused: "a null fails VALID_TYPE when the schema has no required array",
  notNull: "explicit null on a defaulted property is refused NOT_NULL on create",
  optionalRemove: "PATCH remove of an undefaulted, non-required property is accepted and the key removed",
} as const;
export const POLICY_SOURCE = "docs/api/10-managed-objects.md";

/** Declared types a top-level mismatch was measured for (2026-11-08 probe). */
const MEASURED_TYPES = new Set(["string", "object", "array"]);

/** The refusal body's `detail`: the measured envelope, one entry per failing requirement. */
export function policyRefusalDetail(failed: readonly FailedRequirement[]): Document {
  return { result: false, failedPolicyRequirements: failed as unknown as JsonValue[] };
}

function jsonType(value: JsonValue): string {
  if (value === null) return "null";
  if (Array.isArray(value)) return "array";
  return typeof value;
}

const requirement = (property: string, policyRequirement: "NOT_NULL" | "VALID_TYPE", invalidType?: string, declared?: string): FailedRequirement =>
  ({ policyRequirements: [policyRequirement === "VALID_TYPE"
    ? { params: { invalidType: invalidType!, validTypes: [declared!] }, policyRequirement }
    : { policyRequirement }], property });

/** Top-level properties this write submits to policy, in schema order. */
function written(type: ManagedType, object: Document, write: PolicyWrite): string[] {
  const declared = Object.keys(type.schema.properties);
  if (write.patchedFrom === undefined) return declared.filter((name) => Object.hasOwn(object, name));
  const named = new Set<string>();
  if (Array.isArray(write.operations)) for (const op of write.operations) {
    if (isDocument(op) && typeof op.field === "string") {
      named.add(op.field.startsWith("/") ? pointerParts(op.field)[0] ?? "" : op.field);
    }
  }
  const before = write.patchedFrom;
  return declared.filter((name) => named.has(name) ||
    Object.hasOwn(object, name) !== Object.hasOwn(before, name) || !isDeepStrictEqual(object[name], before[name]));
}

/** The probe sent wrong-typed nested values, never a nested null; it is accepted on the same rule. */
export const NESTED_NULL = "a null nested in an object or array property is not enforced either (the probe sent wrong-typed nested values, not a null)";

function holdsNull(value: JsonValue): boolean {
  if (value === null) return true;
  if (Array.isArray(value)) return value.some(holdsNull);
  return isDocument(value) && Object.values(value).some((child) => holdsNull(child as JsonValue));
}

function itemFault(path: string, config: PropertyConfig, value: JsonValue): string | undefined {
  if (!Array.isArray(value) || !isDocument(config.items) || typeof config.items.type !== "string") return undefined;
  const expected = config.items.type;
  const index = value.findIndex((item) => jsonType(item) !== expected);
  if (index < 0) return undefined;
  return `${path}[${index}] is ${jsonType(value[index] as JsonValue)} and items are declared ${expected}. Nested items ` +
    "schemas were measured unenforced, but a top-level array's items were not tested, so what AIC does here is unmeasured";
}

export function implicitPolicy(type: ManagedType, object: Document, write: PolicyWrite): ImplicitPolicyVerdict {
  const hasRequired = Array.isArray(type.schema.required);
  const newRecord = write.creating && write.patchedFrom === undefined;
  const failed: FailedRequirement[] = [];
  const measured = new Set<string>();
  const assumed = new Set<string>();
  const fault = (id: PolicyFaultId, reason: string): ImplicitPolicyVerdict => ({ kind: "fault", id, reason });
  for (const name of written(type, object, write)) {
    const config = type.schema.properties[name] ?? {};
    const path = `${type.name}.${name}`;
    if (!Object.hasOwn(object, name)) {
      // Absent after the write: a PATCH removed it. A required-named property never gets here (the
      // capability gate faults it).
      if (Object.hasOwn(config, "default")) {
        return fault("idm-policy/required", `${path} declares a default and the write removes it. ` +
          "Stock policy.js adds an implicit required for any default, null included; AIC's 403 REQUIRED for a PATCH remove " +
          "is measured only with the property also named in schema.required (docs/api/10-managed-objects.md), so what AIC " +
          "answers here is unmeasured");
      }
      measured.add(POLICY_MEASURED.optionalRemove);
      continue;
    }
    const value = object[name] as JsonValue;
    if (typeof config.type !== "string") {
      return fault("idm-policy/valid-type", `${path} declares no type, and stock policy.js's implicit builder reads every ` +
        "property's type; what AIC does is unmeasured");
    }
    if (value === null) {
      const defaulted = Object.hasOwn(config, "default") && config.default !== null;
      if (!newRecord) {
        if (defaulted) {
          return fault("idm-policy/not-null", `${path} has a default and the write sends null. AIC's NOT_NULL is measured only ` +
            "on a create of a new record, and a PATCH replace with null is a measured 400 refused before policy " +
            "(docs/api/10-managed-objects.md); this write is neither, so what AIC does with it is unmeasured");
        }
        if (!hasRequired) {
          return fault("idm-policy/valid-type", `${path} is null, has no default, and the schema has no required array. ` +
            "That is a measured VALID_TYPE refusal on a create of a new record (docs/api/10-managed-objects.md); this write " +
            "is not one, so what AIC does with it is unmeasured");
        }
        assumed.add("a null on an undefaulted property not in a present required array is accepted outside a create, " +
          "as measured on create");
        continue;
      }
      if (defaulted) {
        measured.add(POLICY_MEASURED.notNull);
        failed.push(requirement(name, "NOT_NULL"));
      }
      if (hasRequired) {
        if (!defaulted) measured.add(POLICY_MEASURED.nullAccepted);
      } else {
        measured.add(POLICY_MEASURED.nullRefused);
        failed.push(requirement(name, "VALID_TYPE", "null", config.type));
      }
      continue;
    }
    const actual = jsonType(value);
    if (actual !== config.type) {
      if (!newRecord || !MEASURED_TYPES.has(config.type)) {
        return fault("idm-policy/valid-type", `${path} is declared ${config.type} and the write sends ${actual}. AIC's ` +
          "VALID_TYPE refusal is measured on a create of a new record for declared string, object and array " +
          "(docs/api/10-managed-objects.md); this write is not that case, so what AIC does is unmeasured");
      }
      measured.add(POLICY_MEASURED.topLevelType);
      failed.push(requirement(name, "VALID_TYPE", actual, config.type));
      continue;
    }
    const items = itemFault(path, config, value);
    if (items) return fault("idm-policy/valid-array-items", items);
    if ((actual === "object" && isDocument(config.properties)) || (actual === "array" && isDocument(config.items))) {
      measured.add(POLICY_MEASURED.nestedUnchecked);
      if (holdsNull(value)) assumed.add(NESTED_NULL);
    }
  }
  const properties = [...new Set(failed.map((entry) => entry.property))];
  if (properties.length > 1) {
    return fault(failed.some((entry) => entry.policyRequirements[0].policyRequirement === "NOT_NULL") ? "idm-policy/not-null" : "idm-policy/valid-type",
      `${type.name} create fails policy on ${properties.join(", ")}; each refusal is measured on one property, and the ` +
      "order AIC lists several in is unmeasured");
  }
  if (failed.length > 0) return { kind: "refuse", failed, measured: [...measured] };
  return { kind: "pass", measured: [...measured], assumed: [...assumed] };
}
