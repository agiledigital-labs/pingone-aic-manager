# 05 — OAuth2 / OIDC

## Purpose
Manage OAuth2 clients (also called "agents" in AM-speak) and the realm-wide
OAuth2/OIDC provider service. Feature 3 of aic-edit ("manage OIDC and SAML
config") is partly built on this API.

## Authentication
Service-account bearer. Scope: `fr:am:*`.

## Endpoints

Replace `{realm-path}` with `/realms/root/realms/alpha` (or `bravo`).
Always send `Accept-API-Version: protocol=2.1,resource=1.0`.

### OAuth2 clients (per-agent)

| Op | Method | Path | Notes |
|----|--------|------|-------|
| List | `GET` | `/am/json{realm-path}/realm-config/agents/OAuth2Client?_queryFilter=true` | |
| Read | `GET` | `/am/json{realm-path}/realm-config/agents/OAuth2Client/{id}` | `id` is the client_id string. |
| Upsert | `PUT` | `/am/json{realm-path}/realm-config/agents/OAuth2Client/{id}` | See "Update quirks" below. |
| Delete | `DELETE` | `/am/json{realm-path}/realm-config/agents/OAuth2Client/{id}` | |

### OAuth2 / OIDC provider service (realm-wide)

| Op | Method | Path | Notes |
|----|--------|------|-------|
| Read | `GET` | `/am/json{realm-path}/realm-config/services/oauth-oidc` | Full provider config. |
| Create | `POST` | `/am/json{realm-path}/realm-config/services/oauth-oidc?_action=create` | Only if not yet provisioned. |
| Update | `PUT` | `/am/json{realm-path}/realm-config/services/oauth-oidc` | Full body. |

### Other agent types (same endpoint shape, different `{agentType}`)

`/am/json{realm-path}/realm-config/agents/{agentType}/...` where `{agentType}` ∈

- `OAuth2Client` (most common)
- `WebAgent`, `J2EEAgent`, `IdentityGatewayAgent`, `RemoteConsentAgent`
- `SoftwarePublisher`, `TrustedJwtIssuer`, `OAuth2ClientNG`

## Object shape (real OAuth2 client from sandbox, abbreviated)

```json
{
  "_id": "myapp-client",
  "_rev": "1364633644",
  "overrideOAuth2ClientConfig": { /* per-client OAuth2 overrides */ },
  "advancedOAuth2ClientConfig": {
    "subjectType": "Public",
    "responseTypes": ["token"],
    "tokenEndpointAuthMethod": "client_secret_post",
    "grantTypes": ["client_credentials"],
    "isConsentImplied": true,
    /* ... many more fields */
  },
  "coreOAuth2ClientConfig": {
    "clientName": [],
    "clientType": "Confidential",
    "scopes": [/* ... */],
    "userpassword": null,
    "userpassword-encrypted": "AQIC..."
  },
  "signEncOAuth2ClientConfig": { /* signing & encryption keys */ },
  "coreOpenIDClientConfig": { /* OIDC-specific */ },
  "coreUmaClientConfig": { /* UMA */ },
  "_type": { "_id": "OAuth2Client", "name": "OAuth2 Clients", "collection": true }
}
```

- **Has `_rev`** — use `If-Match` for optimistic locking on `PUT`.
- Many fields are wrapped in `{"inherited": true|false, "value": …}` to indicate
  override of provider defaults.

## OIDC provider service shape (real, from sandbox)

```json
{
  "_id": "",
  "_rev": "-129686093",
  "advancedOIDCConfig": { /* JWE algorithms, supported claims, etc. */ },
  "coreOIDCConfig": { /* base OIDC */ },
  "advancedOAuth2Config": { /* token signing, refresh policy */ },
  "coreOAuth2Config": { /* access token lifetime, grant types allowed */ },
  "clientDynamicRegistrationConfig": { /* DCR */ },
  "consent": { /* consent screen */ },
  "cibaConfig": { /* CIBA */ },
  "deviceCodeConfig": { /* device code grant */ },
  "pluginsConfig": { /* scope plugins, etc. */ }
}
```

## Update quirks (critical for PUT)

When mutating an OAuth2 client, before sending the `PUT` body:

1. **Strip `_provider`** if present — read-only, server rejects.
2. **Strip all `*-encrypted` fields.** These hold AES-wrapped values whose
   transport key differs per cluster — sending them back produces gibberish
   secrets. frodo-lib walks the object and removes any key ending in
   `-encrypted` (`deleteDeepByKey`). We must do the same.
3. **Keep `_rev`** — server uses it to detect concurrent modification.
4. **Decide on `userpassword` etc.**: if the corresponding `-encrypted` was
   stripped, leave the plain field as-is (the server keeps the existing
   ciphertext when the plain field is null/unset).

Failing to strip `-encrypted` is the #1 way to silently corrupt OAuth2 client
secrets. Build this into the Rust client as a hard pre-flight.

## Examples

```bash
# List first OAuth2 client in alpha
$SCRIPTS/verify-endpoint.sh \
  "/am/json/realms/root/realms/alpha/realm-config/agents/OAuth2Client?_queryFilter=true&_pageSize=1" \
  --header "Accept-API-Version: protocol=2.1,resource=1.0"

# Read OIDC provider service
$SCRIPTS/verify-endpoint.sh \
  "/am/json/realms/root/realms/alpha/realm-config/services/oauth-oidc" \
  --header "Accept-API-Version: protocol=2.1,resource=1.0"
```

## Quirks

- **Inherited values.** A field shown as `{"inherited": true, "value": [...]}`
  means it falls through to the provider service. To override, set `"inherited":
  false` and put the local value. Reading back will show the override.
- **`_id` is the client_id.** No separate `name` field.
- **Provider `_id` is empty string.** That's intentional — there's one provider
  service per realm.
- **`_rev` is a stringified int that goes negative** (`"-129686093"`). Treat as
  opaque string.
- **`coreUmaClientConfig`** present even on non-UMA clients with empty fields —
  don't strip it.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-05-17
- Calls: `GET …/realm-config/agents/OAuth2Client?_queryFilter=true&_pageSize=1`
  (200 OK, `_rev` present, multiple `-encrypted` fields observed),
  `GET …/realm-config/services/oauth-oidc` (200 OK).

## Source citations

- frodo-lib: `src/api/OAuth2ClientApi.ts` (lines 130-134 for `deleteDeepByKey`),
  `src/api/OAuth2ProviderApi.ts`.
- fr-config-manager: `packages/fr-config-pull/src/scripts/oauth2Agents.js`,
  `packages/fr-config-push/src/scripts/update-agents.js`.
- Ping docs: <https://docs.pingidentity.com/pingoneaic/latest/am-oauth2/rest-api-oauth2-client-admin-endpoint.html>

## Open questions

- Does `?_action=create` on the provider service work, or is it auto-provisioned?
  In alpha it's already there.
- For "create new OAuth2 client", does `PUT` with a non-existent `_id` create,
  or do we need `POST ?_action=create`? Probably PUT-upsert; verify before
  implementing the create-flow UI.
