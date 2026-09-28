import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  configuredTenantProvider,
  setTenantProvider,
  TenantProviderError,
  type TenantProvider,
} from "./aic/provider.ts";

/**
 * Load a caller's tenant-provider module for a bin (`--provider-module`).
 *
 * A token callback lives in the consumer's code, and a bin is a separate
 * process from the Vitest run that registered it, so the bins take the same
 * module the consumer's `setupFiles` uses. Either shape works: a module whose
 * default export is a `TenantProvider`, or one that calls
 * `setTenantProvider()` when imported.
 */
export async function loadProviderModule(path: string, cwd: string = process.cwd()): Promise<void> {
  const url = pathToFileURL(resolve(cwd, path)).href;
  const loaded = (await import(url)) as { default?: unknown };
  if (isTenantProvider(loaded.default)) {
    setTenantProvider(loaded.default);
    return;
  }
  // An empty environment leaves only the registry to answer.
  if (configuredTenantProvider({}) === undefined) {
    throw new TenantProviderError(
      `${path} neither default-exports a TenantProvider nor calls setTenantProvider()`
    );
  }
}

function isTenantProvider(value: unknown): value is TenantProvider {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.describe === "function" && typeof candidate.getToken === "function";
}
