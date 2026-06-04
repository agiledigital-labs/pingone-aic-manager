# 11 — IDM custom endpoints

## Purpose
IDM custom endpoints are scripted (or table/jdbc) REST endpoints registered as
IDM config objects under `endpoint/<name>`. Their JavaScript lives in a plain
`source` string. `aic-edit`'s script-sync feature treats them as a second
"script kind" alongside AM scripts (see `04-scripts.md`), sharing the same
content-based conflict-detection core.

## Authentication
Service-account bearer. Scope: `fr:idm:*`. **No realm segment** — IDM config is
tenant-global (`/openidm/...`), unlike realm-scoped AM scripts.

## Endpoints

| Op | Method | Path | Accept-API-Version | Notes |
|----|--------|------|--------------------|-------|
| List config | `GET` | `/openidm/config?_queryFilter=true` | none required | Returns **all** config objects; filter `_id` starting `endpoint/`. |
| Read | `GET` | `/openidm/config/endpoint/{name}` | none required | `{name}` is the bare name (no `endpoint/` prefix in the call? — see below). |
| Create | `PUT` | `/openidm/config/endpoint/{name}` | none required | Returns **201** + echoes the object on create. |
| Update | `PUT` | `/openidm/config/endpoint/{name}` | none required | Returns **200** on replace of an existing object. |
| Delete | `DELETE` | `/openidm/config/endpoint/{name}` | none required | Returns **200** + echoes the deleted object. Subsequent `GET` → 404. |

The path segment after `/config/` is `endpoint/{name}` (e.g.
`/openidm/config/endpoint/test`). In the list response each object's `_id` is
the full `endpoint/{name}`; derive the bare name by stripping the `endpoint/`
prefix.

**No `Accept-API-Version` header is needed** — every call above was exercised
without one and succeeded. (Sending the AM `protocol=2.0,resource=1.0` value is
wrong here; omit it for `/openidm`.)

## Object shape (real example: `endpoint/test`)

```json
{
  "_id": "endpoint/test",
  "type": "text/javascript",
  "source": "(function () {\n  if (request.method === \"create\") { ... }\n})();",
  "description": "…",
  "globalsObject": { }
}
```

- `source` is the JavaScript body as **plain text** (NOT base64 — contrast with
  AM scripts). For scripted endpoints `source` may also appear as a nested
  object `{ "source": "…", "type": "…" }` or be replaced by `"file": "…"`
  (file-backed); handle the string and nested-object forms, fall back to the
  raw config otherwise.
- `type` is a MIME-ish discriminator: `text/javascript` (scripted; also seen as
  `scripted`), `table`, `jdbc`.
- **No `_rev` field** on read or write — same as AM scripts, so conflict
  detection is content-based (see `04-scripts.md` "Conflict detection rule").
- **No `name` field** — the human name is the `_id` suffix.

## Examples

```bash
# List endpoint config ids
curl -sS "$TENANT_BASE_URL/openidm/config?_queryFilter=true" \
  -H "Authorization: Bearer $TOKEN" -H "Accept: application/json" \
  | jq -r '.result[]._id | select(startswith("endpoint/"))'

# Read one
curl -sS "$TENANT_BASE_URL/openidm/config/endpoint/test" \
  -H "Authorization: Bearer $TOKEN"

# Create / update (illustrative — use a throwaway name)
curl -X PUT "$TENANT_BASE_URL/openidm/config/endpoint/my-endpoint" \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"type":"text/javascript","source":"(function(){ return {}; })();","description":"…"}'

# Delete
curl -X DELETE "$TENANT_BASE_URL/openidm/config/endpoint/my-endpoint" \
  -H "Authorization: Bearer $TOKEN"
```

## Scripted-endpoint runtime bindings (`request` / `context`)

Verified 2026-06-04 by creating a throwaway `endpoint/rhino-probe` that echoes
the `request` binding, then invoking it once per CREST method. The endpoint is
invoked at `/openidm/endpoint/<name>` (NOT `/config/...`), and a freshly-created
endpoint takes a few seconds to register (first calls 404 until it does).

`request.method` is the CREST verb, mapped from the HTTP call:

| HTTP call | `request.method` | Method-specific fields (beyond the common set) |
|-----------|------------------|------------------------------------------------|
| `GET /endpoint/x` | `read` | — |
| `GET /endpoint/x?_queryFilter=…` or `?_queryId=…` | `query` | `queryFilter`, `queryId`, `queryExpression` (string\|null), `pageSize`, `pagedResultsOffset` (number), `pagedResultsCookie` (string\|null), `sortKeys` |
| `POST /endpoint/x?_action=create` | `create` | `newResourceId` (string\|null), `content` |
| `POST /endpoint/x?_action=NAME` | `action` | `action` (the action name), `content` |
| `PUT /endpoint/x/id` | `update` | `revision` (string\|null), `content` |
| `PATCH /endpoint/x/id` | `patch` | `revision` (string\|null), `patchOperations` |
| `DELETE /endpoint/x/id` | `delete` | `revision` (string\|null) |

Common to every method: `method`, `resourcePath` (string; `""` at the endpoint
root), `additionalParameters` (a map of any non-`_` query params), `fields`
(the `_fields` list).

- **`patchOperations`** is a list of `{ operation, field, value }` (mirrored over
  the wire as an index-keyed object `{"0": {...}}` because it's a Java list).
  `operation` is `add`/`remove`/`replace`/`increment`/`move`/`copy`/`transform`,
  `field` is a JSON pointer (`/foo`). This is the strict body that has blocked
  mocking other PATCH APIs — the request must be CREST patch ops, not an
  arbitrary JSON body.
- **`content`** is the request body for create/update/action.
- **`context`** is the CREST call chain. The originating HTTP request is at
  `context.http`: `{ method, path, headers (map), parameters (map) }`. Identity
  is at `context.security`: `{ authenticationId, authorization: { id, component,
  roles } }`. Many other contexts exist (`oauth2`, `transactionId`, `session`,
  `current`, `parent`, …) and vary by call. (Note `context.http.headers`
  includes the bearer `Authorization` — never log the full context.)

### Response shape

The script's returned value is the HTTP response body. A **`query` handler MUST
return** `{ result: [...], resultCount, pagedResultsCookie, … }` — returning a
plain object fails with `500 "Script returned unexpected query result structure
of type class java.util.HashMap"`. Other methods return a resource object. The
return value is not statically type-checkable (a script isn't a typed function),
but `idm/types/endpoint.d.ts` exposes `IdmQueryResult` / `IdmResource` aliases to
annotate with `/** @type {…} */`.

## Quirks

- **`source` is plain text, not base64** (the opposite of AM scripts). Don't
  base64-encode on write.
- **No `_rev`** — content-based conflict detection only.
- **No `Accept-API-Version`** — `/openidm` config does not require (or want) the
  AM versioning header.
- **List is unfiltered** — `/openidm/config?_queryFilter=true` returns *every*
  config object (85 in the sandbox); filter client-side for `endpoint/` ids.
- **`PUT` is create-or-replace** — 201 on first write, 200 on replace.
- Some shipped endpoints (e.g. `oauthproxy`, `gettasksview`) are product
  defaults; treat with the same care as AM `default:true` scripts (avoid
  clobbering unless the user explicitly pulls + edits them).

## Scheduled jobs (`schedule/<name>`)

IDM scheduled jobs are config objects at `/openidm/config/schedule/<name>` —
same CRUD, no realm, no `_rev`. `aic-edit` syncs them as a second IDM script
kind (`--kind schedule`). They differ from endpoints in **where the script
lives** and **which ones have one**:

- The script is nested at **`invokeContext.script.source`** (plaintext), with
  `invokeContext.script.type: "text/javascript"`. A `globals` object may sit
  beside `source`.
- **Only `invokeService: "script"` schedules carry an inline script.** Others
  (`taskscanner`, `sync`, …) have an `invokeContext` but no `script.source`
  (they reference a script by `scriptProperty`, or scan managed objects) —
  filter these out when listing syncable schedules.
- On write, merge only `invokeContext.script.source` and round-trip the rest
  (`schedule`, `enabled`, `persisted`, `type`, `globals`, …) so the cron
  definition and flags aren't lost.

Object shape (real example, `schedule/UpdateReviewList`):

```json
{
  "_id": "schedule/UpdateReviewList",
  "enabled": true,
  "type": "cron",
  "schedule": "0 0 2 * * ?",
  "persisted": true,
  "invokeService": "script",
  "invokeContext": {
    "type": "text/javascript",
    "script": { "type": "text/javascript", "source": "var oneDay = …", "globals": {} }
  }
}
```

> When creating a schedule for testing, set `"enabled": false` so it doesn't
> actually fire on its cron.

## Verified against
- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-06-01 (CRUD); 2026-06-04 (`request`/`context` runtime shapes per
  method, via `endpoint/rhino-probe` echo — created, probed read/create/update/
  patch/delete/action/query, deleted)
- Endpoints: `GET /openidm/config?_queryFilter=true` (200; 85 objects, 12 with
  `endpoint/` ids), `GET /openidm/config/endpoint/test` (200; keys
  `_id, description, source, type`, no `_rev`, plaintext `source`),
  `PUT /openidm/config/endpoint/aicedit-verify` (201 create), `PUT` again
  (200 replace), `DELETE` (200), `GET` after delete (404).
- Schedules (2026-06-01): `GET …?_queryFilter=true` → 4 `schedule/` configs,
  3 `taskscanner` (no inline script) + 1 `invokeService:"script"`
  (`UpdateReviewList`, script at `invokeContext.script.source`, no `_rev`).
  Throwaway `schedule/aicedit-sched` (disabled): `PUT` 201 create, `PUT` 200
  replace, source-only push preserved `enabled`/`schedule`/`script.type`,
  `DELETE` 200. Removed after the run.

## Source citations
- frodo-lib: `src/api/IdmConfigApi.ts` (`getConfigEntity` / `putConfigEntity`).
- fr-config-manager: `packages/fr-config-pull/src/scripts/endpoints` (IDM endpoints).
- p1aic-script-editor: `src/resources/EndpointHandler.ts`,
  `src/schemas/endpoint.ts` (Zod schema covers scripted/table/jdbc + nested
  `source`).

## Open questions
- Does `PUT` accept (and is it advisable to send) the `globalsObject` field
  round-tripped, or should it be stripped like OAuth2 `-encrypted` fields?
  Not yet tested; current plan round-trips the full config minus nothing.
- Table/JDBC endpoint write shapes are documented from the p1-sync Zod schema
  only — not yet exercised live (sandbox has only scripted endpoints).
