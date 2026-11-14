// GENERATED from docs/api/bindings/saml2-nameid-mapper-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: SAML2_NAMEID_MAPPER.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.
// Applied metadata refinements:
//   - `nameIDScriptHelper` is a `NameIdScriptHelper`. The assertion NameID is the script's completion value.
//   - `session` is bound. No member of it was read.

// Measured 2026-11-14 in an SP-initiated SSO (docs/api/12). The assertion's
// NameID text is the script's completion value when that value is a string.
// A completion of `undefined` fails SSO. `createNameIdentifier` is present
// and was not called. `getNameIDFromSession` returned `null` only.
// `shouldPersistNameIDFormat` returned `false` only.
interface NameIdIdentity {
  getName(): string;
  exists(): boolean;
  getAttributeValues(attributeName: "mail"): { readonly length: 1; readonly 0: string };
}

interface NameIdScriptHelper {
  getNameIDValue(): string;
  getNameIDFromSession(): null;
  shouldPersistNameIDFormat(): false;
  createNameIdentifier: unknown;
}

declare const nameIDScriptHelper: NameIdScriptHelper;
declare const identity: NameIdIdentity;
declare const nameIDFormat: string;
declare const remoteEntityId: string;
declare const hostedEntityId: string;

// Bound in the measured SSO (typeof "object") even though the binding
// descriptor does not list it. No member was read.
declare const session: unknown;
