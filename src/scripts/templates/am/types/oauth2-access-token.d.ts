// LEGACY access-token modification bindings (`OAUTH2_ACCESS_TOKEN_MODIFICATION`,
// no `_NEXT_GEN` suffix). Layered on rhino + common + legacy-common.
//
// This file exists to declare the binding NAMES. Until it was added, the legacy
// leaf fell into `am::leaf_tsconfig`'s catch-all — rhino + common +
// legacy-common — so `accessToken`, `identity` and the rest were `Cannot find
// name`, while the ESLint config had known them all along. `no-undef` is off
// precisely because the type layer is meant to be the authority, so the
// authority knew less than the linter did.
//
// The names come from the sandbox's own context metadata by way of that ESLint
// block, which also carries the verified note that this context cannot
// `require()` a library. **No member shape is verified for the legacy engine**,
// so nothing here claims one: every binding is `any`. That is the honest state,
// not a placeholder to be filled in from the next-gen file — the next-gen
// `AccessToken` interface came from NEXT_GEN editor metadata and says nothing
// about this context.
//
// To earn real types, probe the live context the way the rest of the matrix was
// built: a `typeof` fixture per binding under `scripts/rhino-script-tester/`,
// then a dated row in `docs/api/12-script-bindings-matrix.md`. Replacing an
// `any` here without that is exactly the transcription `.ai/core.md` §2 forbids.
//
// The next-gen contexts are fully typed. If this script does not have to be
// legacy, migrating it is worth more than typing this file: next-gen brings
// `openidm`, `utils`, `require()` and a generated `AccessToken` overlay
// (`oauth2-access-token-ng.d.ts`).

/** The token being modified. Legacy shape unverified — see the header. */
declare const accessToken: any;
/** The resource owner. Legacy shape unverified. */
declare const identity: any;
/** The user's session. Legacy shape unverified. */
declare const session: any;
/** Scopes on the request. Legacy shape unverified. */
declare const scopes: any;
/** Request context. Legacy shape unverified — the next-gen `RequestProperties`
 * in nextgen-common.d.ts is NOT known to describe this one. */
declare const requestProperties: any;
/** OAuth2 client context. Legacy shape unverified. */
declare const clientProperties: any;

// Legacy Java interop, as in the legacy OIDC claims context.
declare const JavaImporter: (...classes: any[]) => void;
declare const org: any;
declare const java: any;
