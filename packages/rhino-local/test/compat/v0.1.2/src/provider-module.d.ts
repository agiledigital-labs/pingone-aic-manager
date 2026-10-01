import { type TenantProvider } from "./aic/provider.ts";
/**
 * Load a caller's tenant-provider module for a bin (`--provider-module`).
 *
 * A token callback lives in the consumer's code, and a bin is a separate
 * process from the Vitest run that registered it, so the bins take the same
 * module the consumer's `setupFiles` uses. Either shape works: a module whose
 * default export is a `TenantProvider`, or one that calls
 * `setTenantProvider()` when imported.
 */
export declare function loadProviderModule(path: string, cwd?: string): Promise<TenantProvider>;
