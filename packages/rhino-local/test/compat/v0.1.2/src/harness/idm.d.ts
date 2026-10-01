/**
 * Local `check()`/`cleanup()` handle and shared managed-path split. The tenant
 * counterpart in `aic/idm.ts` imports only that path rule across the seam.
 */
import type { JsonObject } from "../case/types.ts";
import type { FixtureSpec, IdmHandle } from "./types.ts";
/** Local handle over one run's harvested managed store. */
export declare function localIdmHandle(store: Record<string, JsonObject[]>, onDelete: (resource: string) => void): IdmHandle;
/** `managed/alpha_user/alice` → [`managed/alpha_user`, `alice`]. */
export declare function splitResource(resource: string): [string, string];
/** Collapse a ledger into the `given.managed` shape the local lane seeds. */
export declare function ledgerToManaged(ledger: readonly FixtureSpec[]): Record<string, JsonObject[]>;
