# AIC API Documentation (local)

This directory is the canonical reference for the PingOne Advanced Identity
Cloud APIs that `pingone-aic-manager` calls. **Read the relevant file before
writing any code that hits a tenant.** Don't guess paths or headers.

## Files

| File                                                               | Covers                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| ------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [00-auth.md](00-auth.md)                                           | Service-account JWT bearer grant flow, token caching                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| [01-realms-and-paths.md](01-realms-and-paths.md)                   | Realm path conventions, base URL composition                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| [02-headers-and-versioning.md](02-headers-and-versioning.md)       | `Accept-API-Version` cheat sheet                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| [03-esvs.md](03-esvs.md)                                           | Variables, secrets, versions, startup/restart                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| [04-scripts.md](04-scripts.md)                                     | All script contexts, base64 body, no `_rev`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| [05-oauth2-oidc.md](05-oauth2-oidc.md)                             | OAuth2 clients (agents), OIDC provider service                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| [06-saml.md](06-saml.md)                                           | Hosted/remote entities, CoT, metadata XML                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| [07-secret-stores.md](07-secret-stores.md)                         | Store _collection_ API not available — but per-type subpaths (mappings) are; see 15                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| [08-logs.md](08-logs.md)                                           | `/monitoring/logs`, admin-token-only key minting, source taxonomy, journey join key; local store in [../logs-store.md](../logs-store.md)                                                                                                                                                                                                                                                                                                                                                                                         |
| [09-journeys.md](09-journeys.md)                                   | Auth trees, nodes, custom nodes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| [10-managed-objects.md](10-managed-objects.md)                     | IDM managed config, hooks                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| [11-idm-endpoints.md](11-idm-endpoints.md)                         | IDM custom endpoints + scheduled jobs (`config/endpoint/*`, `config/schedule/*`), plaintext `source`, no `_rev`                                                                                                                                                                                                                                                                                                                                                                                                                  |
| [12-script-bindings-matrix.md](12-script-bindings-matrix.md)       | Runtime-verified Rhino 1.7.14 syntax matrix + per-context binding tables (drives the workspace lint/type templates)                                                                                                                                                                                                                                                                                                                                                                                                              |
| [13-script-contexts.md](13-script-contexts.md)                     | AM `contexts/{id}` endpoint — authoritative per-context binding metadata (drives the `.d.ts` types)                                                                                                                                                                                                                                                                                                                                                                                                                              |
| [14-am-identity-attributes.md](14-am-identity-attributes.md)       | IDM-property → AM-attribute-name mapping for the `identity`/`idRepository` binding (drives typed `identity`); `getIdentity` resolves by uuid                                                                                                                                                                                                                                                                                                                                                                                     |
| [15-secret-mappings.md](15-secret-mappings.md)                     | ESV secret → AM secret-label (purpose) mappings; per-type secret-store subpath; curated helper text per label                                                                                                                                                                                                                                                                                                                                                                                                                    |
| [16-sync-mappings.md](16-sync-mappings.md)                         | IDM `config/sync` mappings; embedded behaviour/correlation/result/transform/condition scripts; whole-doc RMW PUT (no `_rev`); runtime-verified per-slot binding surface for typing; recon endpoints; **queued (async) implicit sync + the `/openidm/sync/queue` endpoint (view/count/clear, throughput, why backlogs stall)**                                                                                                                                                                                                    |
| [17-jwt-bearer-user-tokens.md](17-jwt-bearer-user-tokens.md)       | Trusted JWT Issuer (`agents/TrustedJwtIssuer`) — mint an access token **as an end user** from a locally-held RSA key, no journey or password; per-agent keys in one shared `jwkSet`; `allowedSubjects` is the whole security boundary                                                                                                                                                                                                                                                                                            |
| [18-internal-roles.md](18-internal-roles.md)                       | IDM **internal roles** (`/openidm/internal/role`) — the authorization roles `config/access` and `config/authentication` refer to; **`PUT /{id}` creates with an `_id` you choose** instead of the console's random UUID; destructive-replace `PUT`, mandatory privilege fields, and the schema's misspelled `accessFlags`; why a missing role reference is only a warning                                                                                                                                                        |
| [19-config-access.md](19-config-access.md)                         | **`config/access`** — the rule list gating `/openidm` routes for the identities it governs (**not all of them** — a scoped bypass means our own bearer is only partly subject to it). `configs` is a **disjunction**, so an appended rule can never revoke access and `edit`/`rm` are the dangerous verbs; no `_rev`, so content is the only precondition; `actions` is **optional** and must not be synthesised; the service-account bearer is only partly governed by these rules, so a change cannot be confirmed empirically |
| [20-config-authentication.md](20-config-authentication.md)         | **`config/authentication`** — the rsFilter document. Whole-document PUT, no `_rev`; `staticUserMapping[]` entries have no id (content hash is the identity); `roles` is an **array** (unlike access); `RCSClient` omits `roles`; an appended mapping RMW leaves siblings and the rest of `rsFilter` byte-identical |
| [bindings/](bindings/)                                             | Raw per-context binding probe results (JSON) backing files 10, 12–13                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| [99-quirks-and-open-questions.md](99-quirks-and-open-questions.md) | Cross-cutting weirdness + TODOs                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |

## Doc template

Every capability file follows this structure:

```markdown
# {Capability area}

## Purpose

1–2 sentences: what this API family does, when pingone-aic-manager uses it.

## Authentication

Which auth (service-account bearer / log API key) and which scopes.

## Endpoints

| Op | Method | Path template | Accept-API-Version | Notes |

## Object shapes

JSON skeletons (annotated) for requests & responses.

## Examples

Fenced curl/JSON snippets working against the sandbox. (`${TOKEN}` and
`${TENANT_BASE_URL}` are placeholders.)

## Quirks

Non-obvious behaviors a future implementer will trip on.

## Verified against

- Tenant + date + which endpoints were actually exercised.

## Source citations

- frodo-lib path + fr-config-manager path + docs.pingidentity.com URL.

## Open questions

Things not yet verified.
```

## Updating these docs

1. **Verify first.** Run `scripts/verify-endpoint.sh <path> [--header ...]`
   against the sandbox to confirm behavior. Capture real response shape.
2. **Update the relevant file** with: new endpoint, new field, new header
   requirement, observed quirk. Always update "Verified against" with today's
   date.
3. **If observed behavior contradicts the doc**, trust observation. Update doc.
   Note the change in `99-quirks-and-open-questions.md` with the date.
4. **Never** transcribe a frodo-lib / fr-config-manager / Ping docs claim
   without verifying it first. The libraries have stale claims (see Q1, Q2 in
   99-…).

## Conventions used in this doc set

- Paths are written relative to `${TENANT_BASE_URL}` (e.g.
  `https://<your-tenant>.forgeblocks.com`).
- `{realm}` in path templates means `alpha` or `bravo` — see
  `01-realms-and-paths.md` for how to build the full path segment.
- All response shapes shown are abbreviated. Run the verify script to see the
  full thing; don't add fields you haven't seen returned.
