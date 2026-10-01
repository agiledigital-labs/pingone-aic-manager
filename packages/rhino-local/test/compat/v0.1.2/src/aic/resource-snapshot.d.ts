export type AicResourceKind = "script" | "node" | "tree";
/** Strip server-owned node metadata even though AM accepts some of it. */
export declare function nodeRequestProjection(value: Readonly<Record<string, unknown>>): Record<string, unknown>;
export declare function resourceRequestProjection(kind: AicResourceKind, value: Readonly<Record<string, unknown>>): Record<string, unknown>;
/**
 * Return the snapshot from the confirming GET, never from submitted bytes.
 *
 * Script and tree `description` round-trips byte-exact (measured 2026-09-14,
 * `docs/api/04-scripts.md`), so the marker stays in the comparison rather than
 * being normalized away — a rewritten marker is a real failure, not noise.
 */
export declare function confirmResourceSnapshot(kind: AicResourceKind, submitted: Readonly<Record<string, unknown>>, confirmed: unknown): Record<string, unknown>;
