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
 * profile pulled before hooks were recorded cannot tell, so its writes fault until a re-pull.
 */
import type { EnvProfile, ObjectSchema, PropertySchema } from "./types.ts";

export type WriteMethod = "create" | "update" | "patch" | "delete";
export const WRITE_METHODS: readonly WriteMethod[] = ["create", "update", "patch", "delete"];
export type WriteFault = { readonly id: string; readonly message: string };

/**
 * Object-level hooks a write runs, in chain order (docs/api/10; the emulator's measured order).
 * Read hooks are not known NOT to run around a write (the response is a read of the record), so
 * they count for every method; so does any hook name this list does not know.
 */
const CHAIN: Record<"create" | "update" | "delete", readonly string[]> = {
  create: ["onCreate", "onValidate", "onStore", "postCreate", "onSync"],
  update: ["onUpdate", "onValidate", "onStore", "postUpdate", "onSync"],
  delete: ["onDelete", "postDelete", "onSync"],
};
const READ_HOOKS = ["onRead", "onRetrieve"];
const KNOWN = new Set([...CHAIN.create, ...CHAIN.update, ...CHAIN.delete, ...READ_HOOKS]);
const PROPERTY_WRITE_HOOKS = ["onValidate", "onStore"];

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
  return [
    ...chain.filter((hook) => declared.includes(hook)),
    ...writes,
    ...READ_HOOKS.filter((hook) => declared.includes(hook)),
    ...propertyHooks(object, (hook) => !PROPERTY_WRITE_HOOKS.includes(hook)),
    ...declared.filter((hook) => !KNOWN.has(hook)),
  ];
}

function propertyPolicies(path: string, property: PropertySchema): string[] {
  const own = [
    ...(property.enum ? [`${path} enum`] : []),
    ...(property.policies ?? []).map((id) => `${path} ${id}`),
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

export function writeFault(object: ObjectSchema, method: WriteMethod): WriteFault | undefined {
  const type = `managed/${object.name}`;
  if (object.hooks === undefined) {
    return { id: "openidm-managed-profile-stale", message: `${type}: the environment profile predates hook recording, so ` +
      "whether this write runs hooks on AIC is unknown here. Re-pull it: aic-script-tester-pull-profile" };
  }
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
  for (const object of Object.values(profile.objects)) {
    const faults: Partial<Record<WriteMethod, WriteFault>> = {};
    for (const method of WRITE_METHODS) {
      const fault = writeFault(object, method);
      if (fault) faults[method] = fault;
    }
    if (Object.keys(faults).length > 0) out[`managed/${object.name}`] = faults;
  }
  return out;
}
