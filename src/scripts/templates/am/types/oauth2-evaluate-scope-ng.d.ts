// GENERATED from docs/api/bindings/oauth2-evaluate-scope-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: OAUTH2_EVALUATE_SCOPE_NEXT_GEN.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.
// Applied metadata refinements:
//   - `AccessToken.getField` returns `any`, not metadata's bare `object`.
//   - `AccessToken.setField` accepts `TokenFieldValue`, not metadata's bare `object`.
//   - `Identity.getAttributeValues` adds the typed `AmUserAttribute` overload first.

// A custom access-token field: any JSON-ish scalar, array, or object. Whatever
// you set here lands in the CTS entry, or as a JWT claim for client-based
// tokens — keep it small.
type TokenFieldValue = StringLike | number | boolean | any[] | object;

declare const requestProperties: object;
declare const clientProperties: object;
interface AccessToken {
  getField(key: StringLike): any;
  toMap(): object;
  getPermissions(): object;
  getScope(): any[];
  setPermissions(permissions: object): void;
  setFields(fields: object): void;
  setField(field: StringLike, value: TokenFieldValue): void;
  setNonce(nonce: StringLike): void;
  getNonce(): StringLike;
  getRealm(): StringLike;
  setRealm(realm: StringLike): void;
  setScope(scopes: any[]): void;
  removeRealm(): void;
  getAudience(): any[];
  setId(tokenId: StringLike): void;
  getAuditTrackingId(): StringLike;
  isExpired(): boolean;
  getResourceOwnerId(): StringLike;
  getAuthTimeSeconds(): number;
  getAct(): object;
  getMayAct(): object;
  setAct(value: object): void;
  setMayAct(value: object): void;
  getCustomFields(): object;
  getAuthGrantId(): StringLike;
  getTokenId(): StringLike;
  getAuthLevel(): number;
  getExpiryTime(): number;
  setAuthLevel(authLevel: number): void;
  getClaims(): StringLike;
  getClientId(): StringLike;
  getGrantType(): StringLike;
  getTokenType(): StringLike;
  getConfirmationKey(): object;
  addExtraData(key: StringLike, value: StringLike): void;
  getResourceOwner(): object;
  setClientId(clientId: StringLike): void;
  setResourceOwnerId(resourceOwnerId: StringLike): void;
  getTokenInfo(): object;
  getTokenName(): StringLike;
  setClaims(claims: StringLike): void;
  setExpiryTime(expiryTime: number): void;
  removeConfirmationKey(): void;
  addExtraJsonData(key: StringLike, value: object): void;
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
