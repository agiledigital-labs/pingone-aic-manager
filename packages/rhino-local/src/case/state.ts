import { deepEqual } from "./equal.ts";
import type { JsonObject, StateMutation } from "./types.ts";

/** Net JSON mutations between two views of one state map. */
export function diffState(initial: JsonObject, final: JsonObject): StateMutation[] {
  const mutations: StateMutation[] = [];
  const keys = new Set([...Object.keys(initial), ...Object.keys(final)]);
  for (const key of [...keys].sort()) {
    const beforePresent = Object.prototype.hasOwnProperty.call(initial, key);
    const afterPresent = Object.prototype.hasOwnProperty.call(final, key);
    if (!beforePresent && afterPresent) {
      const after = final[key];
      if (after !== undefined) {
        mutations.push({ operation: "added", key, after });
      }
      continue;
    }
    if (beforePresent && !afterPresent) {
      const before = initial[key];
      if (before !== undefined) {
        mutations.push({ operation: "removed", key, before });
      }
      continue;
    }
    if (!deepEqual(initial[key], final[key])) {
      const before = initial[key];
      const after = final[key];
      if (before !== undefined && after !== undefined) {
        mutations.push({ operation: "changed", key, before, after });
      }
    }
  }
  return mutations;
}

export function sameMutationValue(a: StateMutation, b: StateMutation): boolean {
  if (a.operation !== b.operation || a.key !== b.key) {
    return false;
  }
  if (a.operation === "removed" && b.operation === "removed") {
    return deepEqual(a.before, b.before);
  }
  if (a.operation === "added" && b.operation === "added") {
    return deepEqual(a.after, b.after);
  }
  if (a.operation === "changed" && b.operation === "changed") {
    return deepEqual(a.before, b.before) && deepEqual(a.after, b.after);
  }
  return false;
}
