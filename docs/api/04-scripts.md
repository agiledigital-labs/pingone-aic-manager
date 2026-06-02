# 04 — Scripts

## Purpose
Scripts are JavaScript (and rarely Groovy) snippets that run inside AM during
authentication, token issuance, OIDC claims, SAML mapping, policy decisions,
etc. Feature 2 of aic-edit ("sync scripts to a local directory + watch +
upload with content-based conflict detection") is built on this API.

## Authentication
Service-account bearer. Scope: `fr:am:*`.

## Endpoints (realm-scoped)

Replace `{realm-path}` with `/realms/root/realms/alpha` (or `bravo`).
Always send `Accept-API-Version: protocol=2.0,resource=1.0`.

| Op | Method | Path | Notes |
|----|--------|------|-------|
| List | `GET` | `/am/json{realm-path}/scripts?_queryFilter=true` | Returns **all** results when `_pageSize` is omitted. If you set `_pageSize`, page by **`_pagedResultsOffset`** + `remainingPagedResults` — the `pagedResultsCookie` comes back `null` and is unusable (verified 2026-06-01). |
| Filter | `GET` | `/am/json{realm-path}/scripts?_queryFilter=name+eq+"…"` | CREST filter. |
| Read | `GET` | `/am/json{realm-path}/scripts/{id}` | `id` is a UUID. |
| Upsert | `PUT` | `/am/json{realm-path}/scripts/{id}` | Full body. `script` MUST be base64. |
| Delete | `DELETE` | `/am/json{realm-path}/scripts/{id}` | Permanent (default scripts cannot be deleted). |

## Script context enumeration

Endpoint:

```
GET /am/json/global-config/services/scripting/contexts?_queryFilter=true
Accept-API-Version: protocol=2.0,resource=1.0
```

Returns the full list of supported contexts. Verified live (41 distinct
contexts in the sandbox as of 2026-05-17):

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

Each context has its own permitted `languages` (`JAVASCRIPT`, occasionally
`GROOVY`) and a default-script ID.

## Object shape (real example from sandbox)

```json
{
  "_id": "ac40a394-b3cd-400f-b2aa-b6b2e4a8be8e",
  "name": "Cache Loader Script",
  "description": "Default global script for Cache Loader",
  "script": "LyoKICogQ29weXJpZ2h0...",   // base64 — see Quirks
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
  succeeds — verified 2026-06-03); cannot be *deleted*. (`aic` pushes defaults
  like any other script — no `--force` needed.)
- `evaluatorVersion`: `"1.0"` or `"2.0"`. Affects available bindings; v2 is
  the current default for new scripts.
- **No `_rev` field.** Optimistic locking via `If-Match` is not available for
  scripts. **Conflict detection must be content-based.**
- **`GROOVY` scripts** (`language: "GROOVY"`) — AIC has dropped Groovy support;
  old tenants still carry many. `aic` does not sync them (filtered in the list).
- **Product-internal scripts** are named `"ForgeRock Internal: …"`. A
  `GET …/scripts/{id}` on one returns **403** `"This operation is not available
  in PingOne Advanced Identity Cloud."` — they're read-protected, so un-pullable.
  **No field in the list record marks them as internal** (verified 2026-06-03 —
  `default`, `createdBy`/`lastModifiedBy` null, `creationDate`, `context` all
  overlap normal scripts); the only reliable signal is the name prefix. `aic`
  hides them from the list.

## Conflict detection rule (for two-way sync)

Per user requirement: compare script content, **not** revision numbers. If a
local edit happens against an older "remote snapshot" but the remote content
is back to that snapshot (someone reverted), the local push should succeed.

Algorithm:
1. Cache the last-synced remote `script` content (base64) per script ID locally.
2. Before pushing a local change, `GET` the remote script and base64-decode.
3. If `remote.script_decoded == cached_last_synced_decoded`, push freely.
4. Otherwise (remote drifted), block and prompt the user to resolve:
   show 3-way diff of `cached_last_synced` vs `remote` vs `local`.
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
- **Default scripts** (those with `default: true`) — PUT/DELETE on these may
  silently no-op or return 403. Don't write to them.
- **LIBRARY context** scripts have an additional `exports` array describing
  functions they expose for other scripts to require.
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

## Source citations

- frodo-lib: `src/api/ScriptApi.ts`, `src/api/ScriptTypeApi.ts`.
- fr-config-manager: `packages/fr-config-pull/src/scripts/scripts.js`,
  `packages/fr-config-push/src/scripts/update-scripts.js`
  (note: explicitly base64-encodes before PUT).
- Ping docs: <https://docs.pingidentity.com/pingoneaic/latest/am-scripting/rest-api-scripts-read.html>

## Open questions

- What does PUT actually return — full object echo or thin `{_id,_rev?}`?
  Test on a throwaway script.
- Does the server reject non-base64 in the `script` field, or attempt to detect
  raw JS? frodo-lib seemed to assume raw, which would suggest a tolerant server.
- Are `LIBRARY` scripts' `exports` validated against the script body, or just
  declarative metadata?
