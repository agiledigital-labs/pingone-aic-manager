/**
 * Whether a schema `default` is one AIC accepts for the property it is declared on. One rule for
 * both lanes: the IDM emulator's config check (`idm/declarations.ts`) and the script tester's
 * profile table (`profile/write-guard.ts`) call this, so they cannot disagree about which
 * declaration is invalid.
 *
 * The rule, measured on the sandbox tenant 2026-11-08 (zzemu_def and zzemu_nd probes, REST and IDM
 * script; docs/api/10-managed-objects.md "Default validation"): **a default is invalid exactly when
 * it fails the declared type's `VALID_TYPE` check, and `null` always passes**.
 *
 * - The top-level `type` is checked against the default: `{"type": "boolean", "default": "nope"}`
 *   is invalid (also 2026-08 for the same case).
 * - `integer` admits no number: IDM's type check classifies every JSON number as `"number"`, so a
 *   record write of `3` or `3.5` is `VALID_TYPE {invalidType: "number"}`, and an `integer` default
 *   of `3` and of `3.5` both freeze the config. `number` accepts `3` and `3.5`.
 * - An array default is checked item by item against `items.type`: `["a", 2]` against
 *   `items: {type: "string"}` is invalid, `["a", "b"]` is valid. An `integer` item type therefore
 *   refuses every numeric item, by the same classification (the item case itself is not measured).
 * - An object default is checked only for being an object. Its declared `properties` are **not**
 *   checked: `{x: 1}` against `x: {type: "string"}` is valid and stored as given.
 * - `default: null` is valid on `string`, `boolean`, `number`, `integer`, `object` and `array`, and
 *   on `["string", "null"]`. A nullable spelling of the other types with a null default is valid by
 *   the same rule (null always passes) without a probe of its own.
 *
 * Unmeasured, so `"unmeasured"`: a `null` array item, arrays of arrays, and any default on a type
 * with no JSON shape here (`relationship`, a missing type, a type list that is not one type plus
 * `"null"`).
 *
 * What an invalid default does is not "that type is unavailable": IDM keeps serving the last good
 * `config/managed` for every type (measured, same probe). Callers act on that; see their headers.
 */
export interface DefaultDeclaration {
  readonly type?: unknown;
  readonly items?: unknown;
}

export type DefaultVerdict = "valid" | "invalid" | "unmeasured";

function isDocument(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/** One declared type: a plain string, or IDM's `["<type>", "null"]` nullable spelling. */
function declaredType(type: unknown): string | undefined {
  if (typeof type === "string") return type;
  if (!Array.isArray(type)) return undefined;
  const concrete = type.filter((name) => name !== "null");
  return concrete.length === 1 && type.length === 2 && typeof concrete[0] === "string" ? concrete[0] : undefined;
}

/**
 * A non-null value against a JSON type name, at one level: nothing under it is looked at. `type` is
 * one of the six measured types or it is unmeasured.
 */
function shapeMatches(value: unknown, type: string): DefaultVerdict {
  switch (type) {
    case "string":
    case "boolean":
      return typeof value === type ? "valid" : "invalid";
    case "number":
      return typeof value === "number" && Number.isFinite(value) ? "valid" : "invalid";
    case "integer":
      // Measured 2026-11-08 (zzemu_nd): no JSON value passes VALID_TYPE for `integer` but null.
      return "invalid";
    case "object":
      return isDocument(value) ? "valid" : "invalid";
    case "array":
      return Array.isArray(value) ? "valid" : "invalid";
    default:
      return "unmeasured";
  }
}

/** The types a `default: null` was measured valid on (zzemu_def, zzemu_nd; 2026-11-08). */
const MEASURED_NULL = new Set(["string", "boolean", "number", "integer", "object", "array"]);

export function judgeDefault(value: unknown, declaration: DefaultDeclaration): DefaultVerdict {
  const type = declaredType(declaration.type);
  if (type === undefined) return "unmeasured";
  if (value === null) return MEASURED_NULL.has(type) ? "valid" : "unmeasured";
  const top = shapeMatches(value, type);
  if (top !== "valid" || type !== "array" || !Array.isArray(value)) return top;
  const items = declaration.items;
  if (!isDocument(items) || items.type === undefined) return "valid";
  const itemType = declaredType(items.type);
  if (itemType === undefined || itemType === "array") return "unmeasured";
  let verdict: DefaultVerdict = "valid";
  for (const item of value) {
    const one = item === null ? "unmeasured" : shapeMatches(item, itemType);
    if (one === "invalid") return "invalid";
    if (one === "unmeasured") verdict = "unmeasured";
  }
  return verdict;
}
