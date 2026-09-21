# 02 — Headers & API versioning

Implemented in: `src/aic/`

## Purpose

AIC uses CREST (Common REST) versioning via the `Accept-API-Version` header.
Different endpoint families need different protocol/resource versions. Sending
the wrong one usually returns 400 with a "version not supported" message.

## Header cheat sheet (verified)

| Endpoint family                                                               | `Accept-API-Version`           | Required?                                                                                                    |
| ----------------------------------------------------------------------------- | ------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `/am/oauth2/access_token`                                                     | _(none)_                       | No                                                                                                           |
| `/am/json/global-config/realms`                                               | `protocol=2.0,resource=1.0`    | Yes                                                                                                          |
| `/am/json/global-config/services/scripting/contexts`                          | `protocol=2.0,resource=1.0`    | Yes                                                                                                          |
| `/am/json/{realm-path}/scripts`                                               | `protocol=2.0,resource=1.0`    | Yes                                                                                                          |
| `/am/json/{realm-path}/realm-config/agents/OAuth2Client`                      | `protocol=2.1,resource=1.0`    | Yes                                                                                                          |
| `/am/json/{realm-path}/realm-config/services/oauth-oidc`                      | `protocol=2.1,resource=1.0`    | Yes                                                                                                          |
| `/am/json/{realm-path}/realm-config/saml2`                                    | `protocol=2.1,resource=1.0`    | Yes                                                                                                          |
| `/am/json/{realm-path}/realm-config/federation/circlesoftrust`                | `protocol=2.1,resource=1.0`    | Likely (frodo-lib)                                                                                           |
| `/am/json/{realm-path}/realm-config/authentication/authenticationtrees/trees` | `protocol=2.0,resource=1.0`    | Yes                                                                                                          |
| `/am/json/serverinfo/*`                                                       | `protocol=1.0,resource=1.1`    | No (200 with no version header too). `cookieName` is here — tenant-specific, do not hardcode.                |
| `/am/json/{realm-path}/authenticate`                                          | `protocol=1.0,resource=2.1`    | Yes for the journey conversation. `resource=2.0` also returns the same callback shape (verified 2026-09-03). |
| `/environment/variables`                                                      | _(defaults to `resource=1.0`)_ | No (works without)                                                                                           |
| `/environment/secrets`                                                        | _(defaults to `resource=1.0`)_ | No (works without)                                                                                           |
| `/environment/startup`                                                        | _(defaults to `resource=1.0`)_ | No (works without)                                                                                           |
| `/openidm/config/managed`                                                     | _(none)_                       | No                                                                                                           |
| `/monitoring/logs/*`                                                          | _(none — uses different auth)_ | No                                                                                                           |

## Always-send headers

| Header             | Value              | Notes                                                                                     |
| ------------------ | ------------------ | ----------------------------------------------------------------------------------------- |
| `Authorization`    | `Bearer <token>`   | Service-account token. Not for `/monitoring/logs/*`.                                      |
| `Accept`           | `application/json` | Required for most JSON APIs.                                                              |
| `Content-Type`     | `application/json` | On `POST`/`PUT` with a JSON body.                                                         |
| `X-Requested-With` | `XMLHttpRequest`   | Required for anonymous POST to an IDM custom endpoint (verified 2026-09-21): a matching anonymous `config/access` grant still returned 403 without it; create/action succeeded with it. Continue mirroring it on AM calls. |

## CREST query parameters

Used on `_queryFilter`-supporting endpoints (most AM `/realm-config/...` and
`/global-config/...` lists):

| Param                 | Example                                | Purpose                                    |
| --------------------- | -------------------------------------- | ------------------------------------------ |
| `_queryFilter`        | `true` (all) or `name+eq+"foo"`        | Required to list.                          |
| `_pageSize`           | `100`                                  | Page size; default varies (1000 for logs). |
| `_pagedResultsCookie` | _(opaque)_                             | Continue paging.                           |
| `_fields`             | `_id,name`                             | Limit returned fields.                     |
| `_action`             | `create`, `restart`, `nextdescendents` | Triggers POST actions.                     |

## Conditional updates (`If-Match`)

For resources that have `_rev` (OAuth2 clients, journeys, OIDC provider
service), send `If-Match: <_rev>` on `PUT` to enforce optimistic locking. Server
returns 412 if remote has changed.

For resources **without** `_rev` (scripts, ESV variables), do content equality
checks instead — see [04-scripts.md](04-scripts.md).

## Transaction tracing

AM responses include `X-ForgeRock-TransactionId: <uuid>`. Log it on errors so
that future `/monitoring/logs` queries can find the request.

**The header is also accepted on the request, and the supplied value wins**
(verified 2026-09-14). Send `x-forgerock-transactionid: <your-id>` and the
response echoes it back unchanged — no override by the edge, no decoration —
and `/monitoring/logs` serves the resulting events under it. A caller that
names its own id therefore knows the log key *before* it makes the call,
instead of having to read a response header and correlate afterwards.

AM stores the id with a `/N/M` sub-request suffix appended
(`<your-id>/0/0`, `<your-id>/0/1`), but the log query matches on a **prefix**,
so the bare id you sent retrieves them — see
[08-logs.md](08-logs.md#transaction-id-matching-is-a-prefix-match) for the
matching rule and the collision it creates for sequentially numbered ids.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-09-21
- Calls: anonymous POST to throwaway IDM scripted endpoints with a matching
  `roles: "*"` access rule. Bare create returned 403 without
  `X-Requested-With` and 201 with it; a named action returned 403 without it
  and 200 with it. The authenticated controls succeeded, and anonymous GET
  under the same wildcard rule returned 200, isolating the header requirement
  to the unsafe anonymous method rather than routing or rule propagation. All
  temporary endpoint configs and rules were removed and their absence
  confirmed.
- Date: 2026-09-14
- Calls: `GET /am/json/realms/root/realms/alpha/scripts?_queryFilter=true`
  sent twice — once with `x-forgerock-transactionid: <supplied-id>`, once
  without as a control. The supplied arm returned 200 echoing the header
  verbatim; the control returned a bare server-generated UUID. Both arms'
  events were then retrievable from `/monitoring/logs`, which is what makes
  the supplied-id claim about logging and not only about the response header.
- Date: 2026-05-17
- Verified the entire "header cheat sheet" table above by making one live call
  per row (where credentials allow).
- Date: 2026-09-03
- Calls: `GET /am/json/serverinfo/*` with
  `Accept-API-Version: protocol=1.0,resource=1.1` (200, `cookieName` is a
  15-char hex string) and with no version header (also 200).
  `POST …/authenticate?authIndexType=service&authIndexValue=TxnDemoLogin` with
  `protocol=1.0,resource=2.1` and again with `resource=2.0` — both returned
  `authId` plus `NameCallback` and `PasswordCallback`.

## Source citations

- frodo-lib: `src/api/*Api.ts` (every file pins its `Accept-API-Version`).
- Ping docs:
  <https://docs.pingidentity.com/pingoneaic/latest/am-rest/rest-versioning.html>

## Open questions

- Whether any authenticated AM `PUT` independently requires
  `X-Requested-With: XMLHttpRequest` remains unmeasured. Its necessity for
  anonymous IDM custom-endpoint POST is resolved above.
