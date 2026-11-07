/**
 * IDM's implicit schema policies, for a write the caller supplied no `PolicyEvaluator` for (E15a).
 *
 * On AIC every managed write runs `policy.js`, which builds implicit policies from the schema:
 * `not-null` for a property with a non-null `default`, `valid-type` from `type`, and the explicit
 * `required`/`enum`/`policies` that `checkCapabilities` already refuses without an evaluator. The
 * emulator does not run `policy.js` (it is not supplied, and it reads `config/policy`, which the
 * emulator does not take), so this module decides only what was measured and faults the rest:
 *
 * - **Measured, emulated:** a create that sends an explicit `null` for a property with a non-null
 *   `default` is `403 Policy validation failed`, `NOT_NULL` on that property, whether or not it is
 *   required (docs/api/10-managed-objects.md, 2026-08-05 and 2026-11-04). Both measurements used a
 *   defaulted property; stock `policy.js` adds `not-null` only there, which is why the default is
 *   part of the rule. The detail's single-requirement shape is the stock reading (no `valid-type`
 *   entry when `schema.required` is an array not naming the property), consistent with doc 10's
 *   "+ `VALID_TYPE` when the property is also required".
 * - **Faulted:** a PATCH removing a property that declares a `default` (`idm-policy/required`:
 *   stock adds an implicit `required` for any default; the measured 403 had `schema.required` name
 *   the property too), every other null on a defaulted property (`idm-policy/not-null`), a null on an
 *   undefaulted property when the schema has no `required` array, a value whose JSON type is not
 *   the declared `type`, a nested declared property of the wrong type, or a value for an untyped
 *   property (`idm-policy/valid-type`), and an array item of the wrong `items.type`
 *   (`idm-policy/valid-array-items`). None of these was measured on AIC.
 * - **Assumed:** everything else, including a null on an undefaulted optional property when the
 *   schema has a `required` array, which stock `policy.js`'s `validatePropertyType` accepts, and
 *   removing an undefaulted optional property, which no stock policy refuses (unmeasured on AIC).
 *
 * Pure: the caller decides what an outcome does to the request.
 */
import { isDeepStrictEqual } from "node:util";
import type { JsonValue } from "../protocol.ts";
import { isDocument } from "./document.ts";
import { pointerParts } from "./filter.ts";
import type { Document, ManagedType, PropertyConfig } from "./types.ts";

export type PolicyFaultId = "idm-policy/not-null" | "idm-policy/valid-type" | "idm-policy/valid-array-items" | "idm-policy/required";

export type ImplicitPolicyVerdict =
  | { readonly kind: "assumed" }
  /** The measured refusal: emulate it with {@link notNullRefusal}. */
  | { readonly kind: "notNull"; readonly property: string }
  | { readonly kind: "fault"; readonly id: PolicyFaultId; readonly reason: string };

export type PolicyWrite = {
  /** `create` only when no record exists at the id: an upsert over one is not the measured case. */
  readonly creating: boolean;
  /** The PATCH's previous record. PATCH policy covers only what it writes (doc 10, enum table). */
  readonly patchedFrom?: Document;
  /** The PATCH's operations, for the properties they name even where the value is unchanged. */
  readonly operations?: JsonValue;
};

export const ASSUMED_POLICY = "policy.js not run: no config/policy resource policies apply to this write; " +
  "the implicit not-null, valid-type and array-item rules were checked locally";

/**
 * The `NOT_NULL` refusal body. Measured: the envelope (doc 10 shows it verbatim for `REQUIRED`) and
 * that the property's requirements include `NOT_NULL`. The single-requirement shape (nothing else
 * listed) is the stock `policy.js` reading, not a measurement.
 */
export function notNullDetail(property: string): Document {
  return { result: false, failedPolicyRequirements: [
    { policyRequirements: [{ policyRequirement: "NOT_NULL" }], property },
  ] };
}

function jsonType(value: JsonValue): string {
  if (value === null) return "null";
  if (Array.isArray(value)) return "array";
  return typeof value;
}

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

function typeFault(path: string, config: PropertyConfig, value: JsonValue): string | undefined {
  if (typeof config.type !== "string") {
    return `${path} declares no type, and stock policy.js's implicit builder reads every property's type; what AIC does is unmeasured`;
  }
  const actual = jsonType(value);
  if (actual !== "null" && actual !== config.type) {
    return `${path} is declared ${config.type} and the write sends ${actual}; AIC's valid-type refusal is unmeasured`;
  }
  if (actual === "object" && isDocument(config.properties) && isDocument(value)) {
    for (const [name, child] of Object.entries(config.properties)) {
      if (!Object.hasOwn(value, name) || !isDocument(child)) continue;
      const nested = value[name] as JsonValue;
      if (nested === null) return `${path}/${name} is null inside an object property; nested null handling is unmeasured`;
      const fault = typeFault(`${path}/${name}`, child as PropertyConfig, nested);
      if (fault) return fault;
    }
  }
  return undefined;
}

function itemFault(path: string, config: PropertyConfig, value: JsonValue): string | undefined {
  if (!Array.isArray(value) || !isDocument(config.items) || typeof config.items.type !== "string") return undefined;
  const expected = config.items.type;
  const index = value.findIndex((item) => jsonType(item) !== expected);
  if (index < 0) return undefined;
  return `${path}[${index}] is ${jsonType(value[index] as JsonValue)} and items are declared ${expected}; ` +
    "AIC's array-item check is unmeasured (stock policy.js adds no implicit one)";
}

export function implicitPolicy(type: ManagedType, object: Document, write: PolicyWrite): ImplicitPolicyVerdict {
  const required = type.schema.required;
  const nulls: string[] = [];
  for (const name of written(type, object, write)) {
    const config = type.schema.properties[name] ?? {};
    const path = `${type.name}.${name}`;
    if (!Object.hasOwn(object, name)) {
      // Absent after the write: a PATCH removed it. Stock valid-type accepts an absent value, so no
      // type check applies. A required-named property never gets here (the capability gate faults
      // it). An undefaulted optional one has no policy that could refuse it: assumed.
      if (Object.hasOwn(config, "default")) {
        return { kind: "fault", id: "idm-policy/required", reason: `${path} declares a default and the write removes it. ` +
          "Stock policy.js adds an implicit required for any default, null included; AIC's 403 REQUIRED for a PATCH remove " +
          "is measured only with the property also named in schema.required (docs/api/10-managed-objects.md), so what AIC " +
          "answers here is unmeasured" };
      }
      continue;
    }
    const value = object[name] as JsonValue;
    if (value === null) {
      const defaulted = Object.hasOwn(config, "default") && config.default !== null;
      if (typeof config.type !== "string") {
        return { kind: "fault", id: "idm-policy/valid-type", reason: typeFault(path, config, value)! };
      }
      if (!defaulted) {
        if (Array.isArray(required)) continue;
        return { kind: "fault", id: "idm-policy/valid-type", reason: `${path} is null, has no default, and the schema has no required array, ` +
          "so stock policy.js's valid-type would refuse it; AIC's refusal is unmeasured" };
      }
      if (!write.creating || write.patchedFrom !== undefined || !Array.isArray(required)) {
        return { kind: "fault", id: "idm-policy/not-null", reason: `${path} has a default and the write sends null. AIC's NOT_NULL ` +
          `is measured only on a create of a new record${Array.isArray(required) ? "" : " whose schema has a required array"}, and a ` +
          "PATCH replace with null is a measured 400 refused before policy (docs/api/10-managed-objects.md); this write is neither, " +
          "so what AIC does with it is unmeasured" };
      }
      nulls.push(name);
      continue;
    }
    const fault = typeFault(path, config, value);
    if (fault) return { kind: "fault", id: "idm-policy/valid-type", reason: fault };
    const items = itemFault(path, config, value);
    if (items) return { kind: "fault", id: "idm-policy/valid-array-items", reason: items };
  }
  if (nulls.length > 1) {
    return { kind: "fault", id: "idm-policy/not-null", reason: `${type.name} create sends null for ${nulls.join(", ")}; ` +
      "NOT_NULL is measured on one property, and the order AIC lists several in is unmeasured" };
  }
  if (nulls.length === 1) return { kind: "notNull", property: nulls[0]! };
  return { kind: "assumed" };
}
