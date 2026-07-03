# 03 — ESVs (Environment Secrets & Variables)

Implemented in: `src/esv/`

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
| List pending | `GET` | `/environment/variables?_onlyPending=true` | Variables Ping says need restart/apply. |
| Read | `GET` | `/environment/variables/{id}` | `id` is the `esv-…` ID, not the human name. |
| Upsert | `PUT` | `/environment/variables/{id}` | Body: `{ "valueBase64": "…", "expressionType": "string", "description": "…" }` |
| Set description | `POST` | `/environment/variables/{id}?_action=setDescription` | Body: `{ "description": "…" }` |
| Delete | `DELETE` | `/environment/variables/{id}` | Permanent. |
| Count pending | `GET` | `/environment/count?_onlyPending=true` | Returns counts by resource type, e.g. `{ "variables": 1, "secrets": 0 }`. |

### Secrets

Verified live 2026-05-30 (full create/version/status/delete lifecycle on
throwaway `esv-aicedit-sec*` secrets). Several earlier (library-transcribed)
claims in this table were **wrong** — corrected below.

| Op | Method | Path | Notes |
|----|--------|------|-------|
| List | `GET` | `/environment/secrets` | `{ "result": [...] }` wrapper. Includes `activeVersion`, `loadedVersion`. |
| List pending | `GET` | `/environment/secrets?_onlyPending=true` | Secrets whose `activeVersion != loadedVersion` AND `useInPlaceholders=true`. |
| Read | `GET` | `/environment/secrets/{id}` | Metadata only — values never returned. |
| **Create** | `PUT` | `/environment/secrets/{id}` | **Create-only, NOT upsert.** PUT on an existing id → `400 "Failed to create secret, the secret already exists"`. Body **requires** `encoding`, `useInPlaceholders`, `valueBase64` (and optional `description`). |
| Set description | `POST` | `/environment/secrets/{id}?_action=setDescription` | Body `{ "description": "…" }` → **`200` with an EMPTY body** (does NOT echo the object — earlier claim was wrong, corrected 2026-05-31). **Only action allowed** on a secret. |
| Delete (all versions) | `DELETE` | `/environment/secrets/{id}` | Permanent; removes the whole secret. |
| List versions | `GET` | `/environment/secrets/{id}/versions` | Returns a **bare JSON array** (no `result` wrapper), newest-first. |
| Create new version | `POST` | `/environment/secrets/{id}/versions?_action=create` | Body `{ "valueBase64": "…" }`. Returns the new version object. New version is auto-`ENABLED` and becomes `activeVersion`. |
| Set version status | `POST` | `/environment/secrets/{id}/versions/{v}?_action=changestatus` | Body `{ "status": "ENABLED"\|"DISABLED" }` only — **`DESTROYED` is rejected here**. Cannot disable the latest (highest) version → `400 "Cannot disable latest secret version"`. |
| Destroy version | `DELETE` | `/environment/secrets/{id}/versions/{v}` | Sets the version's `status` to `DESTROYED` (irreversible). The version **stays in the list** as `DESTROYED`; it is not removed. |

**There is no API to change `encoding` or `useInPlaceholders` after creation**
— PUT is create-only and `setDescription` is the only action. Both are
effectively immutable post-create; to change them, delete and recreate.

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
  - `generic` — any bytes; `valueBase64` = base64(raw value).
  - `pem` — decoded bytes must be a valid PEM document; invalid → `500 "Failed to create secret"`.
  - `base64hmac` / `base64aes` — **double-encoded**: the value is itself a
    base64-encoded key string, so `valueBase64` = base64(base64-key-string).
    The inner content must be valid base64 (and a valid AES key length for aes);
    invalid → `500 "Failed to create secret"`.
- `useInPlaceholders` — **required on create** (no server default; omitting it →
  `400 … property "useInPlaceholders" is missing`). When `true`, the secret is
  referenceable as `&{esv.secret-id}` and a new active version stays `loaded:false`
  until a restart (it counts toward the pending/apply gate). When `false`, the
  secret is `loaded:true` immediately on create/new-version and **never appears
  pending** — no restart needed. (Confirmed: a `useInPlaceholders:false` create
  did not bump `/environment/count?_onlyPending=true`.)
- `activeVersion` = the highest-numbered `ENABLED` version (auto-updates as
  versions are added/enabled). The runtime picks this up at next restart.
- `loadedVersion` = the version currently in memory; `""` when nothing is loaded yet.
- If `useInPlaceholders=true` and `activeVersion != loadedVersion`, a restart is pending.

### Secret version

Versions list is a bare array, newest-first. **No `_id` field** (the earlier
doc claim was wrong). Fields:

```json
{
  "version": "3",
  "createDate": "2026-05-30T05:48:30.490094Z",
  "loaded": false,
  "status": "ENABLED"   // ENABLED | DISABLED | DESTROYED
}
```

- Multiple versions can be `ENABLED` at once (observed v1 and v2 both ENABLED on
  a real secret). Scripts resolve `&{esv.id}` to the latest active version; an
  OAuth2 client secret matches against *any* ENABLED version.
- Status transitions: `ENABLED`↔`DISABLED` via `changestatus`; `DESTROYED` only
  via `DELETE` on the version (one-way). The latest version can't be disabled.

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
- **`useInPlaceholders`** makes a secret referenceable as `&{esv.secret-id}`.
  It is **required on create** (no default) and there is **no API to change it
  afterwards**. `false` ⇒ loaded immediately, never gates a restart.
- **Secret `PUT` is create-only.** Unlike variables (where PUT is an upsert),
  re-`PUT`ting an existing secret id fails. Change a secret's value by adding a
  new **version** (`POST …/versions?_action=create`), not by PUT.
- **`changestatus` only does ENABLED/DISABLED.** Destroying a version is a
  separate `DELETE …/versions/{v}` and is irreversible; the destroyed version
  stays listed with `status:"DESTROYED"`. The latest version cannot be disabled.
- **Restart is tenant-wide** and takes a few minutes; the UI shows live progress.
  All in-flight sessions survive but new operations get the refreshed values.
- **Deletes are not reported as pending variables.** Verified 2026-05-29:
  deleting `esv-test2` returned the previous body, subsequent `GET` returned
  404, and both `/environment/variables?_onlyPending=true` and
  `/environment/count?_onlyPending=true` reported zero pending variables.
  Recreating the same body returned `loaded=false` and the pending endpoints
  reported `esv-test2`.
- **A create/update is reported pending; a delete cancels it.** Verified
  2026-05-30: creating `esv-aicedit-deltest` returned `loaded=false` and bumped
  `/environment/count?_onlyPending=true` from `variables:1` to `variables:2`.
  Deleting it (while still `loaded=false`, i.e. never applied) returned the
  body, `GET` then 404'd, and the count dropped back to `variables:1` with the
  id absent from `?_onlyPending=true`. So deleting a not-yet-applied create
  simply removes its pending entry.
- **Decision: aic-edit treats deletes as immediate (no apply gate).** Ping's
  pending/count endpoints structurally cannot report a delete (the row is gone),
  and Ping's own tooling never prompts a restart after a delete, so we match it:
  delete tombstones are *not* counted toward `pending_count` / the `^S` restart
  gate. They remain visible as red `!` ghost rows only as a local undo
  affordance (TTL `DELETE_TOMBSTONE_TTL`, 300s), independent of apply state. The
  management API gives no way to observe whether the running tenant still serves
  a deleted-but-previously-loaded value, so this is a deliberate product choice,
  not a measured fact.
- **You cannot rename an ESV.** The `_id` is the identifier; to "rename", create
  a new one and delete the old.
- **`expressionType` is "immutable" via `PUT` only.** An in-place `PUT` that
  changes `expressionType` is rejected with `400 {"message":"Changing the type
  of an existing variable is not permitted"}`. **However**, a `DELETE` followed
  by an immediate `PUT` with the new type works, with no restart required
  between the two — verified on the sandbox 2026-05-26 (string → int round-trip
  via `esv-aicedit-typetest`, response 200 both times). aic-edit takes this
  path automatically when a save changes the type.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-05-26 (variables); 2026-05-30 (full secrets lifecycle);
  2026-05-31 (`setDescription` returns empty `200`)
- Calls: `GET /environment/variables`, `GET /environment/variables/{id}`,
  `GET /environment/secrets`, `GET /environment/startup`. All 200 OK.
- Secrets lifecycle (2026-05-30), throwaway `esv-aicedit-sec*`, all cleaned up:
  - Create requires `useInPlaceholders` (omitted → 400). PUT on existing → 400
    "already exists" (create-only).
  - New version via `POST …/versions?_action=create` → auto-ENABLED, bumps
    `activeVersion`. Versions list is a bare array, no `_id`.
  - `changestatus DISABLED` on latest → 400; on non-latest → 200. `DESTROYED`
    via changestatus → 400; via `DELETE …/versions/{v}` → 200, leaves status
    DESTROYED.
  - `useInPlaceholders:true` create → `loaded:false`, `secrets:1` pending;
    `useInPlaceholders:false` create → `loaded:true`, not pending.
  - Encodings: `generic`/`base64aes` (valid 16-byte key) → 200; `pem` (valid
    cert) → 200; `pem`/`base64hmac` with invalid content → 500.
- `setDescription` (2026-05-31, on throwaway `esv-secret-test1`): `POST
  …?_action=setDescription` with `{"description":"…"}` → **`200` with a
  zero-byte body**. The earlier "echoes object" claim was wrong. A strict
  JSON-decode of the success body therefore fails ("error decoding response
  body"); `AicClient::check_response` now maps an empty `200` to JSON null.
- Type-change round-trip on `/environment/variables/{id}`:
  `PUT (string → int)` → 400 "Changing the type ... not permitted";
  `DELETE` → 200; subsequent `PUT (int)` → 200; `DELETE` → 200. No restart
  between the delete and recreate.
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
