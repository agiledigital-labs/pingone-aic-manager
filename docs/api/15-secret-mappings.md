# 15 — Secret mappings (ESV secret → AM secret label)

## Purpose

AM resolves cryptographic material (OAuth2 client secrets, OIDC signing/encryption
keys, SAML2 signing keys, MFA/device encryption keys, agent secrets, …) by
**secret label** (also called a *secret ID* or *purpose*) — a fixed dotted name
like `am.applications.oauth2.client.pega.secret`. A **secret mapping** binds one
such label to an **ESV secret alias** (the ESV secret `_id`, e.g.
`esv-pega-client-secret`). This is the "Secret Mappings" page in the AIC console.

This is the surface to use when you want to know *which ESV secret backs which AM
purpose*, or to re-point a purpose at a different ESV secret.

## Availability in aic-edit

aic-edit exposes secret mappings only on Sandbox and Development tenants. They
are static content that should be edited in lower environments and promoted up
to Staging/Production, not changed per environment. This is an aic-edit product
decision, not an API limitation; the API itself works on any tenant.

> **This contradicts the old blanket claim in [07-secret-stores.md](07-secret-stores.md).**
> The AM secret-store **collection** API (`?_action=nextdescendents`, the bare
> `secrets/stores` listing, `secrets/types`) is still 403 ("not available in
> PingOne Advanced Identity Cloud"). But the **per-store-type subpaths are open**,
> and that is exactly where mappings live. See Quirks.

## Authentication

Service-account bearer, scope `fr:am:*` (we already request it). Verified working
with our own SA token, not just a console admin session.

## Where mappings live

ESV is implemented as a single AM secret store of type
`GoogleSecretManagerSecretStoreProvider`, named **`ESV`**, **per realm**:

```
/am/json/realms/root/realms/{realm}/realm-config/secrets/stores/GoogleSecretManagerSecretStoreProvider/ESV
```

- Only that one store exists. The other four creatable types
  (`FileSystemSecretStore`, `GoogleKeyManagementServiceSecretStore`,
  `HsmSecretStore`, `KeyStoreSecretStore`) are listed by `getCreatableTypes` but
  hold **no** stores (`?_queryFilter=true` → empty result).
- Mappings are **per realm**: `alpha` and `bravo` each have their own
  `…/GoogleSecretManagerSecretStoreProvider/ESV/mappings` (both 200).

Let `STORE = …/realm-config/secrets/stores/GoogleSecretManagerSecretStoreProvider/ESV`.

## Endpoints

All require `Accept-API-Version: protocol=2.0,resource=1.0`.

| Op | Method | Path | Notes |
|----|--------|------|-------|
| List mappings | `GET` | `STORE/mappings?_queryFilter=true` | `{ "result": [ {mapping}, … ] }`. ~33 on the sandbox alpha. |
| Read one | `GET` | `STORE/mappings/{secretId}` | `{secretId, aliases, _id, _rev, _type}`. 404 if unmapped. |
| Field/enum schema | `POST` | `STORE/mappings?_action=schema` | JSON-Schema; `secretId.enum` is the full list of **valid** labels (190 on sandbox). See "Helper text". |
| Starter body | `POST` | `STORE/mappings?_action=template` | Returns `{"aliases":[]}`. |
| Create / update | `PUT` | `STORE/mappings/{secretId}` | Body **must** be `{"aliases":["<one-esv-id>"],"secretId":"<label>"}` (the `secretId` is required in the body — see Quirks). Create → 201, update → 200. |
| Delete | `DELETE` | `STORE/mappings/{secretId}` | **→ 200, echoes the deleted object** (verified 2026-06-17); subsequent `GET` → 404. |
| Store metadata | `GET` | `STORE` | The store config (encoding, GSM project, etc.). |
| Store schema | `POST` | `…/GoogleSecretManagerSecretStoreProvider?_action=schema` | Store-level schema (rarely needed). |

`{secretId}` goes in the path **verbatim** (it contains dots, e.g.
`am.applications.oauth2.client.pega.secret`); no URL-encoding needed on the dots.

## Object shapes

### Mapping (list item / read / write response)

```json
{
  "_id": "am.applications.oauth2.client.pega.secret",
  "_rev": "1171649875",
  "secretId": "am.applications.oauth2.client.pega.secret",
  "aliases": ["esv-pega-client-secret"],
  "_type": { "_id": "mappings", "name": "Mappings", "collection": true }
}
```

- `_id == secretId` (the label). `aliases` is the bound ESV secret id(s).
- `_rev` is **content-derived** (re-PUT identical content → identical `_rev`), like
  authentication trees — so conflict detection is by content snapshot per
  CLAUDE.md §5, NOT `If-Match` (and the agent transport can't send `If-Match`
  anyway).

### Write body (PUT) — `secretId` is REQUIRED in the body

```json
{ "aliases": ["esv-pega-client-secret"], "secretId": "am.applications.oauth2.client.pega.secret" }
```

**The body MUST repeat the `secretId` (the label), not just carry it in the path.**
Omitting it makes the store reject the write with `400 "Invalid config: Secret
value is missing"` — this was the #1 trap (a create with only `{"aliases":[…]}`
fails every time). Verified live 2026-06-17 across many label/alias pairings: with
`secretId` present, create → 201 and update → 200 for *any* ESV secret; without it,
400. (`_id` is optional — `secretId` alone is enough. `If-None-Match` is NOT
needed; the console sends it but it makes no difference.) Strip `_rev` from a
round-tripped body before PUT (content-snapshot hygiene).

## Helper text (the whole point of the feature)

The schema's `secretId` property has `enum`, `enumNames`, and
`options.enum_titles` — but **`enumNames` and `enum_titles` are byte-identical to
`enum`**. The API supplies the raw dotted label and **no human description**.
`secretId.description` is a single generic string ("The secret label that is to be
associated with a Secret Manager secret."). So helper text must be **derived /
curated by us**. Two-tier strategy (verified taxonomy of the 190 sandbox labels):

### Tier 1 — structural derivation (≈132/190, future-proof)

`am.applications.oauth2.client.{client}.{kind}` — the per-OAuth2-client purposes.
`{client}` may itself contain dots (e.g. `alpha.vktest`); parse it as *everything
between `client.` and the known kind suffix*. The four kinds:

| Suffix | Meaning |
|--------|---------|
| `.secret` | Client secret for OAuth2/OIDC client **{client}**. |
| `.jwt.public.key` | Public key verifying signed JWTs from client **{client}** (private_key_jwt auth, signed request objects). |
| `.id.token.enc.public.key` | Public key used to **encrypt ID tokens** issued to client **{client}**. |
| `.mtls.trusted.cert` | Trusted client certificate for **mTLS** authentication by client **{client}**. |

A new OAuth2 client automatically yields good text — no catalogue edit needed.

### Tier 2 — curated catalogue (≈58 stable platform purposes)

These are fixed AM constants; key a static map by exact id (longest-suffix match).
Families on the sandbox:

- `am.services.oauth2.oidc.*` — OIDC provider (AS) JWT signing & ID-token
  encryption/decryption keys (RSA/EC variants, `mtls.client.authentication`).
- `am.services.saml2.metadata.signing.RSA` — SAML2 metadata signing.
- `am.default.applications.federation.entity.providers.saml2.{idp,sp}.{signing,encryption,mtls}`
  — default SAML2 IDP/SP signing & encryption.
- `am.services.selfservice.token.{signing,encryption}` — self-service (password
  reset / username recovery) token signing & encryption.
- `am.authn.authid.signing.HMAC` — authentication-tree `authId` JWT signing.
- `am.authn.trees.transientstate.encryption` — encryption of transient tree state.
- `am.authentication.nodes.persistentcookie.{signing,encryption}` /
  `am.default.authentication.modules.persistentcookie.*` — persistent-cookie
  (remember-me) JWT signing & encryption.
- `am.authentication.nodes.webauthn.fidometadataservice.rootcertificate` /
  `am.services.attestation.google.public.key` — WebAuthn/FIDO attestation roots.
- `am.services.authenticator{oath,push,webauthn}.encryption`,
  `am.services.device{binding,id,profiles}.encryption` — MFA/device data
  encryption.
- `am.applications.agents.ig.secret`,
  `am.applications.agents.remote.consent.request.signing.{ES256,ES384,ES512,RSA}`
  — IG/agent secret & remote-consent request signing.
- `am.services.pushnotification.sns.accesskey.secret` — AWS SNS push credentials.
- `am.services.iot.{cert.verification,jwt.issuer.signing}`,
  `am.services.uma.pct.encryption`, `am.policy.configuration.service.mtls.cert`.

Fallback for anything unrecognised: humanise the dotted id.

## Examples

```bash
STORE="$BASE/am/json/realms/root/realms/alpha/realm-config/secrets/stores/GoogleSecretManagerSecretStoreProvider/ESV"
AV="Accept-API-Version: protocol=2.0,resource=1.0"

# List mappings
curl -s -H "Authorization: Bearer $TOKEN" -H "$AV" "$STORE/mappings?_queryFilter=true"

# Valid labels (enum) for the picker
curl -s -X POST -H "Authorization: Bearer $TOKEN" -H "$AV" "$STORE/mappings?_action=schema"

# Re-point a purpose at a different ESV secret
curl -s -X PUT -H "Authorization: Bearer $TOKEN" -H "$AV" -H "Content-Type: application/json" \
  -d '{"aliases":["esv-pega-client-secret"]}' \
  "$STORE/mappings/am.applications.oauth2.client.pega.secret"
```

## Quirks

- **Per-store-type subpaths are open even though the collection API is 403.**
  Blocked (confirmed 2026-06-17): `secrets/stores?_action=nextdescendents`,
  bare `GET secrets/stores`, `secrets/types`, `global-config/secrets/**`.
  Open: `…/secrets/stores/{TypeId}` and everything under it (the store, its
  schema, `getCreatableTypes`, `/mappings`). The old "secret stores entirely
  unavailable" claim was tested only against the blocked collection calls.
- **Single alias only.** Despite the array schema (`aliases` minItems 1,
  uniqueItems), this store type rejects ≥2 aliases:
  `400 "Only a single alias per mapping is allowed for this secret store type"`.
  Treat `aliases` as a single-value field in the UI.
- **Create / update / delete all work via the obvious verbs** (verified
  2026-06-17): create new → `PUT {label}` → 201; update existing → 200; `DELETE
  {label}` → 200 (echoes the deleted object), `GET` after → 404. No `If-Match`.
- **`400 "Invalid config: Secret value is missing"` means the PUT body omitted
  `secretId` — NOT eventual consistency, NOT a missing/unstaged value.** This was
  a long red herring: a create body of only `{"aliases":[…]}` fails *every time*;
  add `"secretId":"<label>"` and the *exact same* call returns 201, for any ESV
  secret and any label (verified across `esv-56659cc0d1-secret`,
  `esv-pega-client-secret`, etc., and reproduced from the console HAR which always
  includes `secretId`). There is no propagation/staging delay; never advise a
  retry for this error. See "Write body".
- **aic-edit validates the alias against `/environment/secrets` before writing**
  — by product choice (the API accepts any string, including a non-existent name,
  creating a dangling mapping — the footgun the user asked us to prevent). All 33
  existing sandbox mapping aliases are real ESV secrets, and every real ESV secret
  is mappable, so the strict fuzzy picker (no free-text) is correct.
- **`_rev` is content-derived** (idempotent re-PUT → same `_rev`). Use content
  snapshots, not `If-Match`.
- **`secretId` is REQUIRED in the PUT body** (see "Write body"); `_id` is optional.
  Unlike trees/OAuth (which 400 on a body `_id`), here the body carries the label.
- **No descriptions from the API** — `enumNames`/`enum_titles` just repeat the
  raw labels. Helper text is ours to curate (see Helper text).
- **secretId must be in the schema `enum`.** You can only map labels AM
  advertises; you can't invent arbitrary purposes.

## Verified against

- Tenant: `tenant.example.com`, realm `alpha`.
- Date: **2026-06-17** (HAR capture `updating-esv-mapping.har` from the console,
  re-verified live with our SA token).
- Live with our SA token (read): `GET STORE`, `GET STORE/mappings?_queryFilter=true`
  (33 results), `GET STORE/mappings/{id}`, `POST …?_action=schema` (190-entry enum),
  `POST …?_action=template` → `{"aliases":[]}`. All 200.
- Live with our SA token (write): idempotent re-PUT of
  `am.applications.oauth2.client.pega.secret` → `["esv-pega-client-secret"]` →
  **200, `_rev` unchanged** (`1171649875`). No net change made.
- **`secretId`-in-body requirement (the key finding):** create with body
  `{"aliases":[alias]}` (no `secretId`) → **400 "Secret value is missing"** every
  time; the *same* call with `{"aliases":[alias],"secretId":"<label>"}` → **201**.
  Confirmed across `vktest.secret`/`vktest.mtls.trusted.cert`/`saml2.metadata.
  signing.RSA` against `esv-56659cc0d1-secret`, `esv-pega-client-secret`,
  `esv-3d06f2834c-oauth2clientsecret` — i.e. any label/alias pairing. Isolation:
  `secretId` in body alone → 201; `If-None-Match:*` alone (no `secretId`) → 400.
  Matches the console HAR `adding-and-removing-esv-mapping2.har`, whose PUT body
  includes `secretId`.
- Other write errors (no config left behind): two aliases →
  `400 "Only a single alias per mapping is allowed…"`.
- Add/remove lifecycle (left clean at 33 mappings): `PUT {label}
  {"aliases":[alias],"secretId":label}` → **201** → GET 200 → DELETE 200 → GET 404.
- Store enumeration: only `GoogleSecretManagerSecretStoreProvider/ESV` exists;
  other four types empty. `bravo` realm `…/ESV/mappings` → 200 (per-realm).
- Console HAR also shows `?_action=getCreatableTypes` and the store-level
  `?_action=schema`/`?_action=getCreatableTypes` returning 200.

## Source citations

- Console network captures: `updating-esv-mapping.har` (list + two PUT updates of
  `am.applications.oauth2.client.serviceAlfa.secret`),
  `adding-and-removing-esv-mapping.har` (PUT create 201 + DELETE 200 of
  `…vktest.jwt.public.key`), and `adding-and-removing-esv-mapping2.har` (the
  `…vktest.secret` create whose body carries `secretId` → 201, the call that
  pinned down the body requirement).
- frodo-lib: `src/api/SecretStoreApi.ts` (the store/mapping CREST shape; frodo
  assumes self-managed AM but the per-type subpaths match AIC here).

## Open questions

- **`getCreatableTypes` lists 5 store types** — can a service account actually
  `POST`-create a new store of those types, or is creation console-only? Untested;
  out of scope (ESV is the only store we need).
