# 07 — Secret stores (collection API NOT AVAILABLE; per-type subpaths ARE)

Implemented in: — (collection API is forbidden; per-type secret mappings live in `src/secretmap/`)

> **Correction (2026-06-17):** the original blanket "secret stores are entirely
> disabled" claim was over-broad — it was tested only against the *collection*
> calls below. The **per-store-type subpaths are open** and that is where ESV
> **secret mappings** live (ESV secret → AM secret label). See
> [15-secret-mappings.md](15-secret-mappings.md). What remains blocked is the
> store-collection enumeration (`?_action=nextdescendents`, bare listing,
> `secrets/types`, and the whole `global-config/secrets/**` tree).

## Status (collection API)

**The AM secret-stores *collection* API (enumerate/create stores via
`nextdescendents`, `secrets/types`, the global-config variants) is disabled in
PingOne Advanced Identity Cloud.** These return:

```
HTTP 403
{
  "code": 403,
  "reason": "Forbidden",
  "message": "This operation is not available in PingOne Advanced Identity Cloud."
}
```

verified 2026-05-17 on the sandbox for:

- `POST /am/json/realms/root/realms/alpha/realm-config/secrets/stores?_action=nextdescendents`
- `POST /am/json/global-config/secrets/stores?_action=nextdescendents`
- `GET  /am/json/realms/root/realms/alpha/realm-config/secrets` → 404
- `GET  /am/json/realms/root/realms/alpha/realm-config/secrets/stores?_queryFilter=true` → 404

## What to use instead

In AIC, **ESVs (Environment Secrets & Variables)** are the only first-class
secret-management surface. See [03-esvs.md](03-esvs.md).

Specifically:

- **For signing/encryption keys** that AM consumes (OAuth2 token signing, SAML
  assertion signing, etc.): use ESV secrets with appropriate `encoding`
  (`pem` for keys, `generic` for raw bytes, `base64hmac` / `base64aes` for
  symmetric).
- **For per-environment URLs / credentials**: use ESV variables.

## Impact on pingone-aic-manager feature #4

The user's original feature list included "manage secret stores". Since this
isn't exposed in AIC, the implementation should be:

- **Document this clearly in the UI** with a one-line note pointing users to
  the ESV management screen.
- **Do not** expose a generic "Secret Stores" management tab (enumerate/create
  stores) — those collection calls 403.
- **A secret-*mappings* surface IS viable**, though: listing/editing the ESV
  store's secret-label → ESV-alias mappings works fine. See
  [15-secret-mappings.md](15-secret-mappings.md).

If at some point the AIC team enables read access to a subset (e.g.
hsm-backed default-keystore for inspection), revisit this file and add it.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-05-17
- All paths above returned 403 / 404 as noted. Conclusion: not exposed.

## Source citations

- frodo-lib: `src/api/SecretStoreApi.ts` (documented but assumes self-managed
  AM, not AIC).
- Ping AIC docs do not document a secret-stores REST endpoint.

## Open questions

- Are *any* secret-store sub-endpoints exposed (e.g. for read-only schema
  introspection)? Worth testing periodically; Ping may relax this.
