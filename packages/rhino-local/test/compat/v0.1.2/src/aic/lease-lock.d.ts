import type { LeaseIdentity } from "./lease-identity.ts";
import type { CreatedResource } from "./run.ts";
export interface LeaseJournal {
    version: 1;
    tenantHash: string;
    aicId: string;
    realm: string;
    treeName: string;
    ownerToken: string;
    resources: CreatedResource[];
    /** Absent in journals written before library cleanup became replayable. */
    ownedLibraries?: OwnedLibrary[];
    managedFixtures: Array<{
        type: string;
        id: string;
    }>;
}
export interface OwnedLibrary {
    id: string;
    name: string;
    sourceHash: string;
    marker: string;
    /** Absent only in unreleased, pre-status journals; those are probe-only. */
    status?: "owned" | "not-owned";
}
export interface LeaseStatePaths {
    root: string;
    lockDir: string;
    ownerPath: string;
    journalPath: string;
    tenantHash: string;
}
export interface LeaseLock {
    paths: LeaseStatePaths;
    release(): Promise<void>;
}
export declare function leaseStatePaths(baseUrl: string, identity: LeaseIdentity, root?: string): LeaseStatePaths;
export declare function acquireLeaseLock(paths: LeaseStatePaths, identity: LeaseIdentity, options?: {
    timeoutMs?: number;
    processExists?: (pid: number) => boolean;
    pollMs?: number;
}): Promise<LeaseLock>;
export declare function newLeaseJournal(paths: LeaseStatePaths, identity: LeaseIdentity, realm: string, resources: readonly CreatedResource[]): LeaseJournal;
export declare function readLeaseJournal(path: string): Promise<LeaseJournal | undefined>;
export declare function writeLeaseJournal(path: string, journal: LeaseJournal): Promise<void>;
export declare function removeLeaseJournal(path: string): Promise<void>;
export declare function addJournalFixture(path: string, fixture: {
    type: string;
    id: string;
}): Promise<void>;
export declare function addJournalResources(path: string, resources: readonly CreatedResource[]): Promise<void>;
export declare function addJournalOwnedLibrary(path: string, library: OwnedLibrary): Promise<void>;
export declare function markJournalLibraryNotOwned(path: string, id: string): Promise<void>;
export declare function removeJournalFixture(path: string, fixture: {
    type: string;
    id: string;
}): Promise<void>;
