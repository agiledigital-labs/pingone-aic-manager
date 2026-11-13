// GENERATED from docs/api/bindings/oidc-claims-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: OIDC_CLAIMS_NEXT_GEN.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.
// Applied metadata refinements:
//   - `Identity.getAttributeValues` adds the typed `AmUserAttribute` overload first.
//   - `scopes` is a `ScopeList` (a Java list), not a JS array.
//   - `claims` is an `OidcClaimsMap`, and the script's result is an `OidcClaimsResult`.

// The `claims` binding: a copy of AM's `{ sub, subname }`, both the user's id.
// Changing it changes nothing unless the script returns it (measured 2026-11-10,
// docs/api/12). Values read back as JS strings, so `claims.sub === "…"` holds;
// `delete claims.subname` is `false` and leaves the key.
interface OidcClaimsMap {
  get(key: StringLike): unknown;
  put(key: StringLike, value: unknown): unknown;
  remove(key: StringLike): unknown;
  containsKey(key: StringLike): boolean;
  /** Absent once `remove("sub")` has run. */
  sub?: string;
  subname?: string;
  [key: string]: unknown;
}

// What AM does with the script: the COMPLETION VALUE (its last expression), not a
// `return`. `{ values, compositeScopes? }` is accepted and its claims emitted; a plain
// object with no `values`, or `undefined`, gives no script claims; a throw is a 500.
// Numbers come back as doubles (`42` is `42.0`). `compositeScopes` filters nothing.
// Annotate with `/** @type {OidcClaimsResult} */`, or end the script with the literal.
interface OidcClaimsResult {
  values: { [claim: string]: unknown };
  compositeScopes?: { [scope: string]: unknown };
}

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

declare const claims: OidcClaimsMap;
declare const scopes: ScopeList;

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
