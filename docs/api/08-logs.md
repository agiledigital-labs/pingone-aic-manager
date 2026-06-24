# 08 — Logs (`/monitoring/logs`)

## Purpose
Fetch tenant audit + debug logs from AM and IDM. Stretch goal of aic-edit
("log sync with compression + search") is built on this.

## Authentication

**NOT a service-account bearer.** The log API uses a separate, console-issued
API key pair sent as headers:

```
x-api-key:    <api_key_id>
x-api-secret: <api_key_secret>
```

**A service-account bearer cannot read logs (verified 2026-06-24).** `GET
/monitoring/logs/sources` and `GET /monitoring/logs?…` both return **401** for an
SA bearer — even one carrying all 13 grantable `fr:idc:*` scopes plus
`fr:am:*`/`fr:idm:*`. 401 (not 403) means the `/monitoring/logs/*` family is a
**separate auth plane** that only accepts the api-key header pair; scope is
irrelevant. There is no bearer path to log search — the key pair is mandatory.

These are generated in the admin console: **Tenant Settings → Log API Keys**.
Save the secret immediately on creation — it cannot be retrieved later.

There is also a key-management API (`/keys`) that uses the service-account
bearer token to mint new log keys programmatically — see frodo-lib
`src/api/cloud/LogApi.ts`. Bearer-auth fails against `/monitoring/logs/*`
itself (verified live: 401).

**⚠ `/keys` is NOT service-account-accessible (verified 2026-06-24).** Both
`GET /keys` and `POST /keys?_action=create` return **403 "insufficient scope"**
for an SA bearer — and they *still* 403 after granting the SA **all 13**
`fr:idc:*` scopes it can hold (`analytics`, `telemetry`, `dataset`,
`certificate`, `promotion`, `release`, … — see the test in
`99-quirks-and-open-questions.md`). The endpoint accepts the bearer (not 401)
but no service-account scope satisfies it. Conclusion: **log-key management
requires an admin-*user* token** (the cookie / AppAuth session our
cookie/userpass onboarding already mints via `session_to_bearer()`), not a
service-account token. The frodo-lib "SA mints log keys" claim is stale.

Implications for aic-edit:
- **Mint-on-demand is only possible while we hold an admin session** — i.e. at
  cookie/userpass onboarding time, or by re-authing as admin. An existing
  tenant that only has an SA cannot mint keys.
- **Default path: paste console-created keys** (Tenant Settings → Log API
  Keys) and store them in the vault. Always works.

## Endpoints (tenant-global)

| Op | Method | Path | Notes |
|----|--------|------|-------|
| List sources | `GET` | `/monitoring/logs/sources` | Returns array of available source IDs. |
| Fetch logs | `GET` | `/monitoring/logs?source={src}&beginTime=…&endTime=…` | Time-bounded query. |
| Tail logs | `GET` | `/monitoring/logs/tail?source={src}` | Most-recent ~15s window; pageable. |
| List API keys | `GET` | `/keys` | **Admin-user bearer** (see below). CREST paged envelope; elements `{api_key_id, created_at, name}` — no secret. |
| Get API key | `GET` | `/keys/{id}` | Admin-user bearer. |
| Create API key | `POST` | `/keys?_action=create` | Admin-user bearer. Body: `{"name":"..."}`. Returns `{name, api_key_id, api_key_secret, created_at}` — **secret only here, once**. |
| Delete API key | `DELETE` | `/keys/{id}` | Admin-user bearer. → **204 No Content**. |

**Auth for `/keys` (verified 2026-06-24):** these need an **admin-user bearer**,
NOT a service-account token. Mint one via the same `idmAdminClient` PKCE flow
onboarding already uses (`session_to_bearer`, scope `openid fr:idm:*` — no extra
scope needed). With that token, `GET`/`POST create`/`DELETE` all succeed
(200/200/204). An SA bearer 403s on `/keys` no matter the scope (see auth
section). So aic-edit can auto-mint keys only while it holds an admin session
(onboarding); otherwise paste console-created keys.

## Query params (`/monitoring/logs`)

| Param | Type | Notes |
|-------|------|-------|
| `source` | string (comma-separated) | Required. e.g. `am-access`, `idm-everything`. |
| `beginTime` | ISO 8601 (`2026-05-17T10:00:00Z`) | ≤24h before `endTime`. |
| `endTime` | ISO 8601 | Required if `beginTime` set. |
| `transactionId` | string | **Direct top-level param** — `&transactionId=<id>` filters to one transaction. This is the working path (verified via the `gt`-style call), not `_queryFilter`. |
| `_queryFilter` | CREST filter | e.g. `payload/transactionId eq "abc"`. Avoid array indexing. Prefer the `transactionId` param above for the common case. |
| `_pageSize` | int | Default 1000, max 1000. |
| `_pagedResultsCookie` | opaque | From previous page. |

## Object shapes

### Log event

```json
{
  "timestamp": "2026-05-17T10:23:45.123Z",
  "source": "am-access",
  "type": "application/json",
  "payload": {
    "timestamp":     "2026-05-17T10:23:45.123Z",
    "thread":        "http-nio-...",
    "level":         "INFO",
    "logger":        "am.access",
    "message":       "…",
    "context":       "default",
    "mdc":           { "transactionId": "abc-…" },
    "transactionId": "abc-…"
  }
}
```

`payload` may also be a raw string for non-JSON sources.

### API key (creation response)

```json
{
  "name": "aic-edit-dev",
  "api_key_id": "<uuid>",
  "api_key_secret": "<once-only secret — save immediately>",
  "created_at": "2026-05-17T..."
}
```

Subsequent GETs omit `api_key_secret`.

## Rate limits

- **60 requests/min per environment.**
- **1000 log entries per response.**
- Exceeding → HTTP 429 with `Retry-After` header (seconds).
- Theoretical ceiling: 60 000 entries/min.

Built-in retry: see frodo-lib `LogApi.ts` for the exponential-backoff pattern
that honors `Retry-After`.

## Retention

- AIC retains logs for **30 days** server-side. For longer history, sync
  locally — which is exactly the stretch goal.

## Examples

```bash
# Bearer auth fails:
$SCRIPTS/verify-endpoint.sh "/monitoring/logs/sources"
# → HTTP 401

# Correct call (api key pair) — once we have a key:
curl -sS "$TENANT_BASE_URL/monitoring/logs/sources" \
  -H "x-api-key: $LOG_KEY_ID" \
  -H "x-api-secret: $LOG_KEY_SECRET"
```

## Quirks

- **Headers are lowercase** in the docs (`x-api-key`, not `X-API-Key`). HTTP is
  case-insensitive but be consistent for grep-ability.
- **`beginTime`/`endTime` window ≤ 24h.** Bigger windows return 400.
- **`/tail` first call** returns the last ~15s; subsequent calls with the
  returned `pagedResultsCookie` continue from where the last call left off.
  This is the streaming pattern.
- **Don't filter by array index** (`payload/things[0]/foo`) — server rejects.
  Filter by field equality only.
- **`transactionId` appears twice** in payload (top-level and inside `mdc`).
  They should match; use the top-level one.

## Source IDs (verified)

Live `GET /monitoring/logs/sources` on the sandbox returned:

```
am-access  am-activity  am-authentication  am-config  am-core  am-everything
ctsstore  ctsstore-access  ctsstore-config-audit  ctsstore-upgrade
idm-access  idm-activity  idm-authentication  idm-config  idm-core
idm-everything  idm-sync
userstore  userstore-access  userstore-config-audit  userstore-ldif-importer
userstore-upgrade
```

Note: **`idm-recon` is NOT in the live set** (it was in an earlier guessed
list). `am-everything` and `idm-everything` are the catch-all roll-ups and the
right CLI defaults for transaction lookups.

## Verified against

- Tenant: `tenant.example.com` (the aic-edit sandbox)
- Date: 2026-06-24
- Calls:
  - `GET /keys` (Bearer, our SA scopes) → **403 insufficient scope** (endpoint
    exists, scope-gated — see scope-gap note above).
  - `GET /monitoring/logs/sources` (api-key pair) → **401** because the only key
    we had on hand had been rotated/revoked. Auth model confirmed (api-key path,
    not bearer); a live 200 against this tenant is still pending a valid key.
  - Source list above + `transactionId`/`beginTime`/`endTime`/
    `_pagedResultsCookie` query shapes confirmed against working reference
    scripts (`<client-checkout>/logs/`).
  - `/keys` full lifecycle verified live via an **admin-user bearer**
    (`idmAdminClient` PKCE, scope `openid fr:idm:*`): `GET /keys` → 200 (CREST
    envelope, elements `{api_key_id, created_at, name}`); `POST
    /keys?_action=create {name}` → 200 returning `{name, api_key_id,
    api_key_secret, created_at}`; `DELETE /keys/{id}` → 204. SA bearer 403s
    regardless of scope.
- **Still deferred:** a 200 from `/monitoring/logs` on this tenant (needs a live
  key — minting one is now unblocked via the admin flow above).

## Source citations

- frodo-lib: `src/api/cloud/LogApi.ts`.
- Ping docs: <https://docs.pingidentity.com/pingoneaic/latest/tenants/audit-debug-logs-pull.html>

## Open questions

- **Full list of source IDs.** Examples seen in docs: `am-access`, `am-activity`,
  `am-authentication`, `am-config`, `idm-access`, `idm-activity`, `idm-config`,
  `idm-sync`, `idm-recon`, `idm-everything`. Verify the actual set by calling
  `/monitoring/logs/sources` once we have an API key.
- **Compression strategy.** For the local-sync feature: per-source append-only
  parquet/zstd files keyed by hour, with a column store for fast `payload/...`
  filtering. Not yet designed — see future Step 3+ plan.
- **Does the tail endpoint support server-sent events / chunked transfer**, or
  is it just long-poll? frodo-lib treats it as long-poll. Confirm.
