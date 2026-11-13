// GENERATED from docs/api/bindings/oauth2-validate-scope-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: OAUTH2_VALIDATE_SCOPE_NEXT_GEN.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.
// Applied metadata refinements:
//   - `Identity.getAttributeValues` adds the typed `AmUserAttribute` overload first.
//   - The four scope sets are `ValidateScopeList`s (Java lists), not JS arrays; only `requestedScopes` declares `add`.
//   - `scopeValidatorHelper` is a `ScopeValidatorHelper`, not `any`.

// AM calls the global `validateAccessTokenScope()` with NO arguments after the body
// runs, and takes the JS array of scope strings it returns as the grant (measured
// 2026-11-10, docs/api/12). Those four sets are Java lists of JS strings: `map`,
// `join` and the other Array methods throw `Cannot find function`, and `iterator()`
// is the class shutter. `[].concat(list)` copies one into a JS array (measured).
interface ValidateScopeList extends ScopeList, ConcatMeasuredList<string> {
  indexOf(scope: StringLike): number;
  isEmpty(): boolean;
  forEach(callback: (scope: string) => void): void;
}

// `add` was called on `requestedScopes` only (it returned `true` and grew the
// list); the other three sets were read, never written.
interface RequestedScopeList extends ValidateScopeList {
  /** Grows the list in place; `true` on success. */
  add(scope: StringLike): boolean;
}

interface ScopeValidatorHelper {
  /**
   * Always throws: `400 invalid_scope` with the message as `error_description`.
   * A number is stringified, `null` gives AM's default message, and no argument
   * at all is a `500`.
   */
  throwInvalidScope(message: StringLike | number | null): never;
  inheritAccessTokenScopesOnRefresh(): void;
}

declare const scopeValidatorHelper: ScopeValidatorHelper;
declare const requestProperties: RequestProperties;
declare const clientProperties: ClientProperties;
declare const availableScopes: ValidateScopeList;
declare const requestedScopes: RequestedScopeList;
declare const allowedScopes: ValidateScopeList;
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

declare const defaultScopes: ValidateScopeList;
