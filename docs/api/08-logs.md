# 08 — Logs (`/monitoring/logs`)

Implemented in: `src/logs/`

Implemented in: `src/logs/`

## Purpose

Fetch tenant audit + debug logs from AM and IDM. Stretch goal of
pingone-aic-manager ("log sync with compression + search") is built on this.

## Authentication

**NOT a service-account bearer.** The log API uses a separate, console-issued
API key pair sent as headers:

```
x-api-key:    <api_key_id>
x-api-secret: <api_key_secret>
```

**A service-account bearer cannot read logs (verified 2026-06-24).**
`GET /monitoring/logs/sources` and `GET /monitoring/logs?…` both return **401**
for an SA bearer — even one carrying all 13 grantable `fr:idc:*` scopes plus
`fr:am:*`/`fr:idm:*`. 401 (not 403) means the `/monitoring/logs/*` family is a
**separate auth plane** that only accepts the api-key header pair; scope is
irrelevant. There is no bearer path to log search — the key pair is mandatory.

These are generated in the admin console: **Tenant Settings → Log API Keys**.
Save the secret immediately on creation — it cannot be retrieved later.

There is also a key-management API (`/keys`) that uses the service-account
bearer token to mint new log keys programmatically — see frodo-lib
`src/api/cloud/LogApi.ts`. Bearer-auth fails against `/monitoring/logs/*` itself
(verified live: 401).

**⚠ `/keys` is NOT service-account-accessible (verified 2026-06-24).** Both
`GET /keys` and `POST /keys?_action=create` return **403 "insufficient scope"**
for an SA bearer — and they _still_ 403 after granting the SA **all 13**
`fr:idc:*` scopes it can hold (`analytics`, `telemetry`, `dataset`,
`certificate`, `promotion`, `release`, … — see the test in
`99-quirks-and-open-questions.md`). The endpoint accepts the bearer (not 401)
but no service-account scope satisfies it. Conclusion: **log-key management
requires an admin-_user_ token** (the cookie / AppAuth session our
cookie/userpass onboarding already mints via `session_to_bearer()`), not a
service-account token. The frodo-lib "SA mints log keys" claim is stale.

Implications for pingone-aic-manager:

- **Mint-on-demand is only possible while we hold an admin session** — i.e. at
  cookie/userpass onboarding time, or by re-authing as admin. An existing tenant
  that only has an SA cannot mint keys.
- **Default path: paste console-created keys** (Tenant Settings → Log API Keys)
  and store them in the vault. Always works.

## Endpoints (tenant-global)

| Op             | Method   | Path                                                  | Notes                                                                                                                             |
| -------------- | -------- | ----------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| List sources   | `GET`    | `/monitoring/logs/sources`                            | Returns array of available source IDs.                                                                                            |
| Fetch logs     | `GET`    | `/monitoring/logs?source={src}&beginTime=…&endTime=…` | Time-bounded query.                                                                                                               |
| Tail logs      | `GET`    | `/monitoring/logs/tail?source={src}`                  | Most-recent ~15s window; pageable.                                                                                                |
| List API keys  | `GET`    | `/keys`                                               | **Admin-user bearer** (see below). CREST paged envelope; elements `{api_key_id, created_at, name}` — no secret.                   |
| Get API key    | `GET`    | `/keys/{id}`                                          | Admin-user bearer.                                                                                                                |
| Create API key | `POST`   | `/keys?_action=create`                                | Admin-user bearer. Body: `{"name":"..."}`. Returns `{name, api_key_id, api_key_secret, created_at}` — **secret only here, once**. |
| Delete API key | `DELETE` | `/keys/{id}`                                          | Admin-user bearer. → **204 No Content**.                                                                                          |

**Auth for `/keys` (verified 2026-06-24):** these need an **admin-user bearer**,
NOT a service-account token. Mint one via the same `idmAdminClient` PKCE flow
onboarding already uses (`session_to_bearer`, scope `openid fr:idm:*` — no extra
scope needed). With that token, `GET`/`POST create`/`DELETE` all succeed
(200/200/204). An SA bearer 403s on `/keys` no matter the scope (see auth
section). So pingone-aic-manager can auto-mint keys only while it holds an admin
session (onboarding); otherwise paste console-created keys.

## Query params (`/monitoring/logs`)

| Param                 | Type                              | Notes                                                                                                                                                                                           |
| --------------------- | --------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `source`              | string (comma-separated)          | Required. e.g. `am-access`, `idm-everything`.                                                                                                                                                   |
| `beginTime`           | ISO 8601 (`2026-05-17T10:00:00Z`) | ≤24h before `endTime`; exclusive at an exact event timestamp.                                                                                                                                   |
| `endTime`             | ISO 8601                          | Required if `beginTime` set; inclusive at an exact event timestamp.                                                                                                                             |
| `transactionId`       | string                            | **Direct top-level param** — `&transactionId=<id>` filters to one transaction. This is the working path (verified via the `gt`-style call), not `_queryFilter`.                                 |
| `_queryFilter`        | CREST filter                      | Barely usable — `payload/transactionId eq "abc"` works; most other `payload/*` fields return **500**. No array indexing. Prefer the `transactionId` param, then filter client-side. See Quirks. |
| `_pageSize`           | int                               | Default 1000, max 1000.                                                                                                                                                                         |
| `_pagedResultsCookie` | opaque                            | From previous page.                                                                                                                                                                             |

## Object shapes

### Log event

```json
{
  "timestamp": "2026-05-17T10:23:45.123Z",
  "source": "am-access",
  "type": "application/json",
  "payload": {
    "timestamp": "2026-05-17T10:23:45.123Z",
    "thread": "http-nio-...",
    "level": "INFO",
    "logger": "am.access",
    "message": "…",
    "context": "default",
    "mdc": { "transactionId": "abc-…" },
    "transactionId": "abc-…"
  }
}
```

`payload` may also be a raw string for non-JSON sources.

### Who-did-it fields differ between AM and IDM (verified 2026-08-10)

Both `am-access` and `idm-access` answer "who changed this", but not with the
same shape — do not write one parser for both.

|                     | `am-access`                                    | `idm-access`                                     |
| ------------------- | ---------------------------------------------- | ------------------------------------------------ |
| `payload.eventName` | `AM-ACCESS-ATTEMPT` / `AM-ACCESS-OUTCOME`      | the literal `"access"`                           |
| `payload.component` | present (e.g. `"Script"`)                      | **absent**                                       |
| `payload.userId`    | **full DN** — `id=<uuid>,ou=user,ou=am-config` | **bare uuid**                                    |
| `payload.roles`     | absent                                         | present (`["internal/role/openidm-svcacct", …]`) |
| `payload.realm`     | present (`"/alpha"`)                           | absent                                           |

The AM `userId` is character-for-character the resource's own `lastModifiedBy`
(verified for AM scripts — see `04-scripts.md`), so one resolver serves the
audit log and the object metadata. Resolve either form via
`GET /am/json/realms/root/users/{id}`, extracting the `id=` RDN for AM and using
the uuid verbatim for IDM.

**Every audited write appears more than once. Two separate de-duplications are
needed:**

1. **Attempt + outcome.** AM emits `AM-ACCESS-ATTEMPT` (no `response.status`)
   then `AM-ACCESS-OUTCOME` (`SUCCESSFUL`/`FAILED`). Always filter
   `/payload/eventName eq "AM-ACCESS-OUTCOME"`.
2. **A phantom failed `CREATE` in front of every `UPDATE`.** A `PUT` to an
   existing resource is logged as `CREATE`/`FAILED`/`statusCode 412` **and**
   `UPDATE`/`SUCCESSFUL`, in the same millisecond, under **one `transactionId`**
   — the server tries create, gets "already exist", then updates. Observed on
   both `am-access` (AM scripts) and `idm-access` (`/openidm/config/*`). Filter
   `/payload/response/status eq "SUCCESSFUL"` or a history view invents failures
   that never happened.

`/openidm/config/*` writes land in **`idm-access`**, not `idm-config`: a 24 h
`/payload/objectId sw "config"` query on `idm-config` returned zero events while
`/payload/http/request/path co "openidm/config"` on `idm-access` returned 66.

The `co` (contains) predicate works server-side on `/payload/http/request/path`,
so filtering to one resource needs no client-side pass. Match on the **resource
id**, not a path prefix: the record stores the absolute URL exactly as the
client sent it, and clients differ in which realm path form they use
(`docs/api/01-realms-and-paths.md`).

### API key (creation response)

```json
{
  "name": "pingone-aic-manager-dev",
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

- AIC retains logs for **30 days** server-side. For longer history, sync locally
  — which is exactly the stretch goal.

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
- **`beginTime`/`endTime` window ≤ 24h.** Bigger windows return 400, naming the
  span you asked for:
  `13.89 days worth of data requested (…) Please limit the scope of your query to within a day: Cannot request more than one days worth of logs`
  (verified 2026-08-10). **The 24h cap is per query, not the retention limit** —
  events stay queryable for roughly 30 days, so reaching an older one means
  _moving_ a ≤24h window back, never widening it. Anything offering a "history"
  over these logs has to paginate in day-sized steps.
- **Time ranges are `(beginTime, endTime]`.** An event whose timestamp exactly
  matched the shared boundary of two adjacent live queries appeared in the
  first query (where it was `endTime`) and not the second (where it was
  `beginTime`). Contiguous pollers can therefore carry the prior end forward as
  the next begin without dropping or duplicating the boundary event (verified
  2026-09-10).
- **`/tail` first call** returns the last ~15s; subsequent calls with the
  returned `pagedResultsCookie` continue from where the last call left off. This
  is the streaming pattern.
- **Don't filter by array index** (`payload/things[0]/foo`) — server rejects.
  Filter by field equality only.
- **`_queryFilter` on most `payload/*` fields returns
  `500 Internal server error`**, not a 4xx. Verified 2026-08-12 on `am-core`:
  both `payload/message co "…"` and `payload/logger eq "…"` 500, and the same
  filter 500s on a 20-second window as on a 6-hour one — so it is the filter
  that is rejected, not the range. Only `payload/transactionId` is known to
  work, and the `transactionId` top-level param is the better route to it
  anyway. **Plan for client-side filtering**: fetch the window, then match in
  code. `am-core` is verbose enough (~1,000 events in 20s on UAT, with
  `idm-core` adding ~8,400) that this matters for any feature that greps logs by
  logger or message.
- **`aic logs tx` is an exact match on the full transaction id**, including the
  `-request-N/M` suffix — it is not a prefix match. Sub-requests AM makes while
  serving a request (notably the SAML `/am/AuthConsumer` POST) carry a
  _different_ root id and will not be returned. To follow one user action across
  transactions, `aic logs range` a few seconds either side and group by
  `payload.transactionId`. See `06-saml.md` → "Diagnosing a rejected assertion"
  for a worked example.
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
right CLI defaults for explicit transaction/range/query lookups. Sync uses the
curated signal-first list below.

## Source taxonomy — signal vs noise (verified 2026-06-30)

Default local sync should keep the structured, low-to-moderate volume sources
that support audit search and journey analysis, and skip the high-volume core
debug streams unless the user explicitly asks for them.

| Source              | Sync default       | Signal                                                                                |
| ------------------- | ------------------ | ------------------------------------------------------------------------------------- |
| `am-authentication` | Keep               | Journey progress: node and tree login events.                                         |
| `am-access`         | Keep               | AM access/audit outcomes, including who-changed evidence.                             |
| `am-activity`       | Keep               | AM identity changes and session activity.                                             |
| `idm-activity`      | Keep               | Managed-object changes with before/after, `changedFields`, `userId`.                  |
| `idm-config`        | Keep               | IDM config changes.                                                                   |
| `idm-access`        | Keep               | IDM API access events.                                                                |
| `am-core`           | Discard by default | CTS reaper and internal debug/WARN stream; operational noise except WARN/ERROR lines. |
| `idm-core`          | Discard by default | Raw-string FINE debug stream; operational noise except WARN/ERROR lines.              |

`idm-core` is the dominant payload behind `idm-everything` in the sandbox sample
(about 99%). Its events are raw JSON strings rather than structured payload
objects, mostly FINE-level traces from Felix OSGi health checks
(`org.apache.felix.hc.*`), recon-queue polling
(`openidm.sync.impl.queue.QueueConsumerFactory`), ClusterManager, RepoJobStore,
and Quartz internals. These records have no payload `_id`, no `eventName`, and
no user identity, so they add storage volume without audit or product signal.

`am-authentication` carries the journey-progress signal needed by the future
journey-progress view:

| Event                     | Fields                                                                                                                               |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `AM-NODE-LOGIN-COMPLETED` | `payload.entries[].info.{treeName,nodeId,displayName,nodeType,nodeOutcome,authLevel}`; no principal                                  |
| `AM-TREE-LOGIN-COMPLETED` | `payload.entries[].info.treeName`, `payload.result` (`SUCCESSFUL`/`FAILED`), `payload.principal[]` (username), `payload.userId` (DN) |

**Join key (verified 2026-07-01, re-corrected).** Group on the **full
`payload.trackingIds[0]`** value — used verbatim, with **no transformation**.
Both the node events and the tree event of one execution carry the **same** full
`trackingIds[0]` (e.g. `a3c45e03-1244-4a1e-98c8-3cde967c4de1-19612069`). That
string is the per-execution key.

Two earlier claims here were WRONG — do not repeat them:

- Do **not** join on `transactionId`: it is a per-HTTP-request id (`Root=1-…/0`,
  `…-request-2/0`) that differs within one execution.
- Do **not** strip the trailing `-<digits>` suffix, and do **not** key the tree
  event off `payload._id`. The `<uuid>` prefix before the numeric suffix is an
  **AM server/cluster instance id**, not an execution id — verified against
  AIC's own `Journey-Node-History` export, where a single base `a3c45e03-…`
  spans **3,226 distinct executions across 2,502 users and multiple journeys**.
  Stripping it merges thousands of executions into one (a rolled-up "attempt"
  with 1,000+ nodes is the symptom). The tree event's `_id` has a _different_
  base (its own logging instance) and matches node `trackingIds[0]` only by
  coincidence of prefix — it is **not** the join.

Verified against `<client-checkout>/logs/prod-logs.json` (4,152 am-authentication events):
grouping on full `trackingIds[0]` yields **322 executions** (median 19 nodes
each, max 49), of which 138 have a matching tree event (tree `trackingIds[0]` ∈
node `trackingIds[0]`; tree `_id` matches → 0) and 184 are node-only. Distinct
users 114, journeys 8 (SSP-Login, DealerLogin, SSP-ResetPassword, …), results
127 SUCCESSFUL / 11 FAILED.

Order node events by `payload.timestamp`; the last node's outcome is the
furthest point reached. Journey name = the tree event's `treeName`
(authoritative; inner `InnerTreeEvaluatorNode` steps report the sub-tree's name,
so ~16% of executions have mixed node `treeName`s — prefer the tree event's).
Attempt result: tree `result=SUCCESSFUL` → COMPLETED, any other tree `result` →
FAILED, no tree event for a node group → ABANDONED. Skip
`AM-LOGIN-MODULE-COMPLETED`/`AM-LOGIN-COMPLETED` with
`authIndex=module_instance` — those are OAuth2 client/service-account module
logins (no `treeName`/`nodeId`), not user journeys.

The curated `aic logs sync` default source list is:

```
am-authentication,am-access,am-activity,idm-activity,idm-config,idm-access
```

The explicit `tx`, `range`, and `query` commands still default to
`am-everything,idm-everything` because they are user-driven lookups. Sync does
not default to the `-everything` rollups because that would mostly persist
`idm-core` and `am-core` debug noise. The sync path also applies an
`is_core_noise` post-fetch filter to every fetched page: if top-level `source`
ends with `-core` and `payload` is a raw JSON string without `WARN` or `ERROR`,
the event is dropped before insertion. Structured core payloads and WARN/ERROR
raw strings are retained. This filter always runs during sync, including when a
user explicitly syncs `--source idm-core` or `--source am-core`.

## Verified against

- Tenant: `tenant.example.com` (the pingone-aic-manager
  sandbox)
- Date: 2026-06-30 (journey join key re-corrected 2026-07-01 — full
  `trackingIds[0]`, not the stripped base or tree `_id`)
- Calls:
  - `GET /keys` (Bearer, our SA scopes) → **403 insufficient scope** (endpoint
    exists, scope-gated — see scope-gap note above).
  - `GET /monitoring/logs/sources` (api-key pair) → **200**; source list above.
    A prior 2026-06-24 call with a rotated/revoked key returned **401**,
    confirming the api-key auth failure mode.
  - Source list above + `transactionId`/`beginTime`/`endTime`/
    `_pagedResultsCookie` query shapes confirmed against working reference
    scripts (`<client-checkout>/logs/`).
  - `/keys` full lifecycle verified live via an **admin-user bearer**
    (`idmAdminClient` PKCE, scope `openid fr:idm:*`): `GET /keys` → 200 (CREST
    envelope, elements `{api_key_id, created_at, name}`);
    `POST /keys?_action=create {name}` → 200 returning
    `{name, api_key_id, api_key_secret, created_at}`; `DELETE /keys/{id}` → 204.
    SA bearer 403s regardless of scope.
  - Source-taxonomy sampling with a valid log API key:
    `GET /monitoring/logs?source=idm-everything&beginTime=...&endTime=...`,
    `source=idm-core`, `source=am-core`, `source=am-authentication`,
    `source=am-access`, `source=am-activity`, `source=idm-activity`,
    `source=idm-config`, and `source=idm-access`.
  - `idm-everything` sample composition: about 99% `idm-core`; raw string
    payloads with no `_id`, no `eventName`, and no user.
  - `am-authentication` join verified against `<client-checkout>/logs/prod-logs.json`
    (4,152 am-authentication events) AND cross-checked against AIC's own
    `Journey-Node-History` export (146,159 rows): the per-execution key is the
    **full `trackingIds[0]`** (322 executions, median 19 nodes, 138 with a
    matching tree event). The stripped-base and tree-`_id` joins are WRONG (a
    base spans 3,226 executions / 2,502 users); `transactionId` is per-request.
    See the join-key note above. The sandbox tenant itself has only
    module/service-account logins (no tree/node events) in the synced window, so
    journey extraction is verified against this prod capture, not the sandbox.

### Who-did-it fields and duplicate events — 2026-08-10

Tenant `tenant.example.com`, all calls made live by the
verifying agent via `aic logs query` (the stored api-key pair; the
service-account bearer cannot reach `/monitoring/logs`, so `verify-endpoint.sh`
is not usable here). The `am-access` events were generated by a throwaway
`test_aic_who` script create → update → delete cycle; see `04-scripts.md` for
that side of the audit trail.

- `aic logs sources` → **200** (api-key pair present and valid).
- `/payload/component eq "Script" and /payload/eventName eq "AM-ACCESS-OUTCOME" and /payload/request/operation eq "UPDATE"`,
  `--source am-access` → **1 event**; `payload.userId` =
  `id=ad604d54-…,ou=user,ou=am-config`, identical to the script's
  `lastModifiedBy`.
- `/payload/component eq "Script"` alone over the same window → **58 events**.
  The `eventName × operation × status` cross-tab: 14 `ATTEMPT|QUERY`, 12
  `ATTEMPT|READ`, 2 `ATTEMPT|CREATE`, 1 `ATTEMPT|UPDATE` (all with no
  `response.status`), against 14 `OUTCOME|QUERY|SUCCESSFUL`, 10
  `OUTCOME|READ|SUCCESSFUL`, 2 `OUTCOME|READ|FAILED`, 1
  `OUTCOME|CREATE|SUCCESSFUL`, 1 `OUTCOME|CREATE|FAILED`, 1
  `OUTCOME|UPDATE|SUCCESSFUL`.
- The `CREATE|FAILED` event carries `statusCode: "412"`,
  `detail.reason: "Script with UUID … already exist in realm /alpha"` and the
  **same `transactionId`** as the `UPDATE|SUCCESSFUL` 30 ms later — one `PUT`,
  two logged operations. The genuine create has a non-null
  `response.detail.revision`; the update's is `null`.
- Adding `and /payload/http/request/path co "<script-id>"` → **14 events**, all
  for that id: server-side `co` on the path confirmed. Re-run after the delete
  returned the `DELETE|SUCCESSFUL` event. The same window contains
  `http.request.path` values in three realm-path spellings
  (`/am/json/alpha/scripts`, `/am/json/realms/alpha/scripts`,
  `/am/json/realms/root/realms/alpha/scripts`) for the same collection.
- `/payload/objectId sw "config"`, `--source idm-config`, 24 h → **0 events**;
  `/payload/http/request/path co "openidm/config"`, `--source idm-access`, 24 h
  → **66 events** (39 `READ|GET|SUCCESSFUL`, 11 `UPDATE|PUT|SUCCESSFUL`, 11
  `CREATE|PUT|FAILED`, …). Their payload keys are
  `_id, client, eventName, http, level, request, response, roles, server, source, timestamp, topic, transactionId, userId`
  — `eventName: "access"`, no `component`, `userId` a bare uuid, plus `roles`.

### Payload shape, ordering, and range boundaries — 2026-09-10

Tenant `tenant.example.com`, queried live through the stored log API key (the
bearer-only `verify-endpoint.sh` cannot authenticate to `/monitoring/logs`):

- A 15-second `am-everything,idm-everything` range returned 2,034 events over
  three pages: 86 object payloads and 1,948 raw-string payloads. Every event had
  the documented top-level string `timestamp`.
- The three-page response was already ascending by top-level `timestamp`, with
  no adjacent descents, including across page boundaries. This sample does not
  reproduce the earlier report of out-of-order transaction results; consumers
  should still sort a page before presenting it.
- Two adjacent `am-core` queries sharing an exact event timestamp returned that
  event once: in the left `(beginTime, endTime]` window only. This confirms the
  range-boundary behavior documented above.

## Source citations

- frodo-lib: `src/api/cloud/LogApi.ts`.
- Ping docs:
  <https://docs.pingidentity.com/pingoneaic/latest/tenants/audit-debug-logs-pull.html>

## See also

- [`docs/logs-store.md`](../logs-store.md) — local DuckDB sync/search/compact
  and journey rollup implementation details.
