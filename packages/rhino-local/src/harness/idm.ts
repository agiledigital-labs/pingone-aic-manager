/**
 * Local `check()`/`cleanup()` handle and shared managed-path split. The tenant
 * counterpart in `aic/idm.ts` imports only that path rule across the seam.
 */
import { isPlainObject } from "../case/util.ts";
import type { JsonObject, JsonValue } from "../case/types.ts";
import type { FixtureSpec, IdmHandle } from "./types.ts";

/** Local handle over one run's harvested managed store. */
export function localIdmHandle(
  store: Record<string, JsonObject[]>,
  onDelete: (resource: string) => void,
  identityCustomAttrs: Record<string, string[]> = {},
  identityCustomAttrsOwnedKeys: Record<string, string[]> = {},
): IdmHandle {
  // The AM adapter measurements do not establish the external check handle's
  // materialized response shape. Refuse non-object bags before any operation.
  function requireMaterialized(method: string, resource: string): void {
    const values = identityCustomAttrs[resource];
    if (values === undefined || values.length === 0) return;
    const bag: unknown = JSON.parse(values[0] as string);
    if (isPlainObject(bag)) return;
    const category =
      bag === null ? "null" : Array.isArray(bag) ? "array" : typeof bag;
    if (method === "delete" && category === "string") {
      throw new Error(
        `rhino-local: check idm.delete: ${resource} carries a string fr-idm-custom-attrs bag; REST DELETE fails with HTTP 500 (measured); restore an object bag through AM`,
      );
    }
    throw new Error(
      `rhino-local: check idm.${method}: ${resource} carries a non-object fr-idm-custom-attrs bag (${category}); external handle behavior is unmeasured; restore an object bag through AM`,
    );
  }
  return {
    async read(resource) {
      const [collection, id] = splitResource(resource);
      const row = (store[collection] ?? []).find((entry) => entry._id === id);
      if (row !== undefined) requireMaterialized("read", resource);
      return row ?? null;
    },
    async query(type, filter) {
      const rows = (store[type] ?? []).filter((row) =>
        Object.entries(filter).every(
          (entry) =>
            JSON.stringify(row[entry[0]]) ===
            JSON.stringify(entry[1] as JsonValue),
        ),
      );
      for (const row of rows)
        requireMaterialized("query", `${type}/${String(row._id)}`);
      return rows;
    },
    async delete(resource) {
      const [collection, id] = splitResource(resource);
      const rows = store[collection];
      if (rows !== undefined) {
        const index = rows.findIndex((entry) => entry._id === id);
        if (index >= 0) {
          requireMaterialized("delete", resource);
          rows.splice(index, 1);
        }
      }
      delete identityCustomAttrs[resource];
      delete identityCustomAttrsOwnedKeys[resource];
      onDelete(resource);
    },
  };
}

/** `managed/alpha_user/alice` → [`managed/alpha_user`, `alice`]. */
export function splitResource(resource: string): [string, string] {
  const cut = resource.lastIndexOf("/");
  if (cut <= 0 || cut === resource.length - 1) {
    throw new Error(
      `rhino-local: ${JSON.stringify(resource)} is not a managed record path (expected managed/<type>/<id>)`,
    );
  }
  return [resource.slice(0, cut), resource.slice(cut + 1)];
}

/** Collapse a ledger into the `given.managed` shape the local lane seeds. */
export function ledgerToManaged(
  ledger: readonly FixtureSpec[],
): Record<string, JsonObject[]> {
  const managed: Record<string, JsonObject[]> = {};
  for (const fixture of ledger) {
    const rows = managed[fixture.type] ?? [];
    rows.push(fixture.record);
    managed[fixture.type] = rows;
  }
  return managed;
}
