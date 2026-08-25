# 22 — OAuth2 token exchange (RFC 8693) and the mint-time scope gate

Implemented in: **nothing yet.** Discovery for the capability-token demo
(`../../../aic-demos/capability-tokens/PLAN.md`).

## Purpose

Trade one token for another: present a long-lived, low-privilege **identity
token** and receive a short-lived token narrowed to a single **capability**.
This is the mechanism behind the capability-token pattern — the client holds
almost nothing most of the time, and asks for exactly the power it needs, per
call.

The grant is `urn:ietf:params:oauth:grant-type:token-exchange` at the ordinary
realm token endpoint. Two things have to be arranged before it works, and a
third before it means anything:

1. the client must be allowed the grant type;
2. the **subject token must carry a `may_act` claim** naming the client that
   will act — otherwise every exchange is `invalid_request`;
3. something must decide **which capabilities this user may receive**, because
   the exchange by itself will happily widen scope (see the warning below).

## Authentication

The exchange authenticates as the acting **client** (`client_secret_basic` here)
and carries the user's token as `subject_token`. Setting the whole thing up —
clients, scripts, override config — needs only a service-account bearer with
`fr:am:*`.

## Endpoints

| Op | Method | Path | Notes |
| -- | ------ | ---- | ----- |
| Exchange | `POST` | `/am/oauth2{realm-path}/access_token` | The ordinary realm token endpoint. |
| Discovery | `GET` | `/am/oauth2{realm-path}/.well-known/openid-configuration` | **Does not advertise the grant.** See Quirks. |
| Client grant-type enum | `POST` | `…/realm-config/agents/OAuth2Client?_action=schema` | Lists `urn:ietf:params:oauth:grant-type:token-exchange`. |
| Provider exchange config | `GET` | `…/realm-config/services/oauth-oidc` | `coreOAuth2Config.accessTokenMayActScript`, `advancedOAuth2Config.acceptAudienceParametersInTokenExchangeRequests`, `tokenExchangeClasses`. |

Request:

```sh
curl -su "$CLIENT_ID:$CLIENT_SECRET" \
  -d grant_type=urn:ietf:params:oauth:grant-type:token-exchange \
  -d subject_token="$IDENTITY_TOKEN" \
  -d subject_token_type=urn:ietf:params:oauth:token-type:access_token \
  -d requested_token_type=urn:ietf:params:oauth:token-type:access_token \
  -d scope=orders.approve \
  "$TENANT_BASE_URL/am/oauth2/realms/root/realms/bravo/access_token"
```

Response is an ordinary token response plus
`issued_token_type: urn:ietf:params:oauth:token-type:access_token`.

## `may_act` is the gate, and it is stamped by a script

Without it: `400 {"error":"invalid_request","error_description":"Invalid token
exchange."}` — the same message for every misconfiguration, so it tells you
nothing about which one.

`may_act` goes into the **subject** token when that token is issued, by an
`OAUTH2_MAY_ACT[_NEXT_GEN]` script wired to
`overrideOAuth2ClientConfig.accessTokenMayActScript` (or the realm-wide
`coreOAuth2Config.accessTokenMayActScript`). It names the client permitted to
act:

```javascript
// context OAUTH2_MAY_ACT_NEXT_GEN, evaluatorVersion 2.0
token.setMayAct({ client_id: "CapTokenDemo_web" });
```

The issued token then carries `"may_act": {"client_id": "CapTokenDemo_web"}` and
that client's exchange succeeds. A `sub` may be added alongside `client_id` to
pin the actor to one identity (the sandbox's pre-existing
`OAuth2 May Act - Domain Token Exchange` script does exactly that, in Groovy).

**The same JS body on the legacy `OAUTH2_MAY_ACT` context fails** with
`500 … "Error running may_act script"` — legacy Rhino does not coerce a JS
object literal into the Java map `setMayAct` wants. Either use the `_NEXT_GEN`
context, or write Groovy with `JsonValue.json(JsonValue.object(...))`.

### `evaluatorVersion` is decided by the context, not by the field

`PUT`ting a script with `"context": "OAUTH2_MAY_ACT"` and
`"evaluatorVersion": "2.0"` returns **201 echoing `"evaluatorVersion": "1.0"`**.
No error, no warning. The `_NEXT_GEN` context ids are what select the v2
evaluator; the field follows. Verified 2026-08-25 — this refines the advice in
[04-scripts.md](04-scripts.md) to always send `evaluatorVersion` explicitly:
sending it is still right, but it will not save you from the wrong context.

## The exchange can WIDEN scope — you must add a gate

With no scope validation in place, a subject token holding only `openid` was
exchanged for `payments.refund`, for a user with no claim to it. AM checks the
requested scope against the **client's** allowed scopes, not against the subject
token's. Verified 2026-08-25:

| subject token scope | requested | issued |
| ------------------- | --------- | ------ |
| `openid orders.read` | `payments.refund` | `payments.refund` |
| `openid orders.read` | `orders.approve payments.refund` | both |

So "capability token" is theatre until something decides what this user may
hold. Two candidate gates; only the second works on this path.

### `usePolicyEngineForScope` — engages, but has no subject (does not work here)

`overrideOAuth2ClientConfig.usePolicyEngineForScope: true` plus
`scopesPolicySet` does take effect on the token-exchange grant: with it on, the exchange issued
`scope: []` instead of what was asked for. But the policy engine is handed an
**unauthenticated** subject — a scope policy with subject `AuthenticatedUsers`
grants nothing, and the same policy with `NOT(AuthenticatedUsers)` grants. So
the resource owner never reaches the policy, and a per-user scope decision
cannot be expressed this way.

It also does **not** gate the `password` grant at all: a scope with no policy
whatsoever (`profile`) was issued normally.

Untested on `authorization_code`, which has a real session and may behave
differently. The `CapTokenDemoScopes` policy set is left in `bravo` for that
retest ([21-am-policies.md](21-am-policies.md), open question 1).

### A validate-scope script — this is the one that works

`OAUTH2_VALIDATE_SCOPE_NEXT_GEN` on
`overrideOAuth2ClientConfig.validateScopeScript` (with
`validateScopePluginType: "SCRIPTED"`). Two facts make it usable:

- **It is invoked on the token-exchange grant** — extending the earlier
  client-credentials-only observation in
  [12-script-bindings-matrix.md](12-script-bindings-matrix.md). Also on
  `password`.
- **AM intersects the return with what was requested**, so the script can narrow
  a request and never widen one. That makes fail-closed easy and fail-open hard.

It is a function-entry-point script (`validateAccessTokenScope()` and friends —
see [12](12-script-bindings-matrix.md)).

#### `identity` is bound but empty; use `requestProperties`

On both the `password` and token-exchange grants, `identity` exists as an object
whose `AMIdentity` is null:

```
InternalError: Cannot invoke "com.sun.identity.idm.AMIdentity.getName()"
because "this.amIdentity" is null
```

The resource owner has to be recovered from the request instead.
`requestProperties.requestParams` carries the grant's own parameters —
`username` on `password`, `subject_token` on token-exchange — and notably does
**not** carry `password` or `client_secret`. `openidm` is bound, so:

```javascript
function resourceOwnerId(params) {
  var grant = params.grant_type[0];
  if (grant === "urn:ietf:params:oauth:grant-type:token-exchange") {
    var parts = String(params.subject_token[0]).split(".");
    return JSON.parse(utils.base64url.decode(parts[1])).sub;   // an IDM uuid
  }
  if (grant === "password") {
    var found = openidm.query("managed/bravo_user",
      { "_queryFilter": 'userName eq "' + params.username[0] + '"' }, ["_id"]);
    return found.result.length ? String(found.result[0]._id) : null;
  }
  return null;
}
```

`requestProperties` also carries `requestHeaders` (including the client's
`Authorization: Basic …`) and `requestUri`. Treat it as sensitive in logs.

The full working script is
`../../../aic-demos/capability-tokens/scripts/am/validate-scope.js`.

## One client per layer

`may_act` names a **client**, and the acting client authenticates the exchange,
so the natural deployment is one OAuth2 client per hop rather than one client
doing everything. Verified 2026-08-25 with a login client and a separate caller
client:

| Client | Grants | Scopes | Lifetime | Role |
| ------ | ------ | ------ | -------- | ---- |
| `CapTokenDemo_web` | `authorization_code`, `password`, `refresh_token` | `openid` | 900s | Logs the user in. Its may-act script stamps `may_act: {client_id: "CapTokenDemo_caller"}`. |
| `CapTokenDemo_caller` | token-exchange only | the capabilities | **60s** | The BFF's outbound identity. Mints capability tokens; carries the scope gate. |

A cross-client exchange succeeds exactly when the subject token's `may_act`
names the acting client. The issued token then has the **caller's** `client_id`,
`aud` and — usefully — its `accessTokenLifetime`: `expires_in` came back as
`59`. There is no per-exchange lifetime parameter, so **a separate short-lived
client is the only way to get a short-lived capability token.**

### The scope gate lives on the ACTING client — this one bites

With the validate-scope script attached to **both** clients, alice's request for
`payments.refund` (a capability she has no claim to) was correctly refused.
Remove it from the *caller* and leave it on the *login* client — the intuitive
place, since that is "the BFF's client" — and the same request **succeeds**:

```
both clients have the gate:  payments.refund -> null
gate on the login client only:
  payments.refund -> payments.refund      <- alice has no claim to this
```

The login client's configuration has no bearing on an exchange it is not
performing. Attach the gate (and the access-token-modification script) to every
client that can mint, and treat a mint-capable client with
`validateScopePluginType: "PROVIDER"` as a hole.

### The issued token carries no delegation trail

The exchanged token had **`act: null`** and **`may_act: null`**. AM does not
record the actor in the token it issues, so there is nothing in the capability
token saying which client minted it — if you need that, the acting client's own
access-token-modification script has to write it.

`may_act: null` is a useful default, though: **a capability token cannot be
exchanged onward** unless the acting client also runs a may-act script. Chaining
to a third layer is therefore opt-in, per client, and the `token.setAct()` /
`setMayAct()` pair on the may-act binding is the hook for it.

## Getting attributes into the token: the modification script

`OAUTH2_ACCESS_TOKEN_MODIFICATION_NEXT_GEN` on
`accessTokenModificationScript` (+ `accessTokenModificationPluginType:
"SCRIPTED"`) adds claims. The subject is available here even though `identity`
is not:

```javascript
var sub = String(accessToken.getResourceOwnerId());   // IDM uuid
accessToken.setField("demoRoles", roles);             // array claim
```

`accessToken.getSubject()` **does not exist** — calling it fails the whole token
request with `Error running access token modification plugin` and no further
detail. The binding's real method list is on
`GET /am/json{realm-path}/contexts/OAUTH2_ACCESS_TOKEN_MODIFICATION_NEXT_GEN`;
`getResourceOwnerId`, `getClientId`, `getScope`, `getClaims`, `getGrantType`,
`setField`, `setFields` are the useful ones.

An array claim set this way survives the exchange: the capability token carried
`demoRoles` too, which is what lets an AM policy key on it
([21-am-policies.md](21-am-policies.md)).

## Quirks

### Discovery does not advertise the grant

`.well-known/openid-configuration` lists ten grant types and
`urn:ietf:params:oauth:grant-type:token-exchange` is **not** among them, on a
tenant where the exchange demonstrably works. The provider's
`tokenExchangeClasses` are configured and the grant is in the OAuth2 client
schema's `grantTypes` enum. Treat discovery as incomplete here; it is enabled
per client, and the client config is the truth.

### `audience` and `resource` are accepted and ignored

`acceptAudienceParametersInTokenExchangeRequests` is `false` by default. With it
off, passing `audience=shop-api` or `resource=https://…` produces **no error** —
the exchange succeeds and the parameter has no effect, `aud` still being the
acting client. If a demo or design depends on audience-restricted tokens, check
the flag rather than the response.

Usefully, the flag exists on `overrideOAuth2ClientConfig` as well as the realm
service, so it can be turned on for one client without a realm-wide change.

### The issued token's lifetime is the acting client's

There is no per-exchange lifetime parameter — see
[One client per layer](#one-client-per-layer), where a 60s caller client issues
a capability token with `expires_in: 59` from a 900s identity token.

## Verified against

- Sandbox tenant, realm **`bravo`**, **2026-08-25**, service-account bearer.
- Clients `CapTokenDemo_web` (login: `password`, `authorization_code`,
  `refresh_token`; 900s) and `CapTokenDemo_caller` (token-exchange only; 60s),
  both confidential `client_secret_basic` with overrides enabled and
  `statelessTokensEnabled: true`. The login client carries the may-act script;
  both carry the validate-scope and access-token-modification scripts.
- Scripts created in `bravo` and left in place:
  `CapTokenDemo_MayAct_NextGen` (`…aa02`), `CapTokenDemo_ValidateScope`
  (`…aa03`), `CapTokenDemo_TokenModification` (`…aa04`).
  `CapTokenDemo_MayAct` (`…aa01`, legacy context) is the failed 1.0 attempt,
  kept as the counter-example.
- Users `alice@captoken.demo` (roles `orders.reader`, `orders.approver`) and
  `bob@captoken.demo` (`orders.reader`) in `managed/bravo_user`.
- Exercised: exchange without `may_act` (fails), with it (succeeds), scope
  widening, `audience`/`resource` parameters, `usePolicyEngineForScope` on and
  off, validate-scope on `password` and token-exchange, token modification,
  same-client and cross-client exchange, and the scope gate attached to the
  acting client versus the login client.

## Source citations

None. First-hand observation only.

## Open questions

1. **`authorization_code` + `usePolicyEngineForScope`** — does a real session
   give the policy engine a subject? This is the difference between a
   declarative scope gate and a scripted one.
2. **`requested_token_type: …:id_token`** and the id-token exchangers in
   `tokenExchangeClasses` are unexercised.
3. **Refresh tokens on exchanged tokens — mostly answered.** The single-client
   exchange returned a refresh token with a 604800s life beside a 900s access
   token, which for a capability token would be alarming: a refreshable 60s
   capability is a 7-day one. The per-layer caller client returns **no refresh
   token**, because `refresh_token` is not in its `grantTypes`. So omitting the
   grant is the fix, and it falls out of the one-client-per-layer shape for
   free. Whether a refresh token issued *with* the grant can outlive the subject
   token is still untested.
4. **`tokenExchangeAuthLevel`** on `advancedOAuth2ClientConfig` — untouched;
   presumably raises the bar on the subject token's `auth_level`.
5. **`oidcMayActScript`** — the id-token twin of the access-token may-act hook,
   untested.
