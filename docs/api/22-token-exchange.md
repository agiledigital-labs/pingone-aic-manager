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
| Client grant-type enum | `POST` | `…/realm-config/agents/OAuth2Client?_action=schema` | Lists `urn:ietf:params:oauth:grant-type:token-exchange`. Also where `advancedOAuth2ClientConfig.allowedResourceServerAudienceValues` and the client's own `acceptAudienceParametersInTokenExchangeRequests` live. |
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

### The token endpoint does verify the subject token

Worth stating because its neighbour does not (see
[21-am-policies.md](21-am-policies.md): `?_action=evaluate` checks neither
signature nor expiry on a `jwt` subject). Take a valid identity token, rewrite
a claim, leave the signature in place, and present it as `subject_token`:

```
genuine bob identity token  -> scope: null      (policy said no, correctly)
FORGED, demoRoles rewritten -> invalid_request  "Invalid token exchange."
```

The exchange is a token-endpoint operation and validates the subject token
before anything downstream sees it. So a validate-scope script may treat the
claims on `subject_token` as trustworthy — which is what makes a mint-time gate
keyed on a roles claim sound, rather than a decoration a caller can rewrite.

The contrast is the thing to remember: **the token endpoint authenticates, the
policy endpoint does not.**

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

That last clause is now observed rather than inferred: a 2026-08-27 probe with
the may-act script attached to the **acting** client got an exchanged token
carrying `may_act` itself. So `act: null` is AM's behaviour, but `may_act: null`
is a property of *where the script hangs* — put it on a mint-capable client and
every token it issues is onward-exchangeable.

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

## Setting the `aud` claim — the audience whitelist

`aud` is the acting client's id, and on an **ordinary grant there is no way to
change it** — no request parameter, no configuration field. Verified
2026-08-27 with a `client_credentials` client carrying a populated whitelist:
`audience=`, `resource=` and even `aud=` were all accepted and had no effect,
with `acceptAudienceParametersInTokenExchangeRequests` both off *and* on.

Token exchange is the one place AM will put a caller-chosen value in `aud`, and
it does it from a **per-client whitelist**, not from a script:

| Where | Field | Effect |
| ----- | ----- | ------ |
| Client, `advancedOAuth2ClientConfig` | `allowedResourceServerAudienceValues` | The whitelist. Exact strings; template default is `[""]`, stored as `[]`. |
| Client, `overrideOAuth2ClientConfig` — or provider `advancedOAuth2Config` | `acceptAudienceParametersInTokenExchangeRequests` | The switch. Default **false**. |

With the switch on, `audience=<value>` on the exchange request is validated
against the list and **appended** to `aud`:

| Request | Result |
| ------- | ------ |
| `audience=https://api.example.com` | `aud: ["<client_id>", "https://api.example.com"]` |
| the parameter twice, both allowed | both appended, in request order |
| `audience=https://not-allowed.example.com` | `400 invalid_request` — `"Invalid audience requested."` |
| `audience=https://api.example.com/orders` | `400` — matching is exact, no prefix or wildcard |
| `audience=` (empty) | `400` — omit the parameter instead of sending it empty |
| no `audience` parameter | `aud: "<client_id>"` — a **string** |
| `resource=https://api.example.com` | ignored, switch on or off |

Two shapes to settle with the resource server before it validates this:

- **`aud` changes type.** A bare string with no audience requested, an array
  when one is. An RS comparing `aud === "my-api"` breaks on the array; it has
  to test membership.
- **the client id stays, as the first element.** Ping documents that as a
  consequence of the provider's `includeClientIdClaimInStatelessTokens`
  (default `true`) — which exists **only realm-wide**, with no client override
  in the OAuth2Client schema. So `aud` holding nothing but the resource server
  is not reachable per client; that is the one thing here still needing an
  access-token-modification script, or a realm-wide change.

### Both the switch and the whitelist are read from the ACTING client

Same trap as [the scope gate](#the-scope-gate-lives-on-the-acting-client--this-one-bites),
and worth proving the same way — with two clients whose configuration
deliberately disagrees:

| Acting client | Subject's client | `audience=` requested | Result |
| ------------- | ---------------- | --------------------- | ------ |
| list `[api, second]`, switch on | list `[api]` | `second` | **appended** — the subject's list does not narrow |
| list `[api, second]`, switch on | list `[third]` | `third` | **`400` Invalid audience requested** — the subject's list does not widen |
| list `[api, second]`, switch **off** | list `[api]`, switch **on** | `api` | **ignored**, `aud` a plain string — the subject's switch does not enable |

So the whitelist belongs on every client that can mint, exactly like the scope
gate, and a mint-capable client with an empty list simply cannot be asked for
an audience.

## Quirks

### Discovery does not advertise the grant

`.well-known/openid-configuration` lists ten grant types and
`urn:ietf:params:oauth:grant-type:token-exchange` is **not** among them, on a
tenant where the exchange demonstrably works. The provider's
`tokenExchangeClasses` are configured and the grant is in the OAuth2 client
schema's `grantTypes` enum. Treat discovery as incomplete here; it is enabled
per client, and the client config is the truth.

### `audience` and `resource` are accepted and ignored by DEFAULT

`acceptAudienceParametersInTokenExchangeRequests` is `false` by default. With it
off, passing `audience=shop-api` or `resource=https://…` produces **no error** —
the exchange succeeds and the parameter has no effect, `aud` still being the
acting client. A design that depends on audience-restricted tokens therefore
fails **silently**: check the flag, not the response.

Usefully, the flag exists on `overrideOAuth2ClientConfig` as well as the realm
service, so it can be turned on for one client without a realm-wide change —
and it is the acting client's copy that counts. `resource` is ignored even with
the flag on: there is no RFC 8707 resource-indicator support in this schema.
See [Setting the `aud` claim](#setting-the-aud-claim--the-audience-whitelist).

### `requested_token_type` is the one extra parameter AM rejects

Everything else the RFC and `draft-ietf-oauth-transaction-tokens-11` add to the
request is accepted. `requested_token_type` is not, unless it names a token type
AM actually issues — and the failure is the same opaque
`invalid_request: Invalid token exchange.` as every other misconfiguration.

Bisected 2026-08-28 against a throwaway exchange pair in `bravo`, one parameter
at a time, with a working vanilla exchange as the control:

| Added to a working exchange | Result |
| --- | --- |
| *(control — nothing extra)* | 200 |
| `requested_token_type=…:token-type:access_token` | 200 |
| `requested_token_type=…:token-type:txn_token` | **`invalid_request`** |
| `audience=acme-internal` | 200 (and ignored — see above) |
| `request_details={…}` and `request_context={…}` | 200 |

So a transaction-token-shaped exchange can carry the draft's own parameters, but
must ask for an access token. The response then says
`issued_token_type: …:token-type:access_token` and `token_type: "Bearer"`,
neither of which is settable.

### Arbitrary request parameters reach the token-modification script

`request_details` and `request_context` — or anything else you post — arrive in
the access-token-modification script as
`requestProperties.requestParams.<name>`, each one a **single-element array of
strings**, not the parsed JSON:

```javascript
var raw = requestProperties.requestParams.request_details; // ['{"cost_cents":45000}']
var proposed = JSON.parse(String(raw[0]));
```

On an exchange the full parameter set the script sees is `grant_type`, `scope`,
`subject_token`, `subject_token_type`, plus whatever you added.

### `setField("aud", …)` beats the audience whitelist

The `audience` request parameter needs
`acceptAudienceParametersInTokenExchangeRequests` and a whitelist, and even then
only accepts registered values. An access-token-modification script can simply
overwrite the claim:

```javascript
accessToken.setField("aud", "acme-internal");
```

Verified 2026-08-28: the emitted JWT carried `"aud": "acme-internal"` with the
realm flag off and no whitelist entry. That is the shortest path to an `aud`
that is a trust domain rather than a client id — at the cost of the audience no
longer being anything AM itself will enforce.

### The JWT header is not scriptable

`typ` is always `"JWT"`. The context that would change it,
`OAUTH2_SCRIPTED_JWT_ISSUER[_NEXT_GEN]`, has schema metadata but **no
configuration hook anywhere in AIC** — see
[`12-script-bindings-matrix.md`](12-script-bindings-matrix.md). A profile that
requires its own `typ` (`txntoken+jwt`, `at+jwt`) cannot be met by AM-issued
tokens.

### The issued token's lifetime is the acting client's

There is no per-exchange lifetime parameter — see
[One client per layer](#one-client-per-layer), where a 60s caller client issues
a capability token with `expires_in: 59` from a 900s identity token.

## Verified against

- Date: 2026-08-28 — realm `bravo`, throwaway `ZZProbe_sub` / `ZZProbe_atm`
  client pair plus two throwaway scripts, all deleted afterwards.
- Calls: one exchange per row of the `requested_token_type` table above, each
  against the same working control, so the rejection is attributable to that
  one parameter. Plus an access-token-modification script that dumped
  `requestProperties.requestParams` on the exchange, set `aud` by `setField`
  with the realm audience flag off, and emitted a nested `tctx` object with a
  boxed integer. `capability-tokens`' `chain.sh` was re-run afterwards to
  confirm the realm was left as it was found.

- Sandbox tenant, realms **`alpha`** and **`bravo`**, **2026-08-27**,
  service-account bearer. AM reports `9.0.0-SNAPSHOT` (build 2026-August-14).
  Subject of the run: how `aud` is set without an access-token-modification
  script. Calls: `_action=schema` on both `…/agents/OAuth2Client` and
  `…/services/oauth-oidc` — the only audience-shaped fields in either are
  `allowedResourceServerAudienceValues`,
  `acceptAudienceParametersInTokenExchangeRequests` and the provider's
  inbound-only `allowedAudienceValues`. Throwaway confidential clients
  `AudienceProbe_20260827` (alpha and bravo) and `AudienceProbeSubject_20260827`
  (bravo), plus a throwaway `OAUTH2_MAY_ACT_NEXT_GEN` script, all four created
  by `PUT` and **deleted**, each confirmed `404` afterwards. Ordinary
  `client_credentials` mints with `audience=`/`resource=`/`aud=`, flag off and
  on, all left `aud` as the client_id string. **Alpha cannot exchange at all** —
  its provider `advancedOAuth2Config.grantTypes` omits token-exchange while
  bravo's includes it, which surfaces as `unsupported_grant_type` and reads like
  a client misconfiguration. Exchange runs in bravo covered: flag off (audience
  ignored), flag on with an allowed value (appended, `aud` becomes an array),
  two allowed values, a value absent from the list, a path-suffixed value, an
  empty value, `resource=`, and the three acting-versus-subject-client
  permutations in the table above. The
  `includeClientIdClaimInStatelessTokens` claim in that section is **Ping's
  documentation, not a local measurement** — see open question 6.
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
6. **Can `aud` hold only the resource server?** Ping's docs tie the leading
   client-id audience value to the provider's
   `includeClientIdClaimInStatelessTokens`, which is realm-wide with no client
   override. Untested here deliberately: flipping it rewrites every stateless
   token in the realm, so it wants its own run rather than a side effect of an
   audience probe.
7. **Does an exchanged token's `aud` survive introspection?** The probes decoded
   the JWT locally; `/oauth2/introspect` was not exercised, and its
   audience-members-may-introspect behaviour keys on `aud` membership.
