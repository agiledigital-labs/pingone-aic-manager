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
export declare function findResidue(store: Record<string, JsonObject[]> | undefined, ledger: readonly FixtureSpec[]): ResidueEntry[];
export declare function describeResidue(entries: readonly ResidueEntry[]): string;
