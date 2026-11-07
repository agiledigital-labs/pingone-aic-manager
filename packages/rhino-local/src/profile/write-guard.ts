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
 * Defaulted properties are the one implicit policy the lane does answer (P2-2): AIC is measured to
 * apply a `default` on a create that omits the property, and to refuse an explicit `null` on one
 * (docs/api/10). {@link managedDefaults} gives the lane those per type as plain lists.
 */
import type { JsonValue } from "../case/types.ts";
import { type EnvProfile, type ObjectSchema, PROFILE_VERSION, type PropertySchema } from "./types.ts";

export type WriteMethod = "create" | "update" | "patch" | "delete";
export const WRITE_METHODS: readonly WriteMethod[] = ["create", "update", "patch", "delete"];
export type WriteFault = { readonly id: string; readonly message: string };

/**
 * Object-level hooks a write runs, in chain order (docs/api/10; the emulator's measured order).
 * Read hooks run only around an update or PATCH, which read the record first: a create runs none
 * and a delete runs `onDelete → postDelete → onSync` (docs/api/24-idm-script-runtime.md). A hook
 * name this list does not know counts for every method.
 */
const CHAIN: Record<"create" | "update" | "delete", readonly string[]> = {
  create: ["onCreate", "onValidate", "onStore", "postCreate", "onSync"],
  update: ["onUpdate", "onValidate", "onStore", "postUpdate", "onSync"],
  delete: ["onDelete", "postDelete", "onSync"],
};
const READ_HOOKS = ["onRead", "onRetrieve"];
const KNOWN = new Set([...CHAIN.create, ...CHAIN.update, ...CHAIN.delete, ...READ_HOOKS]);
const PROPERTY_WRITE_HOOKS = ["onValidate", "onStore"];
const READS_FIRST: readonly WriteMethod[] = ["update", "patch"];

function propertyHooks(object: ObjectSchema, wanted: (hook: string) => boolean): string[] {
  const out: string[] = [];
  for (const [name, property] of Object.entries(object.properties)) {
    for (const hook of property.hooks ?? []) if (wanted(hook)) out.push(`${name}.${hook}`);
  }
  return out;
}

/** The hooks a write of `method` would run on AIC, first one first. */
export function hooksRun(object: ObjectSchema, method: WriteMethod): string[] {
  const chain = CHAIN[method === "patch" ? "update" : method];
  const declared = object.hooks ?? [];
  const writes = method === "delete" ? [] : propertyHooks(object, (hook) => PROPERTY_WRITE_HOOKS.includes(hook));
  const reads = READS_FIRST.includes(method);
  return [
    ...chain.filter((hook) => declared.includes(hook)),
    ...writes,
    ...(reads ? READ_HOOKS.filter((hook) => declared.includes(hook)) : []),
    ...(reads ? propertyHooks(object, (hook) => READ_HOOKS.includes(hook)) : []),
    ...propertyHooks(object, (hook) => !PROPERTY_WRITE_HOOKS.includes(hook) && !READ_HOOKS.includes(hook)),
    ...declared.filter((hook) => !KNOWN.has(hook)),
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

/** The lane's lookup table: `managed/<type>` → method → fault, for every type that has one. */
export function managedWriteFaults(profile: EnvProfile): Record<string, Partial<Record<WriteMethod, WriteFault>>> {
  const out: Record<string, Partial<Record<WriteMethod, WriteFault>>> = {};
  const stale = profileStale(profile);
  for (const object of Object.values(profile.objects)) {
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
 * What the lane needs about one type's defaulted properties (P2-2):
 * - `defaults`: every declared non-null `default`, to fill in on a create that omits the property
 *   (measured for scalars and arrays, docs/api/10). A `null` default is not applied: what AIC stores
 *   for one is unmeasured, and a read renders an absent scalar as `null` either way.
 * - `notNull`: the properties an explicit `null` is refused on (stock `policy.js` adds `not-null`
 *   for a non-null default; measured on create, docs/api/10 and 12).
 * - `emulable`: the subset where the lane may emulate the measured refusal itself — the schema has
 *   a `required` array and does not name the property, which is the measured single-`NOT_NULL`
 *   case. Required-named nulls are `NOT_NULL` + `VALID_TYPE` (shape unmeasured).
 * - `defaulted`: every property that declares a `default`, `null` included. Stock `policy.js` adds
 *   an implicit `required` for any default, so a PATCH `remove` of one is checked on AIC; the
 *   measured 403 `REQUIRED` had the property in `schema.required` too, so the lane faults the rest.
 *
 * The lane decides only what it alone knows: the method, whether the record exists, the engine,
 * and how many nulls the write sends.
 */
export type DefaultsRule = {
  readonly defaults: Record<string, JsonValue>;
  readonly notNull: readonly string[];
  readonly emulable: readonly string[];
  readonly defaulted: readonly string[];
};

/** The lane's defaults table: `managed/<type>` → rule, for every type that declares a default. */
export function managedDefaults(profile: EnvProfile): Record<string, DefaultsRule> {
  const out: Record<string, DefaultsRule> = {};
  if (profileStale(profile)) return out;
  for (const object of Object.values(profile.objects)) {
    const defaults: Record<string, JsonValue> = {};
    const defaulted: string[] = [];
    for (const [name, property] of Object.entries(object.properties)) {
      if (property.default === undefined) continue;
      defaulted.push(name);
      if (property.default !== null) defaults[name] = property.default;
    }
    if (defaulted.length === 0) continue;
    const notNull = Object.keys(defaults);
    const emulable = object.noRequiredArray ? [] : notNull.filter((name) => !object.required.includes(name));
    out[`managed/${object.name}`] = { defaults, notNull, emulable, defaulted };
  }
  return out;
}
