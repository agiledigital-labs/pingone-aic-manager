/** Recursive value matchers shared by verdicts, lane diffs, and seed checks. */
import type { ExpectedValue, StandardSchema } from "./types.ts";
import { deepEqual } from "./equal.ts";
import { isPlainObject, isStandardSchema } from "./util.ts";

export function isMatcher(value: unknown): value is RegExp | StandardSchema {
  return value instanceof RegExp || isStandardSchema(value);
}

export function containsMatcher(value: unknown): boolean {
  if (isMatcher(value)) return true;
  if (Array.isArray(value)) return value.some(containsMatcher);
  return isPlainObject(value) && Object.values(value).some(containsMatcher);
}

export function matchesValue(expected: ExpectedValue, actual: unknown): boolean {
  if (expected instanceof RegExp) {
    return typeof actual === "string" && new RegExp(expected.source, expected.flags).test(actual);
  }
  if (isStandardSchema(expected)) {
    const result = expected["~standard"].validate(actual);
    if (typeof result === "object" && result !== null && "then" in result) {
      throw new Error("rhino-local: async Standard Schema matchers are unsupported in synchronous judge()");
    }
    if (typeof result !== "object" || result === null) {
      throw new Error("rhino-local: Standard Schema matcher returned no result object");
    }
    return !("issues" in result) || result.issues === undefined ||
      (Array.isArray(result.issues) && result.issues.length === 0);
  }
  if (Array.isArray(expected)) {
    return Array.isArray(actual) && actual.length === expected.length &&
      expected.every((item, index) => matchesValue(item, actual[index]));
  }
  if (isPlainObject(expected)) {
    if (!isPlainObject(actual)) return false;
    const keys = Object.keys(expected);
    return keys.length === Object.keys(actual).length && keys.every((key) =>
      Object.prototype.hasOwnProperty.call(actual, key) &&
      matchesValue(expected[key] as ExpectedValue, actual[key])
    );
  }
  return deepEqual(expected, actual);
}

/** Compare lanes exactly except at declared matcher leaves. */
export function equalExceptMatchers(expected: unknown, left: unknown, right: unknown): boolean {
  if (isMatcher(expected)) return left !== undefined && right !== undefined;
  if (Array.isArray(expected) && Array.isArray(left) && Array.isArray(right)) {
    return left.length === right.length && left.every((item, index) =>
      equalExceptMatchers(expected[index], item, right[index]));
  }
  if (isPlainObject(expected) && isPlainObject(left) && isPlainObject(right)) {
    const keys = new Set([...Object.keys(left), ...Object.keys(right)]);
    return [...keys].every((key) => {
      const leftHas = Object.prototype.hasOwnProperty.call(left, key);
      const rightHas = Object.prototype.hasOwnProperty.call(right, key);
      return leftHas === rightHas && (!leftHas ||
        equalExceptMatchers(expected[key], left[key], right[key]));
    });
  }
  return deepEqual(left, right);
}

/** The tenant's own carried value must satisfy prior matchers; other leaves stay exact. */
export function seedMatches(expected: unknown, local: unknown, tenant: unknown): boolean {
  if (isMatcher(expected)) return tenant !== undefined && matchesValue(expected, tenant);
  if (Array.isArray(expected) && Array.isArray(local) && Array.isArray(tenant)) {
    return local.length === tenant.length && local.every((item, index) =>
      seedMatches(expected[index], item, tenant[index]));
  }
  if (isPlainObject(expected) && isPlainObject(local) && isPlainObject(tenant)) {
    const keys = new Set([...Object.keys(local), ...Object.keys(tenant)]);
    return [...keys].every((key) => {
      const localHas = Object.prototype.hasOwnProperty.call(local, key);
      const tenantHas = Object.prototype.hasOwnProperty.call(tenant, key);
      return localHas === tenantHas && (!localHas ||
        seedMatches(expected[key], local[key], tenant[key]));
    });
  }
  return deepEqual(local, tenant);
}
