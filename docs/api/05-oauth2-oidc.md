# 05 — OAuth2 / OIDC

Implemented in: `src/oauth/`

## Purpose
Manage OAuth2 clients (also called "agents" in AM-speak) and the realm-wide
OAuth2/OIDC provider service. Feature 3 of pingone-aic-manager ("manage OIDC and SAML
config") is partly built on this API.

## Authentication
Service-account bearer. Scope: `fr:am:*`.

## Endpoints

Replace `{realm-path}` with `/realms/root/realms/alpha` (or `bravo`).
Always send `Accept-API-Version: protocol=2.1,resource=1.0`.

### OAuth2 clients (per-agent)

| Op | Method | Path | Notes |
|----|--------|------|-------|
| List | `GET` | `/am/json{realm-path}/realm-config/agents/OAuth2Client?_queryFilter=true` | Use `_fields=_id` for id-only lists; pass a large `_pageSize` and follow non-empty `pagedResultsCookie` with `_pagedResultsCookie`. |
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

- **Has `_rev`** — treat it as opaque metadata. OAuth2 client writes use
  plain `PUT` without `If-Match`; conflict detection is by local content
  snapshot.
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

1. **Strip top-level `_id`, `_rev`, `_type`, and `_provider`.** The server
   rejects a `PUT` body containing these server-managed fields with
   `400 {"message":"Invalid attribute specified."}`. `_rev` must not be kept,
   and OAuth2 client update does not use `If-Match`.
2. **Strip all `*-encrypted` fields recursively.** These hold AES-wrapped values whose
   transport key differs per cluster — sending them back produces gibberish
   secrets. frodo-lib walks the object and removes any key ending in
   `-encrypted` (`deleteDeepByKey`). We must do the same. On the 2026-06-14
   tenant version, a freshly-set `userpassword` read back as `null` with no
   `userpassword-encrypted` sibling; the strip is still mandatory because this
   echo behavior is version-dependent.
3. **Use plain `PUT` for create and update.** No `If-Match` or `If-None-Match`
   header is needed. `PUT` to a new id creates the client and returns 201;
   `PUT` to an existing id updates and returns 200.
4. **Decide on `userpassword` etc.**: if the corresponding `-encrypted` was
   stripped or absent, leave the plain field as-is. `null`/unset preserves the
   existing write-only secret on this tenant version.

Failing to strip `-encrypted` is the #1 way to silently corrupt OAuth2 client
secrets. Build this into the Rust client as a hard pre-flight.

## Editing OAuth2 clients — pull / edit / push (verified 2026-06-14)

OAuth2 client edits are managed as JSON files under the workspace:

```bash
aic oauth pull service_C1
$EDITOR workspace/<tenant>/oauth/alpha/service_C1.json
aic oauth push service_C1
```

`pull` writes both the editable export and the last-synced snapshot:

- Export: `workspace/<tenant>/oauth/{realm}/{id}.json`
- Snapshot: `workspace/<tenant>/oauth/{realm}/.snapshots/{id}.json`

`push` reads the local export, fetches the remote client, and compares remote
content to the snapshot after stripping `_rev` recursively. If remote still
matches the snapshot, the local file is safe to push. If remote has drifted,
the command blocks and asks the user to re-pull or pass `--force`. `--force`
overwrites remote with the local export and then refreshes only the snapshot;
it does not clobber the user's local file.

If the remote client id does not exist, `aic oauth push <id>` creates it with
plain `PUT …/OAuth2Client/{id}`. A successful create returns 201. After create,
the CLI re-reads the client and stores that as the snapshot.

Delete is explicit and non-interactive:

```bash
aic oauth delete service_C1 --force
```

Delete removes the remote client and any local snapshot, but leaves the editable
export file in place.

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
- Date: 2026-06-14
- Calls: `PUT …/realm-config/agents/OAuth2Client/test_oauth_probe` with no
  `If-Match` created the throwaway client (201), a second plain `PUT` updated
  it (200), `DELETE` removed it (200), and a follow-up `GET` returned 404.
  Sending server-managed top-level fields (`_id`, `_rev`, `_type`) in the PUT
  body produced `400 {"message":"Invalid attribute specified."}`. The
  throwaway `test_oauth_probe` client was cleaned up. A freshly-set
  `userpassword` read back as `null` with no `userpassword-encrypted` sibling
  on this tenant version.
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

## Open questions / out of scope

- Does `?_action=create` on the provider service work, or is it auto-provisioned?
  In alpha it's already there. This remains untested and out of scope for
  OAuth2 client pull/push.
- OAuth2 client create is resolved: `PUT` with a non-existent id creates the
  client and returns 201.
