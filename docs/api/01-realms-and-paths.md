# 01 — Realms & URL path conventions

## Purpose
AIC tenants always expose two realms: `alpha` (customer identities) and `bravo`
(workforce identities). Some APIs are realm-scoped, others are tenant-global.
This file explains how to compose the URL for each.

## Realm path convention

For **realm-scoped AM APIs** (`/am/json/...`), the realm path segment is:

```
/realms/root/realms/{realm}
```

| Realm | URL segment |
|-------|-------------|
| `alpha` | `/realms/root/realms/alpha` |
| `bravo` | `/realms/root/realms/bravo` |
| root | `/realms/root` (rarely needed in AIC) |

Full URL example:

```
{TENANT}/am/json/realms/root/realms/alpha/scripts?_queryFilter=true
```

Do **not** use the short form `/realms/alpha` — it 404s.

## Tenant-global APIs (no realm in path)

These have no realm segment at all:

| Family | Base path |
|--------|-----------|
| ESV variables / secrets | `/environment/variables`, `/environment/secrets` |
| ESV startup/restart | `/environment/startup` |
| Logs | `/monitoring/logs/*` |
| IDM managed config | `/openidm/config/managed` |
| Realms list/CRUD | `/am/json/global-config/realms` |
| Script context types | `/am/json/global-config/services/scripting/contexts` |

## Listing realms (verification helper)

```bash
GET /am/json/global-config/realms?_queryFilter=true
Accept-API-Version: protocol=2.0,resource=1.0
```

Response (real, from sandbox):

```json
{
  "result": [
    { "_id": "Lw",       "name": "/",     "parentPath": null, "active": true, "aliases": [] },
    { "_id": "L2FscGhh", "name": "alpha", "parentPath": "/",  "active": true, "aliases": [] },
    { "_id": "L2JyYXZv", "name": "bravo", "parentPath": "/",  "active": true, "aliases": [] }
  ],
  "resultCount": 3,
  "totalPagedResultsPolicy": "NONE"
}
```

**Note:** `_id` is the realm path base64url-encoded. `Lw` = `/`, `L2FscGhh` =
`/alpha`, `L2JyYXZv` = `/bravo`. Use `name` for display; use the path-encoded
`_id` only when calling `/am/json/global-config/realms/{_id}`.

## Quirks

- `parentPath` is `null` for the root realm, not `""`.
- `aliases` is normally empty in AIC.
- Trying to create a new realm via API returns 403 — AIC doesn't allow custom
  realms. Only alpha and bravo exist (plus root).

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-05-17
- Calls: `GET /am/json/global-config/realms?_queryFilter=true` (200 OK).

## Source citations

- frodo-lib: `src/utils/ForgeRockUtils.ts` (`getRealmPath`), `src/api/RealmApi.ts`.
- fr-config-manager: realm loop in `packages/fr-config-push/src/scripts/update-*.js`.
- Ping docs: <https://docs.pingidentity.com/pingoneaic/latest/am-rest/rest-realms.html>

## Open questions

- Custom-domain hostnames map directly to a realm without `/realms/root/realms/...`
  in the path. Not yet tested in sandbox (no custom domain configured).
