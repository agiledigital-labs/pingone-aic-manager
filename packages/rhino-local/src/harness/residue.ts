import type { JsonObject } from "../case/types.ts";
import type { FixtureSpec } from "./types.ts";

export interface ResidueEntry {
  resource: string;
  reason: "created-by-script" | "mutated-by-script";
}

/**
 * Identify records that survived a test which the harness did not create.
 *
 * This is the check that stops `cleanup()` rotting. A cleanup function nobody
 * verifies drifts the moment the script grows a new write, and on a tenant the
 * drift is invisible — the harness cannot see a record it has no id for. The
 * local lane can: it owns the whole store, so the residue is simply what is
 * left over after removing everything the ledger accounts for.
 *
 * Compared against the **store**, not the recorded call log. Matching
 * `create` calls to `delete` calls checks intent, and would call a test clean
 * when its cleanup deleted a different id than the one the script wrote. It
 * would also miss a script that mutated a record it never created, which
 * leaves the tenant just as dirty as an extra row does.
 */
export function findResidue(
  store: Record<string, JsonObject[]> | undefined,
  ledger: readonly FixtureSpec[]
): ResidueEntry[] {
  if (store === undefined) {
    return [];
  }
  const accounted = new Map<string, JsonObject>();
  for (const fixture of ledger) {
    const id = recordId(fixture.record);
    if (id !== undefined) {
      accounted.set(`${fixture.type}/${id}`, fixture.record);
    }
  }
  const residue: ResidueEntry[] = [];
  for (const [collection, rows] of Object.entries(store)) {
    for (const row of rows) {
      const id = recordId(row);
      const resource = id === undefined ? collection : `${collection}/${id}`;
      const seeded = id === undefined ? undefined : accounted.get(resource);
      if (seeded === undefined) {
        residue.push({ resource, reason: "created-by-script" });
        continue;
      }
      if (!sameRecord(seeded, row)) {
        residue.push({ resource, reason: "mutated-by-script" });
      }
    }
  }
  return residue;
}

export function describeResidue(entries: readonly ResidueEntry[]): string {
  if (entries.length === 0) {
    return "";
  }
  const lines = entries.map(
    (entry) => `    ${entry.resource}  (${entry.reason.replace(/-/g, " ")})`
  );
  const count = entries.length === 1 ? "1 record" : `${entries.length} records`;
  return [
    `${count} survived the test that the harness did not create:`,
    ...lines,
    "  Add them to the suite's cleanup(), or set allowResidue if deliberate.",
    "  On the AIC lane this would leak permanently and invisibly.",
  ].join("\n");
}

function recordId(record: JsonObject): string | undefined {
  const id = record._id;
  return typeof id === "string" ? id : undefined;
}

/**
 * The mock stores what the script wrote, which may carry fields the fixture
 * never set (`_rev`, server defaults). Comparing only the keys the fixture
 * declared keeps those from reading as mutations the script made.
 */
function sameRecord(seeded: JsonObject, stored: JsonObject): boolean {
  for (const [key, value] of Object.entries(seeded)) {
    if (JSON.stringify(stored[key]) !== JSON.stringify(value)) {
      return false;
    }
  }
  return true;
}
