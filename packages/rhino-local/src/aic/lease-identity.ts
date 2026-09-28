/** Stable resource addressing and per-open ownership for the AIC file lease. */
import { createHash, randomUUID } from "node:crypto";

/**
 * A private UUID namespace for rhino-local AIC lease resources.
 *
 * Changing it would orphan residue addressed by an earlier harness version.
 */
export const AIC_LEASE_NAMESPACE = "b42f3f57-ec8e-5e9f-9a2b-59cc9e9d45d0";

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

export function createLeaseIdentity(options: LeaseIdentityOptions): LeaseIdentity {
  const id = options.id.trim();
  if (id.length === 0) {
    throw new Error("rhino-local: aic.id must not be empty");
  }
  const outcomes = normalizedLeaseOutcomes(options.outcomes);
  const idHash = sha256(id).slice(0, 20);
  const treeName = `rl-aic-${idHash}`;
  const resultScripts: Record<string, string> = {};
  const resultNodes: Record<string, string> = {};
  for (const outcome of outcomes) {
    resultScripts[outcome] = uuidV5(`${id}:result-script:${outcome}`);
    resultNodes[outcome] = uuidV5(`${id}:result-node:${outcome}`);
  }
  const authorDigest = sha256(options.source);
  const structuralDigest = sha256(
    JSON.stringify({ schema: 1, id, outcomes })
  );
  const ownerToken = options.ownerToken ?? randomUUID();
  const marker = [
    "rhino-local:v1",
    idHash,
    ownerToken,
    authorDigest,
    structuralDigest,
  ].join(":");
  return {
    id,
    idHash,
    treeName,
    snapshotKey: `__rhino_local_snapshot_${idHash}`,
    outcomes,
    ids: {
      subjectScript: uuidV5(`${id}:subject-script`),
      subjectNode: uuidV5(`${id}:subject-node`),
      resultScripts,
      resultNodes,
      sessionTree: `rl-aic-${sha256(`${id}:session-tree`).slice(0, 20)}`,
      sessionScript: uuidV5(`${id}:session-script`),
      sessionNode: uuidV5(`${id}:session-node`),
    },
    authorDigest,
    structuralDigest,
    ownerToken,
    marker,
  };
}

export function normalizedLeaseOutcomes(outcomes: readonly string[]): string[] {
  return [...new Set(["true", "false", ...outcomes])];
}

export function sha256(value: string): string {
  return createHash("sha256").update(value, "utf8").digest("hex");
}

/** RFC 4122 UUIDv5 without adding a dependency. */
export function uuidV5(name: string): string {
  const namespace = Buffer.from(AIC_LEASE_NAMESPACE.replace(/-/g, ""), "hex");
  const bytes = createHash("sha1")
    .update(namespace)
    .update(name, "utf8")
    .digest()
    .subarray(0, 16);
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x50;
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80;
  const hex = bytes.toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
