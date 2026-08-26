// GENERATED from docs/api/bindings/saml2-nameid-mapper-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: SAML2_NAMEID_MAPPER.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.
// Applied metadata refinements:
//   - `Identity.getAttributeValues` adds the typed `AmUserAttribute` overload first.

declare const nameIDScriptHelper: any;
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

declare const nameIDFormat: StringLike;
declare const remoteEntityId: StringLike;
declare const hostedEntityId: StringLike;
