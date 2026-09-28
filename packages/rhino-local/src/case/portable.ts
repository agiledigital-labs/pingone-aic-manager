import { ENV_INPUT_KEYS } from "./types.ts";
import type { Given } from "./types.ts";

/**
 * A case is portable when it declares none of the environment-dependent
 * inputs (`esv`, `secrets`, `managed`, `http`). Empty maps/arrays do not
 * count as a declaration. This is a property of the case, not a guarantee
 * that local and AIC verdicts will agree — see the done-note on leaks.
 */
export function isPortable(kase: { given?: Given }): boolean {
  const given = kase.given ?? {};
  for (const key of ENV_INPUT_KEYS) {
    if (hasEnvInput(given[key])) {
      return false;
    }
  }
  return true;
}

function hasEnvInput(value: unknown): boolean {
  if (value === undefined || value === null) {
    return false;
  }
  if (Array.isArray(value)) {
    return value.length > 0;
  }
  if (typeof value === "object") {
    return Object.keys(value).length > 0;
  }
  return true;
}
