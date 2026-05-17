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

These are generated in the admin console: **Tenant Settings → Log API Keys**.
Save the secret immediately on creation — it cannot be retrieved later.

There is also a key-management API (`/keys`) that uses the service-account
bearer token to mint new log keys programmatically — see frodo-lib
`src/api/cloud/LogApi.ts`. Bearer-auth fails against `/monitoring/logs/*`
itself (verified live: 401).

## Endpoints (tenant-global)

| Op | Method | Path | Notes |
|----|--------|------|-------|
| List sources | `GET` | `/monitoring/logs/sources` | Returns array of available source IDs. |
| Fetch logs | `GET` | `/monitoring/logs?source={src}&beginTime=…&endTime=…` | Time-bounded query. |
| Tail logs | `GET` | `/monitoring/logs/tail?source={src}` | Most-recent ~15s window; pageable. |
| List API keys | `GET` | `/keys` | Bearer-auth. |
| Get API key | `GET` | `/keys/{id}` | Bearer-auth. |
| Create API key | `POST` | `/keys?_action=create` | Bearer-auth. Body: `{"name":"..."}`. Secret in response (only time). |

## Query params (`/monitoring/logs`)

| Param | Type | Notes |
|-------|------|-------|
| `source` | string (comma-separated) | Required. e.g. `am-access`, `idm-everything`. |
| `beginTime` | ISO 8601 (`2026-05-17T10:00:00Z`) | ≤24h before `endTime`. |
| `endTime` | ISO 8601 | Required if `beginTime` set. |
| `_queryFilter` | CREST filter | e.g. `payload/transactionId eq "abc"`. Avoid array indexing. |
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

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-05-17
- Calls: `GET /monitoring/logs/sources` (Bearer) → 401 (as expected — wrong
  auth). Log API keys not present in `.envrc`; live verification deferred.

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
