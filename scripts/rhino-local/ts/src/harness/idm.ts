import type { JsonObject, JsonValue } from "../case/types.ts";
import type { FixtureSpec, IdmHandle } from "./types.ts";

/**
 * Local `check()` and `cleanup()` handle. A tenant-backed counterpart does not
 * exist yet; AIC-enabled conformance reports expose that observation gap.
 */
export function localIdmHandle(
  store: Record<string, JsonObject[]>,
  onDelete: (resource: string) => void
): IdmHandle {
  return {
    read(resource) {
      const [collection, id] = splitResource(resource);
      const row = (store[collection] ?? []).find((entry) => entry._id === id);
      return Promise.resolve(row ?? null);
    },
    query(type, filter) {
      const rows = (store[type] ?? []).filter((row) =>
        Object.entries(filter).every(
          (entry) => JSON.stringify(row[entry[0]]) === JSON.stringify(entry[1] as JsonValue)
        )
      );
      return Promise.resolve(rows);
    },
    delete(resource) {
      const [collection, id] = splitResource(resource);
      const rows = store[collection];
      if (rows !== undefined) {
        const index = rows.findIndex((entry) => entry._id === id);
        if (index >= 0) {
          rows.splice(index, 1);
        }
      }
      onDelete(resource);
      return Promise.resolve();
    },
  };
}

/** `managed/alpha_user/alice` → [`managed/alpha_user`, `alice`]. */
export function splitResource(resource: string): [string, string] {
  const cut = resource.lastIndexOf("/");
  if (cut <= 0 || cut === resource.length - 1) {
    throw new Error(
      `rhino-local: ${JSON.stringify(resource)} is not a managed record path (expected managed/<type>/<id>)`
    );
  }
  return [resource.slice(0, cut), resource.slice(cut + 1)];
}

/** Collapse a ledger into the `given.managed` shape the local lane seeds. */
export function ledgerToManaged(
  ledger: readonly FixtureSpec[]
): Record<string, JsonObject[]> {
  const managed: Record<string, JsonObject[]> = {};
  for (const fixture of ledger) {
    const rows = managed[fixture.type] ?? [];
    rows.push(fixture.record);
    managed[fixture.type] = rows;
  }
  return managed;
}
