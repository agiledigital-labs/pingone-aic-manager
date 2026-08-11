# 01 — Realms & URL path conventions

Implemented in: —

## Purpose

AIC tenants always expose two realms: `alpha` (customer identities) and `bravo`
(workforce identities). Some APIs are realm-scoped, others are tenant-global.
This file explains how to compose the URL for each.

## Realm path convention

For **realm-scoped AM APIs** (`/am/json/...`), the realm path segment is:

```
/realms/root/realms/{realm}
```

| Realm   | URL segment                           |
| ------- | ------------------------------------- |
| `alpha` | `/realms/root/realms/alpha`           |
| `bravo` | `/realms/root/realms/bravo`           |
| root    | `/realms/root` (rarely needed in AIC) |

Full URL example:

```
{TENANT}/am/json/realms/root/realms/alpha/scripts?_queryFilter=true
```

Use that form everywhere, as a **convention** — one canonical spelling keeps
cache keys, audit-log path matching and code review simple.

It is a convention, not a requirement. An earlier version of this file claimed
the short forms "404"; that is **wrong** (verified live 2026-08-10 — see below).
All three of these return **200** and resolve to the same realm:

| Form                                        | Result                       |
| ------------------------------------------- | ---------------------------- |
| `/am/json/realms/root/realms/alpha/scripts` | 200, 121 results (canonical) |
| `/am/json/realms/alpha/scripts`             | 200, 121 results             |
| `/am/json/alpha/scripts`                    | 200, 121 results             |

Realm resolution is genuine, not a fallback to root: a bravo-only script id
reads **200** under `/am/json/alpha/…` → **404**, under `/am/json/bravo/…` →
**200**; alpha and bravo return their own distinct counts (121 vs 284) in every
form; and a nonexistent realm 404s with a form-specific message
(`/am/json/notarealm/scripts` → `"Resource 'notarealm/scripts' not found"`,
`/am/json/realms/notarealm/scripts` → `"Realm not found"`), which is the control
proving 200 is not a catch-all.

Practical consequence: **do not match audit-log `http.request.path` values
against the long prefix.** Other clients (`~/w/aic/who-changed` among them) use
the short form, and `am-access` records the URL exactly as sent — so one window
contains both spellings for the same resource. Match on the resource id instead.

## Tenant-global APIs (no realm in path)

These have no realm segment at all:

| Family                  | Base path                                            |
| ----------------------- | ---------------------------------------------------- |
| ESV variables / secrets | `/environment/variables`, `/environment/secrets`     |
| ESV startup/restart     | `/environment/startup`                               |
| Logs                    | `/monitoring/logs/*`                                 |
| IDM managed config      | `/openidm/config/managed`                            |
| Realms list/CRUD        | `/am/json/global-config/realms`                      |
| Script context types    | `/am/json/global-config/services/scripting/contexts` |

## Listing realms (verification helper)

```bash
GET /am/json/global-config/realms?_queryFilter=true
Accept-API-Version: protocol=2.0,resource=1.0
```

Response (real, from sandbox):

```json
{
  "result": [
    {
      "_id": "Lw",
      "name": "/",
      "parentPath": null,
      "active": true,
      "aliases": []
    },
    {
      "_id": "L2FscGhh",
      "name": "alpha",
      "parentPath": "/",
      "active": true,
      "aliases": []
    },
    {
      "_id": "L2JyYXZv",
      "name": "bravo",
      "parentPath": "/",
      "active": true,
      "aliases": []
    }
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

### Short realm path forms — 2026-08-10

Tenant `tenant.example.com`, live calls via
`scripts/verify-endpoint.sh` with
`Accept-API-Version: protocol=2.0,resource=1.0`. This **contradicts** the "short
form 404s" claim this file carried since 2026-05-17; see
`99-quirks-and-open-questions.md`.

- `GET /am/json/alpha/scripts?_queryFilter=true` → **200**, 121 results.
- `GET /am/json/realms/alpha/scripts?_queryFilter=true` → **200**, 121 results.
- `GET /am/json/realms/root/realms/alpha/scripts?_queryFilter=true` → **200**,
  121 results (identical count — the canonical form).
- The bravo equivalents of all three → **200**, 284 results each.
- `GET /am/json/alpha/scripts?_queryFilter=name+eq+"extractSecret"` → **200**;
  `…&_fields=…` honoured.
- `GET /am/json/alpha/realm-config/agents/OAuth2Client?_queryFilter=true` →
  **200** (not scripts-specific).
- Realm-isolation control, bravo-only script id `00117ff1-…`:
  `/am/json/alpha/scripts/00117ff1-…` → **404**,
  `/am/json/bravo/scripts/00117ff1-…` → **200** (`SSP-UnlockAccount`); same
  outcome for the `/am/json/realms/{realm}/…` form, whose 404 body is
  `"Script with UUID … could not be found in realm /alpha"`.
- Bogus-realm control: `/am/json/notarealm/scripts?_queryFilter=true` → **404
  "Resource 'notarealm/scripts' not found"**;
  `/am/json/realms/notarealm/scripts?_queryFilter=true` → **404 "Realm not
  found"**.
- `GET /am/json/realms/root/scripts?_queryFilter=true` → **403 "This operation
  is not available in PingOne Advanced Identity Cloud."**

## Source citations

- frodo-lib: `src/utils/ForgeRockUtils.ts` (`getRealmPath`),
  `src/api/RealmApi.ts`.
- fr-config-manager: realm loop in
  `packages/fr-config-push/src/scripts/update-*.js`.
- Ping docs:
  <https://docs.pingidentity.com/pingoneaic/latest/am-rest/rest-realms.html>

## Open questions

- Custom-domain hostnames map directly to a realm without
  `/realms/root/realms/...` in the path. Not yet tested in sandbox (no custom
  domain configured).
