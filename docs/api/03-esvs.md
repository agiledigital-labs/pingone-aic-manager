# 03 — ESVs (Environment Secrets & Variables)

## Purpose
ESVs are the AIC-native way to inject environment-specific config (URLs,
credentials, feature flags) into your tenant. Variables are mutable scalars;
secrets are versioned, encrypted-at-rest values. After changing any ESV that
the runtime has already loaded, you must trigger a tenant restart for the new
value to take effect. Feature 1 of aic-edit ("edit and apply ESVs") is built
entirely on this API.

## Authentication
Service-account bearer. Scopes: `fr:idc:esv:*` (or finer-grained
`fr:idc:esv:read` / `fr:idc:esv:update` / `fr:idc:esv:restart`).

## Endpoints

### Variables (tenant-global, **no realm in path**)

| Op | Method | Path | Notes |
|----|--------|------|-------|
| List | `GET` | `/environment/variables` | Paged; returns `result`, `pagedResultsCookie`. |
| Read | `GET` | `/environment/variables/{id}` | `id` is the `esv-…` ID, not the human name. |
| Upsert | `PUT` | `/environment/variables/{id}` | Body: `{ "valueBase64": "…", "expressionType": "string", "description": "…" }` |
| Set description | `POST` | `/environment/variables/{id}?_action=setDescription` | Body: `{ "description": "…" }` |
| Delete | `DELETE` | `/environment/variables/{id}` | Permanent. |

### Secrets

| Op | Method | Path | Notes |
|----|--------|------|-------|
| List | `GET` | `/environment/secrets` | Includes `activeVersion`, `loadedVersion`. |
| Read | `GET` | `/environment/secrets/{id}` | Metadata only — values never returned. |
| Create | `PUT` | `/environment/secrets/{id}` | Body sets `encoding`, initial `valueBase64`. Encoding is immutable after creation. |
| Set description | `POST` | `/environment/secrets/{id}?_action=setDescription` | |
| Delete (all versions) | `DELETE` | `/environment/secrets/{id}` | Permanent. |
| List versions | `GET` | `/environment/secrets/{id}/versions` | |
| Create new version | `POST` | `/environment/secrets/{id}/versions?_action=create` | Body: `{ "valueBase64": "…" }`. Returns `version`. |
| Set version status | `POST` | `/environment/secrets/{id}/versions/{v}?_action=changestatus` | Body: `{ "status": "ENABLED"\|"DISABLED"\|"DESTROYED" }` |
| Delete version | `DELETE` | `/environment/secrets/{id}/versions/{v}` | |

### Startup / restart

| Op | Method | Path | Notes |
|----|--------|------|-------|
| Check status | `GET` | `/environment/startup` | Returns `{"restartStatus":"ready"\|"restarting"}`. |
| Trigger restart | `POST` | `/environment/startup?_action=restart` | Returns `{"restartStatus":"restarting"}`. |

**Always** `GET /environment/startup` first and abort if status is not `ready`.

## Object shapes (verified from sandbox)

### Variable (list & single)

```json
{
  "_id": "esv-3d06f2834c-tenanturl",
  "description": "Script variable TENANT_URL in file …",
  "expressionType": "string",
  "lastChangeDate": "2024-07-29T02:15:58.519991Z",
  "lastChangedBy": "<user@example.com>",
  "loaded": true,
  "valueBase64": "aHR0cHM6Ly9hcHAuZXhhbXBsZS5jb20="
}
```

- **No `_rev` field.** Conflict detection must be content-based.
- `loaded` is `true` once the runtime has picked it up (i.e. since the last restart).
- `expressionType` ∈ `string | array | bool | int | number | object | list | keyvaluelist | base64encodedinlined`.

### Secret (list & single — value never returned)

```json
{
  "_id": "esv-02249e160b-datahmacsigningkey",
  "activeVersion": "2",
  "loadedVersion": "2",
  "encoding": "generic",
  "useInPlaceholders": true,
  "description": "Configuration parameter /data/hmacSigningKey in file …",
  "lastChangeDate": "2024-06-27T01:05:06.682326Z",
  "lastChangedBy": "<user@example.com>",
  "loaded": true
}
```

- `encoding` ∈ `generic | pem | base64hmac | base64aes`. Set at create; immutable.
- `activeVersion` = the version the runtime will pick up at next restart.
- `loadedVersion` = the version currently in memory.
- If `activeVersion != loadedVersion`, a restart is pending.

### Secret version

```json
{
  "_id": "...",
  "version": "3",
  "createDate": "...",
  "loaded": false,
  "status": "ENABLED"   // or DISABLED, DESTROYED
}
```

## ID convention

ESV IDs follow the pattern `esv-{10-hex-hash}-{lowercased-name}`. The hash
appears to be derived from the source file path (config location that originally
referenced the value). When **creating** a new ESV, the caller chooses the full
ID — pick something descriptive like `esv-custom-myvarname`.

## Examples

```bash
# List all variables (paginated; default 1000)
$SCRIPTS/verify-endpoint.sh "/environment/variables"

# Read one
$SCRIPTS/verify-endpoint.sh "/environment/variables/esv-3d06f2834c-tenanturl"

# Create / update (curl skeleton)
curl -X PUT "$TENANT_BASE_URL/environment/variables/esv-custom-greeting" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"valueBase64":"aGVsbG8=","expressionType":"string","description":"demo"}'

# Restart status / restart
$SCRIPTS/verify-endpoint.sh "/environment/startup"
$SCRIPTS/verify-endpoint.sh "/environment/startup?_action=restart" -X POST
```

## Quirks

- **Values are base64-encoded** in `valueBase64`, even for plain strings. Decode
  for display; encode for write.
- **Variables have no `_rev`** — conflict detection requires `lastChangeDate`
  comparison or content equality before push.
- **Secret values are write-only.** Once set, the API never returns the
  plaintext. To "see" a secret, expose it via a journey or test client.
- **Versioning is secret-only.** Variables are single-value (no version history).
- **`useInPlaceholders: true`** is what makes a secret referenceable as
  `&{esv.secret-id}` in config. Default true for new secrets.
- **Restart is tenant-wide** and takes a few minutes; the UI shows live progress.
  All in-flight sessions survive but new operations get the refreshed values.
- **You cannot rename an ESV.** The `_id` is the identifier; to "rename", create
  a new one and delete the old.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-05-17
- Calls: `GET /environment/variables`, `GET /environment/variables/{id}`,
  `GET /environment/secrets`, `GET /environment/startup`. All 200 OK.
- Negative: `GET /environment/esv` → 404. (Ping docs' "ESV aggregate" path
  doesn't exist on AIC.)

## Source citations

- frodo-lib: `src/api/cloud/VariablesApi.ts`, `src/api/cloud/SecretsApi.ts`,
  `src/api/cloud/StartupApi.ts`.
- fr-config-manager: `packages/fr-config-pull/src/scripts/{variables,secrets}.js`,
  `packages/fr-config-push/src/scripts/update-secrets.js`.
- Ping docs: <https://docs.pingidentity.com/pingoneaic/latest/tenants/esvs-manage-api.html>

## Open questions

- Exact ID-prefix rules for *user-created* (vs system-generated) ESVs — does
  AIC enforce the `esv-` prefix? Test by `PUT /environment/variables/foobar`.
- Rate limits on `_action=restart` — is there a cool-down? Probably yes;
  capture the 429 response shape when we hit it.
