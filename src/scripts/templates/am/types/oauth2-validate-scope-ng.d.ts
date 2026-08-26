// GENERATED from docs/api/bindings/oauth2-validate-scope-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: OAUTH2_VALIDATE_SCOPE_NEXT_GEN.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.
// Applied metadata refinements:
//   - `Identity.getAttributeValues` adds the typed `AmUserAttribute` overload first.

declare const scopeValidatorHelper: any;
declare const requestProperties: RequestProperties;
declare const clientProperties: ClientProperties;
declare const availableScopes: any[];
declare const requestedScopes: any[];
declare const allowedScopes: any[];
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

declare const defaultScopes: any[];
