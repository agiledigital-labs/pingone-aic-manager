/**
 * The session vocabulary both lanes share: which properties AM owns, and which
 * it derives from the principal. Measured 2026-09-14 against a live tenant —
 * see `docs/api/12-script-bindings-matrix.md` and `docs/api/09-journeys.md`.
 *
 * It lives beside the case types rather than in either lane, because the two
 * lanes have to agree on it: the local mock seeds the derived properties and
 * the AIC lane must NOT send them, and a copy in each would be a copy that can
 * drift.
 */

/**
 * Session properties AM owns. Measured 2026-09-14: `putSessionProperty` on one
 * of these does not override it — the journey fails the login with a bare
 * `401 Unauthorized / Login failure`, naming nothing. Refusing here turns that
 * into a message at the call site.
 */
export const AM_OWNED_SESSION_KEYS: readonly string[] = [
  "AMCtxId",
  "AuthLevel",
  "CharSet",
  "FullLoginURL",
  "Host",
  "HostName",
  "IndexType",
  "Locale",
  "OidcSid",
  "Organization",
  "Principal",
  "Principals",
  "Service",
  "UserId",
  "UserProfile",
  "UserToken",
  "amlbcookie",
  "authInstant",
  "clientType",
  "loginURL",
  "successURL",
  "sun.am.UniversalIdentifier",
];

/** Set form of the above, for callers that test membership. */
export const AM_OWNED_SESSION_SET: ReadonlySet<string> = new Set(
  AM_OWNED_SESSION_KEYS
);

/** Principal the session-minting mini journey authenticates as. */
export const DEFAULT_SESSION_PRINCIPAL = "rl-session";

/**
 * The session properties AM derives from the principal.
 *
 * Measured 2026-09-14 with two different principals, which is what makes this
 * a derivation rather than a transcription of one run: these five tracked the
 * principal and the rest did not. The rest are deliberately NOT seeded — some
 * are per-run (`AMCtxId`, `authInstant`, `OidcSid`), some name the minting
 * tree (`Service`), and one is the caller's own IP. A script that reads those
 * gets `undefined` here and a value on the tenant; that asymmetry is the same
 * one AM's ambient shared state already has, and it fails in the safe
 * direction.
 */
export function sessionFromPrincipal(
  principal: string,
  realm: string
): Record<string, string> {
  const dn = `id=${principal},ou=user,o=${realm},ou=services,ou=am-config`;
  return {
    UserId: principal,
    Principals: principal,
    UserToken: principal,
    Principal: dn,
    "sun.am.UniversalIdentifier": dn,
  };
}
