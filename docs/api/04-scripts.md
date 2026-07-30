# 04 — Scripts

Implemented in: `src/scripts/`

## Purpose

Scripts are JavaScript (and rarely Groovy) snippets that run inside AM during
authentication, token issuance, OIDC claims, SAML mapping, policy decisions,
etc. Feature 2 of pingone-aic-manager ("sync scripts to a local directory +
watch + upload with content-based conflict detection") is built on this API.

## Authentication

Service-account bearer. Scope: `fr:am:*`.

## Endpoints (realm-scoped)

Replace `{realm-path}` with `/realms/root/realms/alpha` (or `bravo`). Always
send `Accept-API-Version: protocol=2.0,resource=1.0`.

| Op     | Method   | Path                                                    | Notes                                                                                                                                                                                                                        |
| ------ | -------- | ------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| List   | `GET`    | `/am/json{realm-path}/scripts?_queryFilter=true`        | Returns **all** results when `_pageSize` is omitted. If you set `_pageSize`, page by **`_pagedResultsOffset`** + `remainingPagedResults` — the `pagedResultsCookie` comes back `null` and is unusable (verified 2026-06-01). |
| Filter | `GET`    | `/am/json{realm-path}/scripts?_queryFilter=name+eq+"…"` | CREST filter.                                                                                                                                                                                                                |
| Read   | `GET`    | `/am/json{realm-path}/scripts/{id}`                     | `id` is a UUID.                                                                                                                                                                                                              |
| Upsert | `PUT`    | `/am/json{realm-path}/scripts/{id}`                     | Full body. `script` MUST be base64. **201** when `{id}` is new, **200** on replace. See "Creating scripts" below.                                                                                                            |
| Create | `POST`   | `/am/json{realm-path}/scripts/?_action=create`          | **201**; server assigns the UUID. Same body as `PUT`, minus `_id`. Note the trailing slash before `?`.                                                                                                                       |
| Delete | `DELETE` | `/am/json{realm-path}/scripts/{id}`                     | **200** + echoes nothing useful; permanent. **404** if already gone. Default scripts **403** (see Quirks).                                                                                                                   |

## Creating scripts

Verified live 2026-07-30 (sandbox, realm `alpha`, throwaway `test_aic*` scripts,
all deleted afterwards). Two routes, both returning **201**:

- **`PUT …/scripts/{id}`** with a **client-chosen** `{id}` — this is the route
  `aic` uses, so the local workspace can know the id before the write.
- **`POST …/scripts/?_action=create`** with no `_id` — the server picks the
  UUID.

**Required fields.** Omitting any one of these is a `400` with a precise
message:

| Field      | Missing-field error                    |
| ---------- | -------------------------------------- |
| `name`     | `Script name must be specified`        |
| `context`  | `Script type must be specified`        |
| `language` | `Scripting language must be specified` |
| `script`   | `A script must be specified`           |

Everything else defaults: `description` → `null`, `default` → `false`,
`evaluatorVersion` → `"2.0"`. An **empty** `script` (`""`) is accepted (201) —
only a _missing_ one 400s.

**Body `_id` must match the URL id.** Sending a body whose `_id` is a different
script's id — the obvious way to copy a script — fails with
`400 "Script resource id and script JSON body id do not match"`. Either strip
`_id` from the body (the URL id is then used) or rewrite it to the new id.

**Server-owned fields on a copy are ignored, not honoured.** A verbatim fetched
body still carrying `_rev`, `createdBy`, `creationDate`, `lastModifiedBy`, and
`lastModifiedDate` creates fine: the server stamps its own values. So copying a
script is "fetch, rewrite `_id` + `name`, PUT" — no field stripping needed
beyond `_id`.

**`name` is unique per realm, enforced server-side.** A second script with a
name already in the realm →
`409 "Script with name <name> already exist in realm /alpha"`. The same name in
the _other_ realm is fine (201) — which is what makes an alpha→bravo copy a
plain create.

**`context` is normalised on write.** `SCRIPTED_DECISION_NODE` is stored and
returned as `AUTHENTICATION_TREE_DECISION_NODE`. Anything unrecognised →
`400 "Script type not recognised: <value>"`. Because the stored value can differ
from what you sent, re-read (or use the 201 echo) before deriving anything from
`context` — `aic` re-pulls after a create so the workspace path and snapshot
come from the server's canonical form.

**`_id` need not be a UUID.** `PUT …/scripts/test_aic_named_id` created a script
whose `_id` is that literal string (201). `aic` still mints UUIDs, to match what
the console and frodo produce.

## Script context enumeration

Endpoint:

```
GET /am/json/global-config/services/scripting/contexts?_queryFilter=true
Accept-API-Version: protocol=2.0,resource=1.0
```

Returns the full list of supported contexts. Verified live (40 distinct contexts
in the sandbox as of 2026-07-30):

```
AUTHENTICATION_CLIENT_SIDE                          OAUTH2_VALIDATE_SCOPE
AUTHENTICATION_SERVER_SIDE                          OAUTH2_VALIDATE_SCOPE_NEXT_GEN
AUTHENTICATION_TREE_DECISION_NODE                   OIDC_CLAIMS
CACHE_LOADER                                        OIDC_CLAIMS_NEXT_GEN
CONFIG_PROVIDER_NODE                                OIDC_NODE
CONFIG_PROVIDER_NODE_NEXT_GEN                       PINGONE_VERIFY_COMPLETION_DECISION_NODE
DEVICE_MATCH_NODE                                   POLICY_CONDITION
LIBRARY                                             POLICY_CONDITION_NEXT_GEN
NODE_DESIGNER                                       SAML2_IDP_ADAPTER
OAUTH2_ACCESS_TOKEN_MODIFICATION                    SAML2_IDP_ADAPTER_NEXTGEN
OAUTH2_ACCESS_TOKEN_MODIFICATION_NEXT_GEN           SAML2_IDP_ATTRIBUTE_MAPPER
OAUTH2_AUTHORIZE_ENDPOINT_DATA_PROVIDER             SAML2_IDP_ATTRIBUTE_MAPPER_NEXT_GEN
OAUTH2_AUTHORIZE_ENDPOINT_DATA_PROVIDER_NEXT_GEN    SAML2_NAMEID_MAPPER
OAUTH2_DYNAMIC_CLIENT_REGISTRATION                  SAML2_SP_ACCOUNT_MAPPER
OAUTH2_EVALUATE_SCOPE                               SAML2_SP_ADAPTER
OAUTH2_EVALUATE_SCOPE_NEXT_GEN                      SAML2_SP_ADAPTER_NEXTGEN
OAUTH2_MAY_ACT                                      SCRIPTED_DECISION_NODE
OAUTH2_MAY_ACT_NEXT_GEN                             SOCIAL_IDP_PROFILE_TRANSFORMATION
OAUTH2_SCRIPTED_JWT_ISSUER                          SOCIAL_IDP_PROFILE_TRANSFORMATION_NEXT_GEN
OAUTH2_SCRIPTED_JWT_ISSUER_NEXT_GEN                 SOCIAL_PROVIDER_HANDLER_NODE
```

(Note the inconsistent spelling `NEXTGEN` vs `NEXT_GEN` — see Quirks below.)

Each `result` element is an object with `_id`, `_rev`, `isHidden`, `languages`,
`defaultScript`, and `_type`; the context name is `_id`. `NODE_DESIGNER` is the
one hidden entry (`isHidden: true`). All 40 advertise `JAVASCRIPT`; 15 also
advertise `GROOVY` in `languages`.

## Object shape (real example from sandbox)

```json
{
  "_id": "ac40a394-b3cd-400f-b2aa-b6b2e4a8be8e",
  "name": "Cache Loader Script",
  "description": "Default global script for Cache Loader",
  "script": "LyoKICogQ29weXJpZ2h0...", // base64 — see Quirks
  "default": true,
  "language": "JAVASCRIPT",
  "context": "CACHE_LOADER",
  "createdBy": "id=dsameuser,ou=user,dc=openam,dc=forgerock,dc=org",
  "creationDate": 1433147666269,
  "lastModifiedBy": "id=dsameuser,ou=user,dc=openam,dc=forgerock,dc=org",
  "lastModifiedDate": 1433147666269,
  "evaluatorVersion": "2.0"
}
```

- `default: true` ⇒ ForgeRock-shipped default. **Editable** (a content PUT
  succeeds — verified 2026-06-03); cannot be _deleted_. (`aic` pushes defaults
  like any other script — no `--force` needed.)
- `evaluatorVersion`: `"1.0"` or `"2.0"`. Affects available bindings; v2 is the
  current default for new scripts.
- **No `_rev` field** on a `GET` or on an update `PUT` echo — so optimistic
  locking via `If-Match` is not available and **conflict detection must be
  content-based**. One exception, verified 2026-07-30: the **create** echo (201,
  either route) _does_ carry a `_rev`. It is write-only noise — a subsequent
  `GET` of the same script has no `_rev` at all — so never persist it or compare
  against it.
- **`GROOVY` scripts** (`language: "GROOVY"`) — AIC has dropped Groovy support;
  old tenants still carry many. `aic` does not sync them (filtered in the list).
- **Product-internal scripts** are named `"ForgeRock Internal: …"`. A
  `GET …/scripts/{id}` on one returns **403**
  `"This operation is not available in PingOne Advanced Identity Cloud."` —
  they're read-protected, so un-pullable. **No field in the list record marks
  them as internal** (verified 2026-06-03 — `default`,
  `createdBy`/`lastModifiedBy` null, `creationDate`, `context` all overlap
  normal scripts); the only reliable signal is the name prefix. `aic` hides them
  from the list.

## Conflict detection rule (for two-way sync)

Per user requirement: compare script content, **not** revision numbers. If a
local edit happens against an older "remote snapshot" but the remote content is
back to that snapshot (someone reverted), the local push should succeed.

Algorithm:

1. Cache the last-synced remote `script` content (base64) per script ID locally.
2. Before pushing a local change, `GET` the remote script and base64-decode.
3. If `remote.script_decoded == cached_last_synced_decoded`, push freely.
4. Otherwise (remote drifted), block and prompt the user to resolve: show 3-way
   diff of `cached_last_synced` vs `remote` vs `local`.
5. On every successful push, update the cached snapshot.
6. On successful pull (initial sync or refresh), update the cached snapshot.

Always compare **decoded** content. Re-encoding can produce different base64
strings (line breaks, padding) for the same bytes.

## Examples

```bash
# List first script in alpha
$SCRIPTS/verify-endpoint.sh \
  "/am/json/realms/root/realms/alpha/scripts?_queryFilter=true&_pageSize=1" \
  --header "Accept-API-Version: protocol=2.0,resource=1.0"

# Read a specific script
$SCRIPTS/verify-endpoint.sh \
  "/am/json/realms/root/realms/alpha/scripts/ac40a394-b3cd-400f-b2aa-b6b2e4a8be8e" \
  --header "Accept-API-Version: protocol=2.0,resource=1.0"

# Update (PUT — illustrative; do not run on a real script)
curl -X PUT "$TENANT_BASE_URL/am/json/realms/root/realms/alpha/scripts/$ID" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Accept-API-Version: protocol=2.0,resource=1.0" \
  -H "Content-Type: application/json" \
  -d '{
        "name":"My Script","description":"…",
        "script":"'"$(echo -n 'function foo(){return 1;}' | base64 -w0)"'",
        "language":"JAVASCRIPT","context":"SCRIPTED_DECISION_NODE",
        "default":false,"evaluatorVersion":"2.0"
      }'
```

## Quirks

- **`script` is base64-encoded on the wire** (both directions). Decode on read,
  encode on write. This contradicts the frodo-lib research summary but matches
  the Ping docs, fr-config-manager push code, and the live response shown above.
- **No `_rev`** — see "Conflict detection" above.
- **Context naming inconsistency.** Some SAML contexts use `NEXTGEN` (no
  underscore), most others use `NEXT_GEN` (with underscore). Keep an exact
  string list rather than try to derive it. The verified list is above.
- **Default scripts** (those with `default: true`) — `PUT` **succeeds** (content
  edits stick; verified 2026-06-03), but `DELETE` returns
  **`403 "Default script <name> cannot be deleted"`** and the script is still
  readable afterwards (verified 2026-07-30). It is a clean refusal, not a silent
  no-op, so `aic script delete` can rely on the server — it refuses locally
  first only to save the round trip.
- **LIBRARY context** scripts have an additional `exports` array describing
  functions they expose for other scripts to require.
- **A referenced LIBRARY script cannot be deleted** (verified 2026-07-29).
  `DELETE …/scripts/{lib-id}` while any script `require()`s it by name returns
  **`500`** with `"message": "The script <name> is used once"`. Delete the
  consumers first, then the library — the same `DELETE` then returns `200`.
  (Yes, a referential-integrity refusal reported as a 500, not a 409.)
- **`creationDate` / `lastModifiedDate`** are epoch milliseconds, not ISO 8601
  (unlike ESVs which use ISO 8601). Be careful when serializing.
- **Realm-scoped storage.** A script ID can exist in alpha but not bravo, or
  with totally different content in each. Always include realm in any local
  cache key.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-05-17
- Calls: `GET …/scripts?_queryFilter=true&_pageSize=1` (200 OK, base64 body
  confirmed by decoding first 30 chars to JS comment header),
  `GET …/scripts/{id}` (200 OK, no `_rev`),
  `GET /am/json/global-config/services/scripting/contexts?_queryFilter=true`
  (200 OK, full context list captured above).

### Create / copy / delete — 2026-07-30

Realm `alpha` (and one cross-realm write to `bravo`), all throwaway `test_aic*`
scripts deleted afterwards and the realms re-listed to confirm nothing was left
behind:

- `PUT …/scripts/{fresh-uuid}` → **201** + full object echo (with `_rev`);
  second `PUT` to the same id → **200** + echo (no `_rev`).
- `POST …/scripts/?_action=create` (no `_id`) → **201**, server-assigned UUID.
- Duplicate `name` in the same realm → **409**; same `name` in `bravo` →
  **201**.
- Each of `name` / `context` / `language` / `script` omitted in turn → **400**
  with the field-specific message tabulated above; `script: ""` → **201**.
- `context: "NOT_A_CONTEXT"` → **400 "Script type not recognised"**;
  `SCRIPTED_DECISION_NODE` stored as `AUTHENTICATION_TREE_DECISION_NODE`.
- Verbatim fetched body under a new URL id → **400** id-mismatch; same body with
  `_id` stripped → **201**; with `_id` rewritten to the URL id → **201**, and
  the re-read shows the server's own `createdBy`/`creationDate`, not the
  source's.
- `PUT …/scripts/test_aic_named_id` (non-UUID id) → **201**.
- `DELETE` of a `default: true` script (`SAML2 IDP Adapter Script`) → **403**,
  script still `GET`-able; `DELETE` of a nonexistent id → **404**.
- IDM side re-confirmed: `PUT /openidm/config/endpoint/{name}` 201 → 200 on
  replace → `DELETE` 200 → `GET` 404, and `DELETE` of an absent config → 404
  with `"No existing configuration found for …, can not delete"`. A
  `schedule/{name}` created with
  `enabled:false, persisted:true, type:"cron", schedule:"0 0 0 1 1 ? 2099", invokeService:"script"`
  → **201** and reads back verbatim (the shape
  `aic script create schedule/<name>` writes).

## Source citations

- frodo-lib: `src/api/ScriptApi.ts`, `src/api/ScriptTypeApi.ts`.
- fr-config-manager: `packages/fr-config-pull/src/scripts/scripts.js`,
  `packages/fr-config-push/src/scripts/update-scripts.js` (note: explicitly
  base64-encodes before PUT).
- Ping docs:
  <https://docs.pingidentity.com/pingoneaic/latest/am-scripting/rest-api-scripts-read.html>

## Open questions

- Does the server reject non-base64 in the `script` field, or attempt to detect
  raw JS? frodo-lib seemed to assume raw, which would suggest a tolerant server.
- Are `LIBRARY` scripts' `exports` validated against the script body, or just
  declarative metadata?
