/**
 * A private UUID namespace for rhino-local AIC lease resources.
 *
 * Changing it would orphan residue addressed by an earlier harness version.
 */
export declare const AIC_LEASE_NAMESPACE = "b42f3f57-ec8e-5e9f-9a2b-59cc9e9d45d0";
export interface LeaseResourceIds {
    subjectScript: string;
    subjectNode: string;
    resultScripts: Readonly<Record<string, string>>;
    resultNodes: Readonly<Record<string, string>>;
    sessionTree: string;
    sessionScript: string;
    sessionNode: string;
}
export interface LeaseIdentity {
    id: string;
    idHash: string;
    treeName: string;
    snapshotKey: string;
    outcomes: readonly string[];
    ids: LeaseResourceIds;
    authorDigest: string;
    structuralDigest: string;
    ownerToken: string;
    marker: string;
}
export interface LeaseIdentityOptions {
    id: string;
    source: string;
    outcomes: readonly string[];
    ownerToken?: string;
}
export declare function createLeaseIdentity(options: LeaseIdentityOptions): LeaseIdentity;
export declare function parseLeaseMarker(value: unknown): {
    id?: string;
    idHash: string;
    ownerToken: string;
} | undefined;
export declare function normalizedLeaseOutcomes(outcomes: readonly string[]): string[];
export declare function sha256(value: string): string;
/** RFC 4122 UUIDv5 without adding a dependency. */
export declare function uuidV5(name: string): string;
