# 02 — Headers & API versioning

Implemented in: `src/aic/`

## Purpose
AIC uses CREST (Common REST) versioning via the `Accept-API-Version` header.
Different endpoint families need different protocol/resource versions. Sending
the wrong one usually returns 400 with a "version not supported" message.

## Header cheat sheet (verified)

| Endpoint family | `Accept-API-Version` | Required? |
|---|---|---|
| `/am/oauth2/access_token` | _(none)_ | No |
| `/am/json/global-config/realms` | `protocol=2.0,resource=1.0` | Yes |
| `/am/json/global-config/services/scripting/contexts` | `protocol=2.0,resource=1.0` | Yes |
| `/am/json/{realm-path}/scripts` | `protocol=2.0,resource=1.0` | Yes |
| `/am/json/{realm-path}/realm-config/agents/OAuth2Client` | `protocol=2.1,resource=1.0` | Yes |
| `/am/json/{realm-path}/realm-config/services/oauth-oidc` | `protocol=2.1,resource=1.0` | Yes |
| `/am/json/{realm-path}/realm-config/saml2` | `protocol=2.1,resource=1.0` | Yes |
| `/am/json/{realm-path}/realm-config/federation/circlesoftrust` | `protocol=2.1,resource=1.0` | Likely (frodo-lib) |
| `/am/json/{realm-path}/realm-config/authentication/authenticationtrees/trees` | `protocol=2.0,resource=1.0` | Yes |
| `/environment/variables` | _(defaults to `resource=1.0`)_ | No (works without) |
| `/environment/secrets` | _(defaults to `resource=1.0`)_ | No (works without) |
| `/environment/startup` | _(defaults to `resource=1.0`)_ | No (works without) |
| `/openidm/config/managed` | _(none)_ | No |
| `/monitoring/logs/*` | _(none — uses different auth)_ | No |

## Always-send headers

| Header | Value | Notes |
|---|---|---|
| `Authorization` | `Bearer <token>` | Service-account token. Not for `/monitoring/logs/*`. |
| `Accept` | `application/json` | Required for most JSON APIs. |
| `Content-Type` | `application/json` | On `POST`/`PUT` with a JSON body. |
| `X-Requested-With` | `XMLHttpRequest` | frodo-lib sends this on AM APIs; we should mirror. Probably required by some CSRF guards. |

## CREST query parameters

Used on `_queryFilter`-supporting endpoints (most AM `/realm-config/...` and
`/global-config/...` lists):

| Param | Example | Purpose |
|-------|---------|---------|
| `_queryFilter` | `true` (all) or `name+eq+"foo"` | Required to list. |
| `_pageSize` | `100` | Page size; default varies (1000 for logs). |
| `_pagedResultsCookie` | _(opaque)_ | Continue paging. |
| `_fields` | `_id,name` | Limit returned fields. |
| `_action` | `create`, `restart`, `nextdescendents` | Triggers POST actions. |

## Conditional updates (`If-Match`)

For resources that have `_rev` (OAuth2 clients, journeys, OIDC provider service),
send `If-Match: <_rev>` on `PUT` to enforce optimistic locking. Server returns
412 if remote has changed.

For resources **without** `_rev` (scripts, ESV variables), do content equality
checks instead — see [04-scripts.md](04-scripts.md).

## Transaction tracing

AM responses include `X-ForgeRock-TransactionId: <uuid>`. Log it on errors so
that future `/monitoring/logs` queries can filter by `payload/transactionId eq …`.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-05-17
- Verified the entire "header cheat sheet" table above by making one live call
  per row (where credentials allow).

## Source citations

- frodo-lib: `src/api/*Api.ts` (every file pins its `Accept-API-Version`).
- Ping docs: <https://docs.pingidentity.com/pingoneaic/latest/am-rest/rest-versioning.html>

## Open questions

- The `X-Requested-With: XMLHttpRequest` header — is it actually required for any
  endpoint, or just frodo-lib defensive coding? Test by omitting on a `PUT`.
