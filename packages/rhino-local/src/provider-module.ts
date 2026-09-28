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
export async function loadProviderModule(
  path: string,
  cwd: string = process.cwd()
): Promise<TenantProvider> {
  const url = pathToFileURL(resolve(cwd, path)).href;
  // Clear first, so the only provider that can be registered afterwards is
  // one this module supplied: a module that supplies none must not leave an
  // earlier registration — another tenant — in force.
  setTenantProvider(undefined);
  const loaded = (await import(url)) as { default?: unknown };
  if (isTenantProvider(loaded.default)) {
    setTenantProvider(loaded.default);
    return loaded.default;
  }
  // An empty environment leaves only the registry to answer.
  const registered = configuredTenantProvider({});
  if (registered === undefined) {
    throw new TenantProviderError(
      `${path} neither default-exports a TenantProvider nor calls setTenantProvider() ` +
        "(a module that was already imported does not run again, so it cannot register twice)"
    );
  }
  return registered;
}

function isTenantProvider(value: unknown): value is TenantProvider {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.describe === "function" && typeof candidate.getToken === "function";
}
