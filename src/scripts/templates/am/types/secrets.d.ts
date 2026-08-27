// The AM `secrets` binding.
//
// Split out of common.d.ts because it is NOT on every leaf: it is `undefined`
// in the legacy access-token-modification context (`OAUTH2_ACCESS_TOKEN_MODIFICATION`,
// measured 2026-08-27 — docs/api/12-script-bindings-matrix.md), which is the one
// leaf whose tsconfig omits this file. Every other leaf includes it, because
// that is where the evidence stops: `secrets` was confirmed present on the
// legacy scripted-decision probe and is in the next-gen binding metadata, and no
// other legacy context has been probed for it.
//
// If you probe another legacy context and find it absent, drop this file from
// that leaf too rather than widening the claim here.

// Each accessor returns a secret object (read it via its own methods). Method
// set from the next-gen binding metadata.
interface Secrets {
  getGenericSecret(secretId: StringLike): object;
  getDecryptionKey(secretId: StringLike): object;
  getEncryptionKey(secretId: StringLike): object;
  getSigningKey(secretId: StringLike): object;
  getVerificationKey(secretId: StringLike): object;
}
declare const secrets: Secrets;
