import type { WrapperJourney } from "./emit-journey.ts";
import type { LeaseIdentity } from "./lease-identity.ts";
/**
 * A journey whose only job is to end, so that AM issues a session.
 *
 * There is no other way: a session exists only once a journey has run to
 * completion (`docs/api/09-journeys.md` → "Minting a session for a test").
 * Measured 2026-09-14 — the tree needs no Identify Existing User node, the
 * principal need not exist as a managed object, and the journey that later
 * *consumes* the session needs no configuration at all.
 *
 * AM-owned properties are dropped rather than sent. `putSessionProperty` does
 * not override them; the whole login fails with a bare `401 Unauthorized`
 * naming nothing, so sending one would turn a harness bug into an unexplained
 * authentication failure. The five AM derives from the principal arrive by
 * authenticating AS that principal instead.
 */
export declare function emitSessionJourney(session: Record<string, string>, options: {
    runId: string;
    realm: string;
}): WrapperJourney;
/** Deterministic session-minter resources owned by a file lease. */
export declare function emitLeasedSessionJourney(session: Record<string, string>, options: {
    identity: LeaseIdentity;
    realm: string;
}): WrapperJourney;
