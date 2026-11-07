/**
 * Which script-tester `openidm` writes the lane cannot answer faithfully (E15b).
 *
 * The script tester's `openidm.create/update/patch/delete` write `given.managed` directly: no hook
 * runs, no policy is evaluated. On AIC the same write runs the type's lifecycle hooks and
 * `policy.js`, so for a type whose profile declares either, a local pass says nothing about AIC
 * (an `alpha_user` create without `mail`/`givenName`/`sn` is a measured 403 there, docs/api/10).
 * This decides, per type and method, the fault such a write raises. The lane only looks the answer
 * up (`given.managedWriteFaults`), so the rule has one implementation, here.
 *
 * A type with neither hooks nor policies keeps writing silently: nothing about it is wrong. A
 * profile older than {@link PROFILE_VERSION} cannot tell, so every write on it faults until it is
 * re-pulled (or, if hand-written, brought up to the version by hand).
 *
 * Implicit schema policy is the one policy the lane does answer (P2-2, sol-r1b): AIC is measured to
 * apply a `default` on a create that omits the property, to refuse an explicit `null` on one, to
 * check a top-level property's `type` and nothing nested, and to accept a null on an undefaulted
 * property only when the schema's `required` array exists and omits it (docs/api/10).
 * {@link managedPolicy} gives the lane those per type as plain lists.
 */
import type { JsonValue } from "../case/types.ts";
import { defaultMatches } from "../declared-default.ts";
import { type EnvProfile, type ObjectSchema, PROFILE_VERSION, type PropertySchema } from "./types.ts";

export type WriteMethod = "create" | "update" | "patch" | "delete";
export const WRITE_METHODS: readonly WriteMethod[] = ["create", "update", "patch", "delete"];
export type WriteFault = { readonly id: string; readonly message: string };

/**
 * The hook phases a write runs, in the measured order (docs/api/24-idm-script-runtime.md,
 * "Managed-object hooks / Order"). An update or PATCH reads the record first, so its read hooks
 * come before `onUpdate`; property `onValidate` hooks (every property, schema order) run before the
 * object's `onValidate`, and property `onStore` hooks before the object's `onStore`. A create runs
 * no read hooks and a delete runs no property hooks. A hook name these phases do not know counts
 * for every method, after the known ones.
 */
type Phase = { readonly object: string } | { readonly property: string };
const o = (object: string): Phase => ({ object });
const p = (property: string): Phase => ({ property });
const PHASES: Record<"create" | "update" | "delete", readonly Phase[]> = {
  create: [o("onCreate"), p("onValidate"), o("onValidate"), p("onStore"), o("onStore"), o("postCreate"), o("onSync")],
  update: [o("onRetrieve"), p("onRetrieve"), o("onRead"),
    o("onUpdate"), p("onValidate"), o("onValidate"), p("onStore"), o("onStore"), o("postUpdate"), o("onSync")],
  delete: [o("onDelete"), o("postDelete"), o("onSync")],
};
const KNOWN_OBJECT = new Set(Object.values(PHASES).flat().flatMap((phase) => "object" in phase ? [phase.object] : []));
const KNOWN_PROPERTY = new Set(Object.values(PHASES).flat().flatMap((phase) => "property" in phase ? [phase.property] : []));

function propertyHooks(object: ObjectSchema, wanted: (hook: string) => boolean): string[] {
  const out: string[] = [];
  for (const [name, property] of Object.entries(object.properties)) {
    for (const hook of property.hooks ?? []) if (wanted(hook)) out.push(`${name}.${hook}`);
  }
  return out;
}

/** The hooks a write of `method` would run on AIC, first one first. */
export function hooksRun(object: ObjectSchema, method: WriteMethod): string[] {
  const declared = object.hooks ?? [];
  const known = PHASES[method === "patch" ? "update" : method].flatMap((phase) => "object" in phase
    ? declared.filter((hook) => hook === phase.object)
    : propertyHooks(object, (hook) => hook === phase.property));
  return [
    ...known,
    ...propertyHooks(object, (hook) => !KNOWN_PROPERTY.has(hook)),
    ...declared.filter((hook) => !KNOWN_OBJECT.has(hook)),
  ];
}

function propertyPolicies(path: string, property: PropertySchema): string[] {
  const own = [
    ...(property.enum ? [`${path} enum`] : []),
    ...(property.policies ?? []).map((id) => `${path} ${id}`),
    ...(property.conditionalPolicies ?? []).map((id) => `${path} ${id} (conditional)`),
    ...(property.fallbackPolicies ?? []).map((id) => `${path} ${id} (fallback)`),
  ];
  return property.items ? [...own, ...propertyPolicies(`${path}[]`, property.items)] : own;
}

/** The declared policies a create or update is checked against on AIC. */
export function policiesDeclared(object: ObjectSchema): string[] {
  return [
    ...(object.required.length > 0 ? [`required ${object.required.join(", ")}`] : []),
    ...Object.entries(object.properties).flatMap(([name, property]) => propertyPolicies(name, property)),
  ];
}

/**
 * A profile older than this package's format cannot say a type declares nothing. Only an integer
 * version at or above {@link PROFILE_VERSION} is current: a hand-written `"2"` or `"v2"` is not the
 * format a pull writes, and reading it as current would let a profile without `hooks` pass every
 * write on a hooked type.
 */
export function profileStale(profile: EnvProfile): boolean {
  const version: unknown = profile.profileVersion;
  return !(Number.isInteger(version) && (version as number) >= PROFILE_VERSION);
}

function staleFault(object: ObjectSchema, profile: EnvProfile): WriteFault {
  return { id: "openidm-managed-profile-stale", message: `managed/${object.name}: the environment profile is format version ` +
    `${JSON.stringify(profile.profileVersion ?? 1)}, not the integer ${PROFILE_VERSION} or later, so it cannot say whether this ` +
    "write runs hooks or policies on AIC. Re-pull it: aic-script-tester-pull-profile. A hand-written profile cannot be " +
    "re-pulled: give each type `hooks` (its object hook names, [] for none) and `noRequiredArray: true` when its schema " +
    "has no `required` array, give each property its `default`, `conditionalPolicies` and " +
    `\`fallbackPolicies\` where the tenant declares them, then set "profileVersion": ${PROFILE_VERSION}` };
}

/** The fault a write of `method` raises on a type of a current-version profile, if any. */
export function writeFault(object: ObjectSchema, method: WriteMethod): WriteFault | undefined {
  const type = `managed/${object.name}`;
  const remedy = "This lane runs no hooks and evaluates no policy, so the local result is not what AIC does. " +
    "Stub the call with given.openidmFailures if the test means a refusal, or expect the fault (E19)";
  const hooks = hooksRun(object, method);
  if (hooks.length > 0) {
    return { id: `openidm-managed-hook/${object.name}.${hooks[0]}`,
      message: `${type}: on AIC a ${method} runs ${hooks.join(", ")}. ${remedy}` };
  }
  const policies = method === "delete" ? [] : policiesDeclared(object);
  if (policies.length > 0) {
    return { id: `openidm-managed-policy/${object.name}`,
      message: `${type}: on AIC a ${method} is checked against ${policies.join("; ")}. ${remedy}` };
  }
  return undefined;
}

/**
 * Every type of the profile, named by its map key: the key is what declares the type exists, so
 * every table is keyed the same way. `checkProfileObjects` refuses a profile where they disagree
 * before any table is built; this keeps the tables from depending on that check alone.
 */
function byKey(profile: EnvProfile): ObjectSchema[] {
  return Object.entries(profile.objects).map(([key, object]) => object.name === key ? object : { ...object, name: key });
}

/** The lane's lookup table: `managed/<type>` → method → fault, for every type that has one. */
export function managedWriteFaults(profile: EnvProfile): Record<string, Partial<Record<WriteMethod, WriteFault>>> {
  const out: Record<string, Partial<Record<WriteMethod, WriteFault>>> = {};
  const stale = profileStale(profile);
  for (const object of byKey(profile)) {
    const faults: Partial<Record<WriteMethod, WriteFault>> = {};
    for (const method of WRITE_METHODS) {
      const fault = stale ? staleFault(object, profile) : writeFault(object, method);
      if (fault) faults[method] = fault;
    }
    if (Object.keys(faults).length > 0) out[`managed/${object.name}`] = faults;
  }
  return out;
}

/** Declared types a top-level mismatch was measured for (docs/api/10, 2026-11-08 probe). */
const MEASURED_TYPES = new Set(["string", "object", "array"]);
/** Declared types whose mismatch is unmeasured: the lane faults one. `relationship`/`any` stay unchecked. */
const UNMEASURED_TYPES = new Set(["number", "integer", "boolean"]);

/**
 * What the lane needs to answer a type's implicit policies (P2-2, sol-r1b), as plain lists. IDM's
 * `policy.js` checks only the top-level property's `type`: schemas under `items`/`properties` are
 * stored and not enforced (measured 2026-11-08, docs/api/10). Every field is about top-level
 * properties:
 * - `defaults`: every declared non-null `default`, to fill in on a create that omits the property
 *   (measured for scalars and arrays). A `null` default is not applied. The lane never reaches one:
 *   `null` matches no declared type, so {@link managedUnavailable} faults every call on such a type
 *   first. The entries stay so this table does not depend on that ordering.
 * - `notNull`: the properties with a non-null default, on which an explicit `null` is `NOT_NULL`.
 * - `nullRefused`: the properties an explicit `null` on a create of a new record is refused on:
 *   `notNull`, plus every typed property when the schema has no `required` array (`VALID_TYPE`,
 *   `invalidType "null"`). Measured. Any other typed property's null is accepted and stored, because
 *   a required-named property never gets here (`writeFault` faults the type's writes).
 * - `nullUnmeasured`: a nullable (`["string","null"]`) property, whose null nothing measured, and,
 *   when the schema has no `required` array, a property of a type the rule does not know
 *   (`relationship`, `any`). With a `required` array those keep writing as before (gaps doc).
 * - `checked`: property → declared type, for `string`/`object`/`array`, where a mismatching
 *   top-level value on a create of a new record is a measured `403 VALID_TYPE`.
 * - `unmeasured`: property → declared type, for `number`/`integer`/`boolean`; a mismatch faults.
 * - `itemTypes`: array property → its `items.type`. An item of another type faults: nested `items`
 *   were measured unenforced, a top-level array's own items were not tested.
 * - `defaulted`: every property that declares a `default`, `null` included. Stock `policy.js` adds
 *   an implicit `required` for any default, so a PATCH `remove` of one is checked on AIC; the
 *   measured 403 `REQUIRED` had the property in `schema.required` too, so the lane faults the rest.
 *   Removing any other (non-required) property is a measured 200.
 *
 * The lane decides only what it alone knows: the method and whether the record exists.
 */
export type PolicyRule = {
  readonly defaults: Record<string, JsonValue>;
  readonly notNull: readonly string[];
  readonly nullRefused: readonly string[];
  readonly nullUnmeasured: readonly string[];
  readonly checked: Record<string, string>;
  readonly unmeasured: Record<string, string>;
  readonly itemTypes: Record<string, string>;
  readonly defaulted: readonly string[];
};

/** The lane's implicit-policy table: `managed/<type>` → rule, for every type of a current profile. */
export function managedPolicy(profile: EnvProfile): Record<string, PolicyRule> {
  const out: Record<string, PolicyRule> = {};
  if (profileStale(profile)) return out;
  for (const object of byKey(profile)) {
    const defaults: Record<string, JsonValue> = {};
    const defaulted: string[] = [];
    const nullUnmeasured: string[] = [];
    const checked: Record<string, string> = {};
    const unmeasured: Record<string, string> = {};
    const itemTypes: Record<string, string> = {};
    for (const [name, property] of Object.entries(object.properties)) {
      if (property.default !== undefined) {
        defaulted.push(name);
        if (property.default !== null) defaults[name] = property.default;
      }
      const known = MEASURED_TYPES.has(property.type) || UNMEASURED_TYPES.has(property.type);
      if (property.nullable || (object.noRequiredArray && !known)) nullUnmeasured.push(name);
      else if (MEASURED_TYPES.has(property.type)) checked[name] = property.type;
      if (UNMEASURED_TYPES.has(property.type)) unmeasured[name] = property.type;
      const item = property.items?.type;
      if (property.type === "array" && item !== undefined && (MEASURED_TYPES.has(item) || UNMEASURED_TYPES.has(item))) itemTypes[name] = item;
    }
    const notNull = Object.keys(defaults);
    const typed = Object.keys(object.properties).filter((name) => !nullUnmeasured.includes(name));
    const nullRefused = object.noRequiredArray ? typed : notNull.filter((name) => !nullUnmeasured.includes(name));
    out[`managed/${object.name}`] = { defaults, notNull, nullRefused, nullUnmeasured, checked, unmeasured, itemTypes, defaulted };
  }
  return out;
}

/**
 * Why a property's declaration makes its type unavailable, if it does (sol-r1 P1-1): a `default`
 * that does not match the declared type ({@link defaultMatches}, the IDM emulator's rule), or a
 * `default` nested inside `items`, which the emulator refuses too and nothing measured.
 */
function declarationProblem(name: string, property: PropertySchema, depth = 0): string | undefined {
  if (property.default !== undefined) {
    if (depth > 0) return `${name} declares a nested default, which nothing measured`;
    if (!defaultMatches(property.default, property)) {
      return `${name} declares default ${JSON.stringify(property.default)}, which does not match its type ` +
        `${JSON.stringify(property.type)}${property.nullable ? " (a nullable type is judged by its non-null type)" : ""}`;
    }
  }
  return property.items ? declarationProblem(`${name}[]`, property.items, depth + 1) : undefined;
}

/**
 * The lane's unavailable-type table: `managed/<type>` → the fault every `openidm` call on it raises.
 *
 * Measured (docs/api/10): a config with `{"type": "boolean", "default": "nope"}` is accepted, and
 * the type then answers 404 to every `/openidm/managed/<type>` call, indefinitely. What a script's
 * `openidm.read`/`create`/… returns for such a type is not measured (a type the config does not
 * declare at all reads as `null`, but that is a different tenant state), so the lane faults every
 * call rather than emulate an answer. Never repaired: the profile is used as written. Judged on any
 * profile version, because a default is in the profile or it is not.
 */
export function managedUnavailable(profile: EnvProfile): Record<string, WriteFault> {
  const out: Record<string, WriteFault> = {};
  for (const object of byKey(profile)) {
    for (const [name, property] of Object.entries(object.properties)) {
      const problem = declarationProblem(name, property);
      if (problem === undefined) continue;
      out[`managed/${object.name}`] = { id: `openidm-managed-invalid-default/${object.name}.${name}`,
        message: `managed/${object.name}: ${problem}. On AIC a type-mismatched default leaves the type unavailable ` +
          "(every record call 404, measured for a boolean declaring default \"nope\", docs/api/10), and what an openidm " +
          "call answers then is unmeasured. Fix the default on the tenant and re-pull the profile" };
      break;
    }
  }
  return out;
}
