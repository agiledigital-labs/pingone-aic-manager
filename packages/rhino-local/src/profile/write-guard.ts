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
 * apply a `default` on a create that omits the property (`null` included; an array's null becomes
 * `[]`), to refuse an explicit `null` on a property with a non-null one, to check a top-level
 * property's `type` and nothing nested, and to accept a null on an undefaulted property only when
 * the schema's `required` array exists and omits it (docs/api/10).
 * {@link managedPolicy} gives the lane those per type as plain lists.
 */
import type { JsonValue } from "../case/types.ts";
import { judgeDefault } from "../declared-default.ts";
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
    "`fallbackPolicies` where the tenant declares them, an object property its normalised `properties`, and a property " +
    "whose type list is not one type plus \"null\" `type: \"any\"` with the list verbatim as `types`, then set " +
    `"profileVersion": ${PROFILE_VERSION}` };
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

/**
 * Declared types a top-level mismatch was measured for (docs/api/10): `string`/`object`/`array`
 * (zzemu_pol) and `boolean`/`number`/`integer` (zzemu_nd, 2026-11-08). `integer` admits no number:
 * IDM classifies every JSON number as `number`. `relationship`/`any` stay unchecked.
 */
const MEASURED_TYPES = new Set(["string", "object", "array", "boolean", "number", "integer"]);

/**
 * What the lane needs to answer a type's implicit policies (P2-2, sol-r1b), as plain lists. IDM's
 * `policy.js` checks only the top-level property's `type`: schemas under `items`/`properties` are
 * stored and not enforced (measured 2026-11-08, docs/api/10). Every field is about top-level
 * properties:
 * - `defaults`: every declared `default`, to fill in on a create that omits the property (measured
 *   for scalars, arrays, objects and, 2026-11-08, `default: null` on every type, stored as `null`
 *   except on an array, where it is stored as `[]`). A null default the lane cannot apply faithfully
 *   is also in `nullUnmeasured`, and the lane faults rather than apply it.
 * - `notNull`: the properties with a non-null default, on which an explicit `null` is `NOT_NULL`.
 *   It keys on the non-null default, not the type (measured 2026-11-08: an object defaulting to
 *   `{x:1}` gets it, no `default: null` property on any type does).
 * - `nullRefused`: the properties an explicit `null` on a create of a new record is refused on:
 *   `notNull`, plus every typed property when the schema has no `required` array (`VALID_TYPE`,
 *   `invalidType "null"`; measured for an undefaulted property and for a string with
 *   `default: null`). Any other typed property's null is accepted and stored, because a
 *   required-named property never gets here (`writeFault` faults the type's writes).
 * - `emptyOnNull`: array properties declaring `default: null` with a `required` array. Their null,
 *   applied or sent on a create of a new record, is stored as `[]` (measured 2026-11-08, zzemu_nd).
 *   A null sent by any other write faults: only the create was measured.
 * - `nullUnmeasured`: a nullable (`["<type>","null"]`) property, whose null is measured only on a
 *   string with a `default: null` and a `required` array, and, when the schema has no `required` array, a
 *   non-string property that declares `default: null` (only the string was measured there) or a
 *   property of a type the rule does not know (`relationship`, `any`). With a `required` array the
 *   unknown types keep writing as before (gaps doc).
 * - `checked`: property → declared type, for `string`/`object`/`array`/`boolean`/`number`/
 *   `integer`, where a mismatching top-level value on a create of a new record is a measured
 *   `403 VALID_TYPE`. A nullable property is checked only where its null default was measured.
 * - `mismatchUnmeasured`: property → concrete type, for every other nullable property of a measured
 *   type (`["boolean","null"]`, `["integer","null"]`, `["array","null"]`, a nullable string with no
 *   measured null default, …). A non-null value of another JSON type faults as unmeasured (sol-r3
 *   P2-3): the plain type's `403 VALID_TYPE` was measured, the nullable spelling's never was, so the
 *   lane neither passes the value nor borrows the plain type's 403. A value of the concrete type
 *   writes, as on the plain type.
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
  readonly emptyOnNull: readonly string[];
  readonly nullUnmeasured: readonly string[];
  readonly checked: Record<string, string>;
  readonly mismatchUnmeasured: Record<string, string>;
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
    const emptyOnNull: string[] = [];
    const checked: Record<string, string> = {};
    const mismatchUnmeasured: Record<string, string> = {};
    const itemTypes: Record<string, string> = {};
    const notNull: string[] = [];
    for (const [name, property] of Object.entries(object.properties)) {
      const known = MEASURED_TYPES.has(property.type);
      const nullDefault = property.default === null;
      const nullMeasured = nullDefault && !object.noRequiredArray;
      // D2's nullable null default was measured on a string only.
      const nullableMeasured = nullMeasured && property.type === "string";
      if (property.default !== undefined) {
        defaulted.push(name);
        defaults[name] = nullMeasured && property.type === "array" && !property.nullable ? [] : property.default;
        if (property.default !== null) notNull.push(name);
      }
      const noRequiredNull = object.noRequiredArray && (!known || (nullDefault && property.type !== "string"));
      if ((property.nullable && !nullableMeasured) || noRequiredNull) nullUnmeasured.push(name);
      else if (nullMeasured && property.type === "array") emptyOnNull.push(name);
      if (known && (!property.nullable || nullableMeasured)) checked[name] = property.type;
      else if (known) mismatchUnmeasured[name] = property.type;
      const item = property.items?.type;
      if (property.type === "array" && item !== undefined && MEASURED_TYPES.has(item)) itemTypes[name] = item;
    }
    const typed = Object.keys(object.properties).filter((name) => !nullUnmeasured.includes(name));
    const nullRefused = object.noRequiredArray ? typed : notNull.filter((name) => !nullUnmeasured.includes(name));
    out[`managed/${object.name}`] = { defaults, notNull, nullRefused, emptyOnNull, nullUnmeasured, checked, mismatchUnmeasured, itemTypes, defaulted };
  }
  return out;
}

/** The declaration {@link judgeDefault} reads: the verbatim type list when the profile kept one. */
function declared(property: PropertySchema): { type: unknown; items?: unknown } {
  const type = property.types ?? (property.nullable ? [property.type, "null"] : property.type);
  return property.items ? { type, items: { type: property.items.types ?? property.items.type } } : { type };
}

/** IDM's VALID_TYPE calls every JSON number "number", so no numeric value is an `integer` (zzemu_nd, 2026-11-08). */
const INTEGER_NOTE = ' (IDM types every JSON number as "number", so an integer accepts no number at all)';

/**
 * Why a property's declaration is one AIC refuses or might, if it is (judged by {@link judgeDefault}).
 * A default nested under `items` or `properties` is unmeasured whatever its value, as in the IDM
 * emulator (`nested property behavior default`): no probe declared one.
 */
function declarationProblem(name: string, property: PropertySchema, depth = 0): { kind: "invalid" | "unmeasured"; why: string } | undefined {
  if (property.default !== undefined) {
    if (depth > 0) return { kind: "unmeasured", why: `${name} declares a default nested inside items or properties, which nothing measured` };
    const verdict = judgeDefault(property.default, declared(property));
    if (verdict !== "valid") {
      const type = JSON.stringify(property.types ?? property.type) + (property.nullable && !property.types ? " (nullable)" : "");
      return { kind: verdict, why: verdict === "invalid"
        ? `${name} declares default ${JSON.stringify(property.default)}, which does not match its type ${type}` +
          (property.type === "integer" || property.items?.type === "integer" ? INTEGER_NOTE : "")
        : `${name} declares default ${JSON.stringify(property.default)} on type ${type}, which no probe covered` };
    }
  }
  if (property.items) {
    const problem = declarationProblem(`${name}[]`, property.items, depth + 1);
    if (problem) return problem;
  }
  for (const [child, schema] of Object.entries(property.properties ?? {})) {
    const problem = declarationProblem(`${name}.${child}`, schema, depth + 1);
    if (problem) return problem;
  }
  return undefined;
}

/**
 * The fault every `openidm` call on a `managed/…` resource raises when any type of the profile
 * declares an invalid (or unjudged) default, or `undefined` when none does. One fault for the whole
 * lane, naming the first offending `type.property`.
 *
 * Measured 2026-11-08 (zzemu_def probe, docs/api/10): the config `PUT` with an invalid default is
 * 200 and reads back, but IDM keeps running the last config/managed it compiled. The bad type stays
 * 404, so do new types added beside it, a removed type keeps serving, and an existing type keeps its
 * old schema, until the bad default is gone, when everything applies at once. A profile records the
 * stored document, not that earlier compile, so no answer the lane could give for any managed type
 * is trustworthy: not the 404 for the bad type (it would imply its siblings are live as written),
 * and not a sibling's write. Never repaired: the profile is used as written. Judged on any profile
 * version, because a default is in the profile or it is not.
 */
export function managedInvalidDefault(profile: EnvProfile): WriteFault | undefined {
  for (const object of byKey(profile)) {
    for (const [name, property] of Object.entries(object.properties)) {
      const problem = declarationProblem(name, property);
      if (problem === undefined) continue;
      return { id: `openidm-managed-${problem.kind}-default/${object.name}.${name}`,
        message: `managed/${object.name}: ${problem.why}. ` + (problem.kind === "invalid"
          ? "On AIC a config/managed holding an invalid default is stored but never applied: IDM keeps serving its last " +
            "good compile for every managed type (measured 2026-11-08, docs/api/10)"
          : "If AIC refuses it, IDM keeps serving its last good config/managed for every managed type (measured " +
            "2026-11-08 for invalid defaults, docs/api/10)") +
          ", which this profile does not describe, so no managed call can be answered locally. Fix the default on the " +
          "tenant and re-pull the profile" };
    }
  }
  return undefined;
}
