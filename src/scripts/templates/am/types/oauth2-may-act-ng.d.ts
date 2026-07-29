// GENERATED from docs/api/bindings/oauth2-may-act-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: OAUTH2_MAY_ACT_NEXT_GEN.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.
// Applied metadata refinements:
//   - `Token.getField` returns `any`, not metadata's bare `object`.
//   - `Identity.getAttributeValues` adds the typed `AmUserAttribute` overload first.

declare const requestProperties: object;
declare const clientProperties: object;
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

interface Token {
  getField(key: StringLike): any;
  getAct(): object;
  getMayAct(): object;
  setAct(value: object): void;
  setMayAct(value: object): void;
}
declare const token: Token;

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

declare const scopes: any[];
