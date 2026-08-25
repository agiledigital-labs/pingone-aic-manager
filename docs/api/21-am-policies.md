# 21 — AM policies (policy sets, resource types, evaluation)

Implemented in: **`src/policy/`** (`aic policy`, CLI-only — see
[`../CLI.md`](../CLI.md)). The Terraform resources are still to come; see
`../../../aic-demos/capability-tokens/PLAN.md`.

## Purpose

AM's entitlement engine: **resource types** name a resource space and the
actions over it, a **policy set** (`applications` on the wire) scopes a group of
policies to one or more resource types, and a **policy** grants actions to a
subject under conditions. `?_action=evaluate` is the PDP endpoint a resource
server calls to ask "may this token do this, here, now".

The capability-token demo uses it twice: once at token-mint time to decide which
capabilities a user may hold, and once at call time to decide whether a
presented token may act.

## Authentication

**Service-account bearer, scope `fr:am:*` — sufficient for everything below**,
reads and writes alike: resource types, policy sets, policies, and
`?_action=evaluate`. No admin-user bearer, no console step.

Note what the caller's own identity does to `evaluate`: with no `subject` in the
body, AM evaluates **as the caller**, and the service account satisfies
`AuthenticatedUsers`. That is a convenient probe and a trap — a policy that
looks satisfied from `curl` may be satisfied only because the SA is the subject.

## Endpoints

`{realm-path}` = `/realms/root/realms/bravo` (or `alpha`). Policies and policy
sets need `Accept-API-Version: protocol=1.0,resource=2.0`; resource types,
`subjecttypes` and `conditiontypes` answer on the default `resource=1.0`.

| Op                    | Method   | Path                                          | Notes                                                                     |
| --------------------- | -------- | --------------------------------------------- | ------------------------------------------------------------------------- |
| List policies         | `GET`    | `/am/json{realm-path}/policies?_queryFilter=true` | `resource=1.0` and `2.0` both answer; `2.0` adds `resourceTypeUuid`.  |
| Read policy           | `GET`    | `…/policies/{name}`                           |                                                                           |
| **Create** policy     | `POST`   | `…/policies?_action=create`                   | 201. `name` goes in the body.                                             |
| Update policy         | `PUT`    | `…/policies/{name}`                           | 200. **Update only** — see the create asymmetry below.                    |
| Delete policy         | `DELETE` | `…/policies/{name}`                           | 200.                                                                      |
| Evaluate              | `POST`   | `…/policies?_action=evaluate`                 | The PDP call. Body and semantics below.                                   |
| Evaluate tree         | `POST`   | `…/policies?_action=evaluateTree`             | `resource` (singular). Returned rows were empty in our probe — see Quirks. |
| List policy sets      | `GET`    | `…/applications?_queryFilter=true`            | Each set carries its permitted `conditions[]` / `subjects[]`.             |
| Read policy set       | `GET`    | `…/applications/{name}`                       |                                                                           |
| **Create** policy set | `POST`   | `…/applications?_action=create`               | 201.                                                                      |
| Update policy set     | `PUT`    | `…/applications/{name}`                       | 200. **Update only.**                                                     |
| Policy-set template   | `POST`   | `…/applications?_action=template`             | **501 Not Implemented.** Copy a stock set instead.                        |
| List resource types   | `GET`    | `…/resourcetypes?_queryFilter=true`           |                                                                           |
| **Create** / update RT | `PUT`   | `…/resourcetypes/{id}`                        | **201 on create with an id you choose** — it need not be a UUID.          |
| Subject-type schemas  | `GET`    | `…/subjecttypes?_queryFilter=true`            | JSON Schema per subject type. **Authoritative catalog source.**           |
| Condition-type schemas | `GET`   | `…/conditiontypes?_queryFilter=true`          | Ditto for conditions.                                                     |

### The create asymmetry — read this before writing a client

Three sibling collections, three different create contracts:

- **resource types** create with `PUT /resourcetypes/{id}` → **201**, and the id
  is whatever you pass (`CapTokenDemoShopApi` was accepted; it does not have to
  look like a UUID). The stock types happen to use UUIDs, which makes this easy
  to miss.
- **policies** and **policy sets** refuse `PUT` to a name that does not exist —
  `404 "Policy X does not exist."` / `404 "X not found."`. They create only via
  `POST ?_action=create` with the name in the body.

So a provisioning tool cannot use one idempotent verb across all three. This is
the same shape as IDM internal roles vs managed users (see
[18](18-internal-roles.md), [10](10-managed-objects.md)) and it is worth encoding
in the catalog rather than rediscovering per resource.

## Object shapes

### Resource type

```jsonc
{
  "name": "CapTokenDemo Shop API",
  "description": "…",
  "patterns": ["https://*:*/orders/*", "https://*:*/payments/*"],
  "actions": { "read": true, "approve": false, "refund": false }  // value = default
}
```

`actions` values are the **default decision** for that action, not a grant.

### Policy set (`applications`)

```jsonc
{
  "name": "CapTokenDemo",
  "displayName": "Capability Tokens Demo",
  "resourceTypeUuids": ["CapTokenDemoShopApi"],
  "applicationType": "iPlanetAMWebAgentService",
  "entitlementCombiner": "DenyOverride",
  "conditions": ["OAuth2Scope", "AND", "OR", "NOT", "Script", "SimpleTime"],
  "subjects":   ["JwtClaim", "AuthenticatedUsers", "Identity", "AND", "OR", "NOT", "NONE"]
}
```

`conditions` and `subjects` are the **vocabulary this set's policies may use**.
A restricted list is accepted verbatim on create — the stock sets list ~19
conditions and ~7 subjects, but nothing requires that. Widening it later is a
plain `PUT` of the set.

### Policy

```jsonc
{
  "name": "CapTokenDemo_OrdersApprove",
  "active": true,
  "applicationName": "CapTokenDemo",
  "resourceTypeUuid": "CapTokenDemoShopApi",
  "resources": ["https://*:*/orders/*"],
  "actionValues": { "approve": true },
  "subject": { "type": "AND", "subjects": [
    { "type": "JwtClaim", "claimName": "demoRoles", "claimValue": "orders.approver" },
    { "type": "JwtClaim", "claimName": "scope",     "claimValue": "orders.approve"  }
  ]},
  "condition": { "type": "OAuth2Scope", "requiredScopes": ["orders.approve"] }  // optional
}
```

`subject` and `condition` are **recursive discriminated unions**: `AND` and `OR`
carry `subjects[]` / `conditions[]`, `NOT` carries `subject` / `condition`, and
the leaves are typed per `/subjecttypes` and `/conditiontypes`. That recursion
is the shape a typed catalog has to model — `internal/idm/` in the Terraform
provider is the precedent, not the flat node specs.

### The type catalogs

`GET …/subjecttypes?_queryFilter=true` and `…/conditiontypes?_queryFilter=true`
return a JSON Schema per type. Verified fields (bravo, 2026-08-25):

| Subject | Fields |
| ------- | ------ |
| `AND` / `OR` | `subjects` |
| `NOT` | `subject` |
| `AuthenticatedUsers`, `NONE` | *(none)* |
| `Identity` | `subjectValues` |
| `JwtClaim` | `claimName`, `claimValue` |
| `Policy` | `className`, `name`, `values` |

| Condition | Fields |
| --------- | ------ |
| `AND` / `OR` | `conditions` · `NOT`: `condition` |
| `OAuth2Scope` | `requiredScopes` |
| `IdmUser` | `identityResource`, `queryField`, `decisionField`, `comparator` (`EQUALS`/`CONTAINS`/`STARTS_WITH`/`ENDS_WITH`/`REGEX`), `value` |
| `Script` | `scriptId` |
| `SimpleTime` | `startTime`, `endTime`, `startDay`, `endDay`, `startDate`, `endDate`, `enforcementTimeZone` |
| `AuthLevel` / `LEAuthLevel` | `authLevel` |
| `IPv4` / `IPv6` | `startIp`, `endIp`, `dnsName` |
| `AMIdentityMembership` | `amIdentityName` |
| `AuthScheme` | `authScheme`, `applicationName`, `applicationIdleTimeout` |
| `AuthenticateToRealm` / `AuthenticateToService` | `authenticateToRealm` / `authenticateToService` |
| `LDAPFilter` | `ldapFilter` · `SessionProperty`: `properties`, `ignoreValueCase` |
| `Session` | `maxSessionTime`, `terminateSession` · `Transaction`: `authenticationStrategy`, `strategySpecifier` |
| `ResourceEnvIP` | `resourceEnvIPConditionValue` · `Policy`: `className`, `properties` |

`conditiontypes` lists types (`IdmUser`) that the stock policy sets do **not**
permit. Availability in the realm and permission in the set are two different
things; check the set's `conditions[]` before writing a policy.

## Evaluation

```jsonc
POST /am/json{realm-path}/policies?_action=evaluate
Accept-API-Version: protocol=1.0,resource=2.0
{
  "resources":   ["https://shop-api.demo:443/orders/123"],
  "application": "CapTokenDemo",
  "subject":     { "jwt": "<a signed JWT>" },
  "environment": { "scope": ["orders.approve"] }     // optional
}
```

Response, one row per requested resource:

```jsonc
[{ "resource": "…", "actions": { "approve": true }, "attributes": {}, "ttl": 9223372036854775807, "advices": {} }]
```

### Subject forms

| Form | Behaviour |
| ---- | --------- |
| *omitted* | Evaluates as the **caller**. A service-account bearer satisfies `AuthenticatedUsers`. |
| `{"ssoToken": "…"}` | A real AM SSO token. Handing it an OAuth2 access token is `400 "Invalid value subject"`. |
| `{"jwt": "<signed JWT>"}` | **Accepted, and this is the useful one.** A malformed string is rejected `400`, so the JWT really is parsed. |
| `{"claims": {…}}` | Accepted, but satisfies neither `AuthenticatedUsers` nor `JwtClaim` — effectively an anonymous subject. Do not reach for it. |

### `subject.jwt` is unauthenticated input — the PEP must verify the token

**AM does not verify the JWT.** Not the signature, not the expiry. Verified
2026-08-25, twice, and both results are worth stating flatly:

- A capability token whose claims were rewritten — `scope` and the roles claim
  replaced, the original signature left in place and therefore invalid — was
  accepted, and the policy granted the action the forged claims asked for. Bob,
  who holds `orders.read` only, got `{"approve": true}`.
- A token that had **expired nearly two hours earlier** also got
  `{"approve": true}`.

`subject.jwt` means "here are some claims the caller asserts", not "here is an
authenticated identity". That is defensible — the PDP is answering a
hypothetical, and a policy engine that re-implemented token validation would be
the wrong place for it — but it puts the entire burden on the caller, and
nothing in the API's shape hints at that.

So a resource server evaluating a presented token **must**, before it calls the
PDP:

1. verify the signature against the realm's JWKS (`{issuer}/connect/jwk_uri`),
2. check `iss` is the realm and `aud` is itself,
3. check `exp` / `nbf`,
4. and only then pass the token as `subject.jwt`.

Skip any of that and the "policy decides" story is theatre: the caller decides,
by writing whatever claims it likes. A PEP that verifies locally and then
evaluates is doing something real; one that only evaluates is an open door.

Do not generalise this to the rest of AM. The **token endpoint** does verify:
the same forged token presented as an RFC 8693 `subject_token` is rejected with
`invalid_request` ([22-token-exchange.md](22-token-exchange.md)). It is
specifically `?_action=evaluate` that takes the caller's word for the subject.

**With a `jwt` subject, `AuthenticatedUsers` never matches.** A JWT is not a
session, so a policy written for browser traffic silently grants nothing to an
API caller. Use `JwtClaim`.

**`JwtClaim` matches inside an array claim.** With `demoRoles:
["orders.reader","orders.approver"]`, a `claimValue` of `"orders.approver"`
matches. The same holds for the standard `scope` claim, which is the whole
trick behind the capability-token demo: the policy can require
`JwtClaim(scope = orders.approve)` and read the capability out of the presented
token, instead of trusting the PEP to declare it.

### Reading a decision

(The subject is not authenticated by AM — see the section above before trusting
any of these answers.)

- `actions: {"approve": true}` — granted.
- `actions: {}` — **no policy applied.** Ambiguous by design: the resource
  matched nothing, or the subject failed, or a condition failed. There is no way
  to tell these apart from the response.
- `actions: {"approve": false}` — a policy matched and denied.
- `advices` was empty in every allow and every deny we produced.
- `ttl` was `9223372036854775807` (Long.MAX_VALUE) in every response.

### Resource strings use the URL comparator

`applicationType: iPlanetAMWebAgentService` means resources are compared as
URLs. `shop://orders/123` did **not** match the pattern `shop://orders/*`;
`https://shop-api.demo:443/orders/123` matched `https://*:*/orders/*`. Write
patterns as `scheme://host:port/path` and nothing will surprise you; write them
any other way and the mismatch is silent, because a resource that matches no
pattern is reported as `actions: {}` — the same answer as a deny.

#### Wildcard semantics, measured

Verified 2026-08-25 with a throwaway resource type, policy set and
`AuthenticatedUsers` policy in `bravo`, evaluated as the service-account caller
so a match reads `{"read": true}` and a non-match `{}`. The probe objects were
deleted afterwards. Every row below is an observation, not a reading of the
vendor docs — which describe `*` the other way round.

| Pattern | Resource | Result |
| ------- | -------- | ------ |
| `https://*:*/g/*` | `https://x:443/g/one` | ✅ |
| `https://*:*/g/*` | `https://x:443/g/one/two` | ✅ **crosses `/`** |
| `https://*:*/g/*` | `https://x:443/g/` | ✅ matches zero characters |
| `https://*:*/g/*` | `https://x:443/g` | ❌ the literal `/` must be present |
| `https://*:*/h/-*-` | `https://x:443/h/one` | ✅ |
| `https://*:*/h/-*-` | `https://x:443/h/one/two` | ❌ **`-*-` is the single-level wildcard** |
| `https://*:*/i/*/z` | `https://x:443/i/one/two/z` | ✅ a mid-pattern `*` crosses `/` too |
| `https://*:*/lit/One` | `https://x:443/LIT/one` | ✅ **matching is case-insensitive** |
| `https://*:*/g/*` | `https://x:443/g/one?b=1` | ❌ **a query string is part of the resource** |
| `https://*:*/q/a?*` | `https://x:443/q/a?b=1` | ✅ … and needs its own `?*` |
| `https://*:*/q/a?*` | `https://x:443/q/a` | ❌ `?*` then requires a query string |
| `https://*:*/t/*/` | `https://x:443/t/one` | ❌ a trailing `/` is significant |
| `https://*:*/g/*` | `https://x/g/one` | ✅ a missing port defaults by scheme |
| `https://*:*/g/*` | `http://x:80/g/one` | ❌ the scheme is compared literally |

Three of those are worth pulling out, because each produces a silent `{}` that
reads like an authorization bug:

- **`*` crosses `/` and `-*-` does not.** This is the opposite of the glob
  intuition and the opposite of how the AM console's help describes it. If you
  want "one path segment", `-*-` is the wildcard you need.
- **A query string is part of the resource being matched.** A PEP that hands the
  PDP the full request URL — `…/orders/123?expand=lines` — gets no match against
  `…/orders/*`. Either strip the query before evaluating, or add `?*` patterns.
  Prefer stripping: `…/orders/*` and `…/orders/*?*` are two patterns to keep in
  step forever.
- **Matching is case-insensitive**, including the path. Do not lean on case to
  separate two resources.

## Examples

The capability-token chain, end to end (bravo, 2026-08-25). `demoRoles` is put
into the token by an access-token-modification script; see
[05](05-oauth2-oidc.md).

```sh
# 1. Log in. The identity token deliberately carries no capability.
curl -su "$CLIENT" -d grant_type=password -d username=alice@captoken.demo \
  -d password=… -d scope=openid \
  "$TENANT_BASE_URL/am/oauth2/realms/root/realms/bravo/access_token"
# → scope ["openid"], demoRoles ["orders.reader","orders.approver"]

# 2. Exchange it for one capability.
curl -su "$CLIENT" -d grant_type=urn:ietf:params:oauth:grant-type:token-exchange \
  -d subject_token="$BASE" -d subject_token_type=urn:ietf:params:oauth:token-type:access_token \
  -d requested_token_type=urn:ietf:params:oauth:token-type:access_token \
  -d scope=orders.approve "$TENANT_BASE_URL/am/oauth2/…/access_token"
# → scope ["orders.approve"]        (payments.refund would come back empty)

# 3. Ask the PDP.
scripts/aicurl.sh POST "/am/json/realms/root/realms/bravo/policies?_action=evaluate" \
  --apiver protocol=1.0,resource=2.0 \
  --data '{"resources":["https://shop-api.demo:443/orders/123"],
           "application":"CapTokenDemo","subject":{"jwt":"'"$CAP"'"}}'
```

Observed matrix, with no `environment` at all — the policies read everything
they need out of the presented token:

| Token | Resource | `actions` |
| ----- | -------- | --------- |
| alice, `scope=[orders.approve]`, roles reader+approver | `/orders/123` | `{"approve": true}` |
| alice, same token | `/payments/9` | `{}` |
| bob, `scope=[orders.read]`, role reader | `/orders/123` | `{"read": true}` |
| **alice's base token, `scope=[openid]`** | `/orders/123` | `{}` |

The last row is the point of the pattern: same user, same resource, denied,
because the token she is holding does not carry the capability.

## Quirks

### Stock resource types share UUIDs across realms but are not shared

`URL` is `76656a38-5f8e-401b-83aa-4ccb74ce88d2` in both `alpha` and `bravo`, yet
`alpha`'s has 210 patterns and `bravo`'s 475, with different modification dates.
The stock types are seeded from a common template, so **a matching UUID is not
evidence of a shared object**. Editing one realm's `URL` does not touch the
other's — but do not rely on that from the id alone; read both.

### `OAuth2Scope` reads `environment.scope`, singular

The condition takes its scopes from the evaluation environment, not from the
subject JWT. The key is **`scope`** — an array of strings. `scopes`,
`oauth2_scope` and `OAuth2Scope` are all silently ignored, and the condition
then fails with no advice:

```jsonc
"environment": { "scope": ["orders.read"] }     // works
"environment": { "scopes": ["orders.read"] }    // ignored; policy does not apply
```

Prefer `JwtClaim(scope = …)` where the subject is a JWT. It reads the same
capability out of the token itself, so a buggy or hostile PEP cannot assert a
scope the token never carried.

### `AuthLevel` + a `jwt` subject is a 500

A policy carrying `{"type": "AuthLevel", "authLevel": 10}` evaluated against
`subject: {"jwt": …}` returns `500 Internal error` — not a deny, not an advice.
Session-oriented conditions (`AuthLevel`, and by inspection `Session`,
`AuthScheme`, `Transaction`, `SessionProperty`) assume an SSO token. Keep them
out of any policy set a resource server evaluates with a JWT.

### `evaluateTree` returned nothing useful

`?_action=evaluateTree` with `resource` (singular) returned the requested
resource **plus** the policies' pattern strings as extra rows, every one with
`actions: {}` — including the exact resource that `?_action=evaluate` had just
granted. Unexplained; not needed for the demo, and not to be used until someone
works out what it is actually reporting.

### `?_action=template` is 501 on policy sets

Unlike OAuth2 clients and `TrustedJwtIssuer`, there is no template action for
`applications`. To learn the default shape, `GET` a stock set (`oauth2Scopes`)
and strip its identity fields.

## Verified against

- Sandbox tenant, realms `alpha` (reads only) and `bravo` (reads and writes),
  **2026-08-25**, with a service-account bearer via `aic whoami --token`.
- Exercised: `GET`/`POST ?_action=create`/`PUT`/`DELETE` on `policies`;
  `GET`/`POST ?_action=create`/`PUT` on `applications`; `POST ?_action=template`
  on `applications` (501); `PUT` create and update on `resourcetypes`;
  `GET ?_queryFilter=true` on `subjecttypes` and `conditiontypes`;
  `POST ?_action=evaluate` with no subject, `claims`, `ssoToken`, a valid `jwt`
  and a malformed `jwt`; `POST ?_action=evaluateTree`.
- **Wildcard semantics** measured 2026-08-25 with a throwaway `ZZProbeGlob`
  resource type, `ZZProbeGlobSet` policy set and an `AuthenticatedUsers` policy,
  19 resources across 6 patterns. All three probe objects were deleted after.
- Objects created and **left in place** in `bravo`: resource type
  `CapTokenDemoShopApi`; policy sets `CapTokenDemo` and `CapTokenDemoScopes`;
  policies `CapTokenDemo_{OrdersRead,OrdersApprove,PaymentsRefund}` and
  `CapTokenDemoScope_{orders.read,orders.approve,payments.refund}`. Probe
  policies (`*_ZZ_*`) were deleted.

## Source citations

None. This file is first-hand observation; no frodo-lib or Ping docs claim was
transcribed. The endpoint names came from the AM console's own REST traffic
shape and were confirmed by call.

## Open questions

1. **Does `usePolicyEngineForScope` ever see the resource owner?** On the
   password and token-exchange grants it does not (see
   [05](05-oauth2-oidc.md)); `authorization_code`, which has a real session,
   is untested. The `CapTokenDemoScopes` policy set is left in `bravo` for that
   retest and is **not** wired to anything today.
2. **What produces an `advices` payload?** Every allow and every deny we
   generated returned `{}`. The conditions that classically emit advice
   (`AuthLevel`, `Transaction`, `AuthScheme`) are exactly the ones that 500 on a
   JWT subject, so it may be unreachable for API-style evaluation.
3. **`evaluateTree` semantics** — see Quirks.
4. **`resourceAttributes`** on a policy (the `attributes` in the response) was
   never populated; the field is unexercised.
5. **Conditional writes.** Policies carry `lastModifiedDate` but no `_rev`, and
   `If-Match` was not tried. Assume content snapshots per CLAUDE.md §5 until
   proven otherwise.
6. **Delegation.** Whether a non-admin bearer can be granted evaluate-only
   rights on one policy set — relevant if the demo's resource server should hold
   something weaker than a full service account.
