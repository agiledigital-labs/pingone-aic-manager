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

## Verified against
- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-05-31
- Calls: `GET /openidm/config?_queryFilter=true` (200; 85 objects, 12 with
  `endpoint/` ids), `GET /openidm/config/endpoint/test` (200; keys
  `_id, description, source, type`, no `_rev`, plaintext `source`),
  `PUT /openidm/config/endpoint/aicedit-verify` (201 create, echoes object),
  `PUT` again (200 replace), `DELETE` (200 echoes object),
  `GET` after delete (404). Throwaway endpoint removed after the run.

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
