import type { Given, JsonObject } from "../case/types.ts";
import { type AicIo, type TenantSession } from "./tenant.ts";
export interface ManagedFixture {
    type: string;
    record: JsonObject;
}
export interface SeededManagedFixture {
    type: string;
    id: string;
}
/** Whether the first pass's managed seed came exactly from this fixture ledger. */
export declare function managedSeedMatches(managed: Given["managed"], fixtures: readonly ManagedFixture[]): boolean;
/** Create with CREST's create-only request, then prove every declared field landed. */
export declare function seedManagedFixtures(io: AicIo, session: TenantSession, fixtures: readonly ManagedFixture[], seeded: SeededManagedFixture[]): Promise<void>;
/** Delete one fixture the harness successfully created. A prior script delete is clean. */
export declare function deleteManagedFixture(io: AicIo, session: TenantSession, fixture: SeededManagedFixture): Promise<void>;
/**
 * Serialize managed-fixture runs across worker processes for one tenant.
 * A collision after this lock is acquired is therefore pre-existing state,
 * not two files in this harness racing each other.
 */
export declare function acquireManagedFixtureLock(session: TenantSession, label: string): Promise<() => Promise<void>>;
export declare function fixtureIdentity(fixture: ManagedFixture): {
    collection: string;
    id: string;
    resource: string;
};
export declare function managedResource(type: string, id: string): string;
