import { isPortable } from "../case/portable.ts";
import { ENV_INPUT_KEYS } from "../case/types.ts";
import type { Case, Given } from "../case/types.ts";

function objectHasKeys(value: object | undefined): boolean {
  return value !== undefined && Object.keys(value).length > 0;
}

/**
 * Why this case must not be executed on a tenant. `undefined` means the
 * wrapper journey can seed `given` faithfully enough that a green AIC run
 * is evidence, not coincidence.
 *
 * Environment-dependent inputs (`esv` / `secrets` / `managed` / `http`) are
 * skipped rather than silently running against whatever the tenant holds —
 * that is the S4 portability rule, applied here as a hard skip.
 */
export function aicUnsupportedReason(kase: Case): string | undefined {
  if (kase.given.engine === "legacy") {
    return "legacy engine: AIC wrapper emit is next-gen only (legacy results go through JavaImporter + Action.send, not callbacksBuilder)";
  }
  if (!isPortable(kase)) {
    const declared = ENV_INPUT_KEYS.filter((key) => hasDeclaredEnv(kase.given, key));
    return `given.${declared.join(", ")} is environment-dependent; AIC lane skips rather than run against whatever the tenant holds`;
  }
  if (kase.given.resumedFromSuspend === true) {
    return "given.resumedFromSuspend=true cannot be seeded on a first authenticate";
  }
  if (objectHasKeys(kase.given.secureState)) {
    return "given.secureState: next-gen nodeState has no putSecure";
  }
  if (kase.given.scriptName !== undefined) {
    return "given.scriptName cannot be seeded on AIC (it is the uploaded script's name)";
  }
  if (kase.given.cookieName !== undefined) {
    return "given.cookieName cannot be seeded on AIC (it is a tenant serverinfo value)";
  }
  if (objectHasKeys(kase.given.locales)) {
    return "given.locales cannot be seeded by a setup script";
  }
  if (objectHasKeys(kase.given.existingSession)) {
    // Seedable in principle — a mini journey run to completion, its cookie
    // forwarded to the subject (docs/api/09-journeys.md) — but the wrapper
    // emitter does not do it yet. Skipping says so; running would seed nothing
    // and grade the result as though the binding had been absent on purpose.
    return "given.existingSession needs a session-minting mini journey the AIC lane does not run yet";
  }
  if (objectHasKeys(kase.given.bindings)) {
    return "given.bindings cannot be seeded on AIC (those are engine bindings, not nodeState)";
  }
  return undefined;
}

function hasDeclaredEnv(given: Given, key: (typeof ENV_INPUT_KEYS)[number]): boolean {
  const value = given[key];
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
