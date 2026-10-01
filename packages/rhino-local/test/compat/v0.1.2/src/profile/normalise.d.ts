import type { EnvProfile } from "./types.ts";
export declare class ProfileShapeError extends Error {
    constructor(message: string);
}
/**
 * Normalise `GET /openidm/config/managed` into an `EnvProfile`.
 *
 * Pure, so the wire shape is testable without a tenant. Unknown keys are
 * dropped rather than carried: this profile is consumed by validation, and a
 * field nothing reads is a field that rots silently.
 */
export declare function normaliseManagedConfig(document: unknown, meta: {
    tenant: string;
    pulledAt: string;
    endpoint: string;
}): EnvProfile;
