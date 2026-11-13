// GENERATED from docs/api/bindings/oauth2-atm-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: OAUTH2_ACCESS_TOKEN_MODIFICATION_NEXT_GEN.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.
// Applied metadata refinements:
//   - `AccessToken.getField` returns `any`, not metadata's bare `object`.
//   - `AccessToken.setField` accepts `TokenFieldValue`, not metadata's bare `object`.
//   - `AccessToken` getters that answer JS `null` on a token with no value return `T | null`.
//   - `AccessToken.addExtraJsonData` also accepts the JSON string measured on next-gen.
//   - `AccessToken.setScope` accepts a JS array or the `scopes` list.
//   - `Identity.getAttributeValues` adds the typed `AmUserAttribute` overload first.
//   - `scopes` is a `TokenScopeList` (a Java list), not a JS array.

// A custom access-token field: any JSON-ish scalar, array, or object. Whatever
// you set here lands in the CTS entry, or as a JWT claim for client-based
// tokens — keep it small.
type TokenFieldValue = StringLike | number | boolean | any[] | object;

// `add()` was measured on this context's `scopes` only: it returned `true` and
// grew the list, and a later `setScope` copy does not see it (docs/api/12).
interface TokenScopeList extends ScopeList {
  /** Grows the list in place; `true` on success. */
  add(scope: StringLike): boolean;
}

declare const requestProperties: RequestProperties;
declare const clientProperties: ClientProperties;
interface AccessToken {
  getField(key: StringLike): any;
  toMap(): object;
  getPermissions(): object | null;
  getScope(): any[];
  setPermissions(permissions: object): void;
  setFields(fields: object): void;
  setField(field: StringLike, value: TokenFieldValue): void;
  setNonce(nonce: StringLike): void;
  getNonce(): StringLike | null;
  getRealm(): StringLike;
  setRealm(realm: StringLike): void;
  setScope(scopes: any[] | ScopeList): void;
  removeRealm(): void;
  getAudience(): any[];
  setId(tokenId: StringLike): void;
  getAuditTrackingId(): StringLike;
  isExpired(): boolean;
  getResourceOwnerId(): StringLike;
  getAuthTimeSeconds(): number;
  getAct(): object | null;
  getMayAct(): object | null;
  setAct(value: object): void;
  setMayAct(value: object): void;
  getCustomFields(): object;
  getAuthGrantId(): StringLike;
  getTokenId(): StringLike;
  getAuthLevel(): number | null;
  getExpiryTime(): number;
  setAuthLevel(authLevel: number): void;
  getClaims(): StringLike | null;
  getClientId(): StringLike;
  getGrantType(): StringLike;
  getTokenType(): StringLike;
  getConfirmationKey(): object | null;
  addExtraData(key: StringLike, value: StringLike): void;
  getResourceOwner(): object;
  setClientId(clientId: StringLike): void;
  setResourceOwnerId(resourceOwnerId: StringLike): void;
  getTokenInfo(): object;
  getTokenName(): StringLike;
  setClaims(claims: StringLike): void;
  setExpiryTime(expiryTime: number): void;
  removeConfirmationKey(): void;
  addExtraJsonData(key: StringLike, value: StringLike | object): void;
  setAuditTrackingId(auditTrackingId: StringLike): void;
  removeAuditTrackingId(): void;
  removePermissions(): void;
  setAuthGrantId(authGrantId: StringLike): void;
  removeAuthGrantId(): void;
  removeClientId(): void;
  removeResourceOwnerId(): void;
  removeScopes(): void;
  setAuthTime(authTime: number): void;
  removeAuthTime(): void;
  removeAuthLevel(): void;
  setTokenName(tokenName: StringLike): void;
  removeTokenName(): void;
  removeNonce(): void;
  removeClaims(): void;
  setTokenType(tokenType: StringLike): void;
  removeTokenType(): void;
  setGrantType(grantType: StringLike): void;
  removeGrantType(): void;
  setConfirmationKey(confirmationKey: object): void;
}
declare const accessToken: AccessToken;

interface EmailService {
  send(to: StringLike, subject: StringLike, body: StringLike): void;
  send(
    to: StringLike,
    subject: StringLike,
    body: StringLike,
    mimeType: StringLike
  ): void;
}
declare const emailService: EmailService;

interface Identity {
  getName(): StringLike;
  store(): void;
  exists(): boolean;
  setAttribute(attributeName: StringLike, attributeValues: any[]): void;
  addAttribute(attributeName: StringLike, attributeValue: StringLike): void;
  // Typed managed-user attribute names first (autocomplete; docs/api/14), then
  // the permissive StringLike fallback for any other attribute.
  getAttributeValues(attributeName: AmUserAttribute): any[];
  getAttributeValues(attributeName: StringLike): any[];
  getUniversalId(): StringLike;
}
declare const identity: Identity;

declare const scopes: TokenScopeList;

// `session` is conditional. Ping documents it for this context and the descriptor
// omits it. Bare reads were `ReferenceError: "session" is not defined.` on a
// `client_credentials` grant (token modification) and on a password grant (OIDC
// claims), neither of which has an SSO session (measured 2026-11-10, docs/api/12);
// an `authorization_code` grant is unmeasured. Guard with
// `typeof session !== "undefined"` before reading it, and do not rely on `null`:
// no grant measured here binds one. The two members are Ping's, unmeasured.
interface OAuthScriptedSession {
  getProperty(name: StringLike): StringLike | null;
  setProperty(name: StringLike, value: StringLike): void;
}
declare const session: OAuthScriptedSession | undefined;
