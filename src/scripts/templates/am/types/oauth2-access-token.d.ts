// LEGACY access-token modification bindings (`OAUTH2_ACCESS_TOKEN_MODIFICATION`,
// no `_NEXT_GEN` suffix). Layered on rhino + common + legacy-common.
//
// Every member below was **called** against the live context on 2026-08-27, not
// read off a metadata dump and not copied from the next-gen overlay. The full
// table, the probe scripts and the throwaway client/user recipe are in
// `docs/api/12-script-bindings-matrix.md`.
//
// That distinction is the whole reason this file is no longer a wall of `any`.
// A first pass enumerated the surface with `typeof`, which LIES for a
// Rhino-wrapped Java object: `typeof identity.getMemberships` is `"function"`
// and calling it throws `Can't find method
// com.sun.identity.idm.AMIdentity.getMemberships()`. `typeof` is only reliable
// in the negative — `"undefined"` really does mean absent — so a member is
// declared here iff a probe invoked it and it returned.
//
// The context is NOT the next-gen one with the suffix stripped. Measured
// differences, each of which would be a silent runtime failure if this file had
// been transcribed from `oauth2-access-token-ng.d.ts`:
//
//   • `identity` is a classic `AMIdentity` (`isExists`, `getAttribute`), not the
//     next-gen `Identity` (`exists`, `getAttributeValues`) — those two names
//     throw here, and `setAttribute`/`addAttribute` are undefined.
//   • `setAct`, `setMayAct`, `setPermissions` and `setConfirmationKey` are on the
//     next-gen interface and are NOT callable here.
//   • `setScope` wants a `java.util.Set`; a JS array throws
//     `Cannot convert NativeArray to java.util.Set`.
//   • `openidm`, `utils`, `idRepository`, `emailService`, `require` and — the
//     surprise — `secrets` are all absent.
//   • `session` is `null`.
//
// This context cannot `require()` a library. If the script does not have to be
// legacy, migrating to `…_NEXT_GEN` buys `openidm`, `utils`, `require()` and a
// generated `AccessToken` overlay.

// ---- accessToken ----------------------------------------------------------

/**
 * A custom access-token field. Whatever you set lands in the CTS entry, or as a
 * JWT claim on a stateless client — keep it small.
 *
 * A number round-trips as a DOUBLE: `setField("n", 42)` reads back as `42.0`
 * (measured), the same coercion `httpClient` applies to request bodies. Box it
 * with `java.lang.Integer.valueOf(42)` if the consumer cares.
 */
type TokenFieldValue = StringLike | number | boolean | any[] | object;

/**
 * The token being modified.
 *
 * Getters that had nothing to return handed back `null` rather than throwing or
 * yielding a blank — `getNonce`, `getAct`, `getMayAct`, `getPermissions`,
 * `getClaims`, `getAuthLevel`, `getConfirmationKey` and `getField` of an unset
 * key were all `null` on a plain `client_credentials` token — so they are typed
 * nullable and `strictNullChecks` will make you say what you want when it is
 * missing.
 */
interface AccessToken {
  /** The value set by `setField`/`setFields`, or `null` if the key is unset. */
  getField(key: StringLike): any;
  /** The token response body: `access_token`, `scope`, `token_type`, `expires_in`. */
  toMap(): JavaMap<JavaString, any>;
  setField(field: StringLike, value: TokenFieldValue): void;
  setFields(fields: object): void;

  getRealm(): JavaString;
  setRealm(realm: StringLike): void;
  /** Leaves `getRealm()` `null`. */
  removeRealm(): void;

  /** Client id on `client_credentials`; the IDM uuid on a user grant. */
  getResourceOwnerId(): JavaString;
  setResourceOwnerId(resourceOwnerId: StringLike): void;
  removeResourceOwnerId(): void;

  getClientId(): JavaString;
  setClientId(clientId: StringLike): void;
  removeClientId(): void;

  getAuditTrackingId(): JavaString;
  setAuditTrackingId(auditTrackingId: StringLike): void;
  removeAuditTrackingId(): void;

  getAuthGrantId(): JavaString;
  setAuthGrantId(authGrantId: StringLike): void;
  removeAuthGrantId(): void;

  getTokenName(): JavaString;
  setTokenName(tokenName: StringLike): void;
  removeTokenName(): void;

  getTokenType(): JavaString;
  setTokenType(tokenType: StringLike): void;
  removeTokenType(): void;

  getGrantType(): JavaString;
  setGrantType(grantType: StringLike): void;
  removeGrantType(): void;

  /** The signed JWT on a stateless client; the CTS id on a stateful one. */
  getTokenId(): JavaString;
  /**
   * Stateful tokens only. On a stateless client this throws
   * `Client-side token's ID cannot be changed` — verified against both, with
   * the same script, by flipping `statelessTokensEnabled`.
   */
  setId(tokenId: StringLike): void;

  /** Epoch **seconds**, unlike `getExpiryTime`. */
  getAuthTimeSeconds(): number;
  setAuthTime(authTime: number): void;
  removeAuthTime(): void;

  /** Epoch **milliseconds**, unlike `getAuthTimeSeconds`. */
  getExpiryTime(): number;
  setExpiryTime(expiryTime: number): void;
  isExpired(): boolean;

  getAudience(): JavaArray<JavaString>;
  /** A `java.util.Set`. */
  getScope(): JavaSet<JavaString>;
  /**
   * Takes a `java.util.Set`, NOT a JS array: `setScope(["a"])` throws
   * `Cannot convert org.mozilla.javascript.NativeArray to java.util.Set`.
   * Build one with `new java.util.HashSet()` and `.add(...)`.
   */
  setScope(scopes: JavaSet<JavaString>): void;
  removeScopes(): void;

  getNonce(): JavaString | null;
  setNonce(nonce: StringLike): void;
  removeNonce(): void;

  getClaims(): JavaString | null;
  setClaims(claims: object | StringLike): void;
  removeClaims(): void;

  getAuthLevel(): number | null;
  setAuthLevel(authLevel: number): void;
  removeAuthLevel(): void;

  /** Extra response-body entries, e.g. `{subname, expires_in}`. */
  getCustomFields(): JavaMap<JavaString, any>;
  addExtraData(key: StringLike, value: StringLike): void;
  addExtraJsonData(key: StringLike, value: object): void;

  getAct(): object | null;
  getMayAct(): object | null;
  getPermissions(): object | null;
  removePermissions(): void;
  getConfirmationKey(): object | null;
  removeConfirmationKey(): void;
  getTokenInfo(): object;

  // Deliberately ABSENT, though `oauth2-access-token-ng.d.ts` declares them:
  // setAct, setMayAct, setPermissions — `Can't find method …(object)` on both
  //   StatelessAccessToken and StatefulAccessToken, so it is the context and
  //   not the token flavour;
  // setConfirmationKey — same;
  // getExtraData, setExtraData — `Cannot find function getExtraData`;
  // getResourceOwner — `Access to Java class
  //   "org.forgerock.oauth2.core.ResourceOwner" is prohibited`;
  // getType, getValue, getCreationTime, getSubject, getIssuer, setIssuer,
  // getSessionId, getRefreshTokenId, getAuthModules,
  // getAuthenticationContextClassReference, getRedirectUri, getExpiresIn,
  // getScriptedClaims — all `undefined`.
}
declare const accessToken: AccessToken;

// ---- identity -------------------------------------------------------------

/**
 * The resource owner as a classic `AMIdentity` — the CLIENT's identity on
 * `client_credentials` (`IdType: agentonly`), the user's on a `password` grant
 * (`IdType: user`, and `getAttribute("mail")` returned the real address).
 *
 * Note this contradicts `docs/api/22-token-exchange.md`, which records
 * `identity` bound but EMPTY in the next-gen validate-scope context on the same
 * grants. Different context, different answer; do not carry either across.
 */
interface AmIdentity {
  getName(): JavaString;
  /** e.g. `id=alice,ou=user,o=alpha,ou=services,ou=am-config`. */
  getUniversalId(): JavaString;
  /** The realm DN, not `/alpha`. */
  getRealm(): JavaString;
  /** e.g. `IdType: user`. */
  getType(): JavaString;
  isExists(): boolean;
  isActive(): boolean;
  /** Empty list, never `null`, for an attribute the identity does not have. */
  getAttribute(attributeName: StringLike): JavaArray<JavaString>;
  /**
   * EVERY attribute. On an `agentonly` identity that includes the OAuth2
   * client's own `userpassword` — do not log this object.
   */
  getAttributes(): JavaMap<JavaString, JavaArray<JavaString>>;
  store(): void;

  // ABSENT here, present on the next-gen `Identity`: `exists`,
  // `getAttributeValues`, `setAttribute`, `addAttribute`. Also absent:
  // `getMemberships`, which `typeof` reports as a function and which throws
  // `Can't find method` when called.
}
declare const identity: AmIdentity;

// ---- the rest -------------------------------------------------------------

/**
 * The resource owner's SSO session — **measured `null`** on both the
 * `client_credentials` and `password` grants (every member access threw
 * `Cannot call method "…" of null`).
 *
 * No grant that populates it was exercised, so the member surface is genuinely
 * unknown and this stays `any` rather than being transcribed from AM's
 * `SSOToken` API. Guard it: `if (session) { … }`.
 */
declare const session: any;

/** Scopes on the request: a `java.util.HashSet`, so `.contains(...)`/`.size()` — not `.length` or `[0]`. */
declare const scopes: JavaSet<JavaString>;

/**
 * The OAuth2 request. Members are PROPERTIES, not `.get()` calls, and the maps
 * inside are Java multimaps — read one as `String(params.grant_type[0])`.
 *
 * The four members below are the ones the context actually enumerated, and they
 * happen to match the next-gen `RequestProperties`; that agreement is measured
 * here, not assumed from there. `requestParams` did NOT carry `client_secret`.
 */
interface AtmRequestProperties {
  requestParams: Record<string, JavaArray<JavaString>>;
  /** Carries the client's `Authorization` header — keep it out of logs. */
  requestHeaders: Record<string, JavaArray<JavaString>>;
  /** e.g. `/alpha`, unlike `identity.getRealm()`. */
  realm: JavaString;
  requestUri: JavaString;
  [key: string]: unknown;
}
declare const requestProperties: AtmRequestProperties;

/**
 * The OAuth2 client. Enumerated members only; `customProperties` is here and is
 * NOT on the next-gen `ClientProperties`.
 */
interface AtmClientProperties {
  clientId: JavaString;
  /** Screaming case: `CLIENT_CREDENTIALS`, not `client_credentials`. */
  allowedGrantTypes: JavaArray<JavaString>;
  allowedScopes: JavaArray<JavaString>;
  allowedResponseTypes: JavaArray<JavaString>;
  customProperties: JavaMap<JavaString, any>;
  [key: string]: unknown;
}
declare const clientProperties: AtmClientProperties;

// Legacy Java interop, as in the legacy OIDC claims context. `JavaImporter` is
// present; `new java.lang.RuntimeException(...)` constructs here, unlike on the
// next-gen engine.
declare const JavaImporter: (...classes: any[]) => void;
declare const org: any;
declare const java: any;
