// GENERATED from docs/api/bindings/oidc-claims-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: OIDC_CLAIMS_NEXT_GEN.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.
// Applied metadata refinements:
//   - `Identity.getAttributeValues` adds the typed `AmUserAttribute` overload first.

declare const requestProperties: RequestProperties;
declare const clientProperties: ClientProperties;
declare const claimsLocales: any[];
declare const requestedClaims: object;
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

declare const claims: object;
declare const scopes: any[];
