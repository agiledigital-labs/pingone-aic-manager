# 17 — JWT bearer user tokens (Trusted JWT Issuer)

Implemented in: [`src/jwtbearer/`](../../src/jwtbearer/) (`aic jwt-bearer setup`
and `aic jwt-bearer issuer`).

## Purpose

Mint an OAuth2 access token **on behalf of an end user** without a journey, a
password, or a browser. A locally-held RSA key signs an assertion naming the
user as `sub`; AM verifies it against a realm-level **Trusted JWT Issuer**
config that holds one public JWK per person/machine, and issues a user token.

This is the tool for "I need to reproduce what the customer's app sees" and for
agent-driven testing. It is deliberately a **lower-environment** capability: an
issuer with a blank `allowedSubjects` can mint a token for **any user in the
realm**.

Not to be confused with two neighbours:

- **[00-auth.md](00-auth.md)** — the same RFC 7523 grant, but the subject is a
  _service account_ and the client is the fixed string `service-account`. Root
  realm, no Trusted JWT Issuer involved.
- **Client `jwkSet`** (`signEncOAuth2ClientConfig`, [05](05-oauth2-oidc.md)) —
  `private_key_jwt` _client authentication_. Lets you authenticate **as a
  client** without holding its secret. Useful (an agent can drive an existing
  `client_credentials` client without being handed the production secret) but
  orthogonal: it does not produce a user token.

## Authentication

**Service-account bearer, scope `fr:am:*` — verified sufficient for the whole
setup.** No admin-user bearer, no console step. The SA can create, update, read
and delete `TrustedJwtIssuer` agents and add the grant type to an OAuth2 client.

The minted user token's own scopes are whatever the client allows; a token with
`fr:idm:*` acts as that user against IDM (verified — see Quirks).

## Endpoints

`{realm-path}` = `/realms/root/realms/alpha` (or `bravo`). All the
`realm-config` calls need `Accept-API-Version: protocol=2.1,resource=1.0`.

| Op              | Method   | Path                                                                          | Notes                                                          |
| --------------- | -------- | ----------------------------------------------------------------------------- | -------------------------------------------------------------- |
| List issuers    | `GET`    | `/am/json{realm-path}/realm-config/agents/TrustedJwtIssuer?_queryFilter=true` | `{result:[…]}`. Empty on a stock tenant.                       |
| Read issuer     | `GET`    | `/am/json{realm-path}/realm-config/agents/TrustedJwtIssuer/{id}`              | 404 if absent.                                                 |
| Create / update | `PUT`    | `/am/json{realm-path}/realm-config/agents/TrustedJwtIssuer/{id}`              | Plain `PUT`, no `If-Match`. 201 on create, 200 on update.      |
| Delete          | `DELETE` | `/am/json{realm-path}/realm-config/agents/TrustedJwtIssuer/{id}`              | 200; a follow-up `GET` 404s.                                   |
| Default object  | `POST`   | `…/agents/TrustedJwtIssuer?_action=template`                                  | Body `{}`. Returns the unset object — good defaults source.    |
| Field schema    | `POST`   | `…/agents/TrustedJwtIssuer?_action=schema`                                    | Body `{}`. Titles/descriptions for every field.                |
| Discovery       | `GET`    | `/am/oauth2{realm-path}/.well-known/openid-configuration`                     | **Read `issuer` from here — do not construct it.** See Quirks. |
| Mint user token | `POST`   | `/am/oauth2{realm-path}/access_token`                                         | Realm-scoped, unlike the service-account endpoint.             |

## Object shape

`POST …?_action=template` returns exactly this (verified 2026-08-06):

```json
{
  "allowedSubjects": [],
  "jwksCacheTimeout": 3600000,
  "jwkSet": null,
  "issuer": null,
  "consentedScopesClaim": "scope",
  "jwkStoreCacheMissCacheTime": 60000,
  "agentgroup": null,
  "jwksUri": null,
  "resourceOwnerIdentityClaim": "sub"
}
```

What the fields do (from `_action=schema`, confirmed by probe):

| Field                        | Meaning                                                                                                          |
| ---------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `issuer`                     | The exact `iss` claim this config matches. Mismatch → `invalid_grant` "Unknown JWT issuer".                      |
| `jwkSet`                     | **A string** containing JWKS JSON — `{"keys":[…]}`. This is where the per-agent public keys live.                |
| `jwksUri` / `jwks*Cache*`    | Alternative to `jwkSet`: fetch keys from a URL. Not used by us — no URL to host.                                 |
| `allowedSubjects`            | Users this issuer may consent for. **Blank means every user in the realm.**                                      |
| `consentedScopesClaim`       | Claim naming the scopes the user consented to. Default `scope`. Acts as a **ceiling**, not a grant — see Quirks. |
| `resourceOwnerIdentityClaim` | Which claim identifies the user. Default `sub`.                                                                  |

Writes take plain values; reads come back wrapped as
`{"inherited": false, "value": …}` — the same pattern as OAuth2 clients
([05](05-oauth2-oidc.md)). As with clients, do not send `_id` / `_rev` / `_type`
back in a `PUT` body.

### Per-client prerequisite

The OAuth2 client used in the exchange needs
`urn:ietf:params:oauth:grant-type:jwt-bearer` in
`advancedOAuth2ClientConfig.grantTypes`. **That is the only per-client change.**
On the sandbox, no existing client had it (2026-08-06: 44 clients in alpha, all
`client_credentials` / `authorization_code` / `password`).

The realm-wide provider service already permits the grant — alpha's
`advancedOAuth2Config.grantTypes` includes it out of the box, and so does the
discovery doc's `grant_types_supported`. No provider edit needed.

## The assertion

Header — `kid` selects the key within `jwkSet`:

```json
{ "alg": "RS256", "typ": "JWT", "kid": "aic:someone@example.com:2026-08-06" }
```

Claims:

```json
{
  "iss": "aic-agent", // must equal the issuer config's `issuer`
  "sub": "<alpha_user _id (UUID)>", // NOT the username — see Quirks
  "aud": "https://<tenant>:443/am/oauth2/realms/root/realms/alpha",
  "exp": 1786000000, // now + ≤180s
  "iat": 1785999820,
  "jti": "<uuid>"
}
```

Exchange (`application/x-www-form-urlencoded`) against the **realm** token
endpoint:

```
client_id=<client>
client_secret=<secret>          # required unless the client uses private_key_jwt
grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer
assertion=<signed JWT>
scope=openid profile fr:idm:*
```

Response is a normal token response: `access_token`, `id_token` (when `openid`
is in scope), `scope`, `token_type`, and `expires_in: 3599` — **one hour**, not
the service account's 898 seconds.

## Consent — how it actually works

This was the open design question, and the answer is simpler than expected: **a
Trusted JWT Issuer _is_ the consent.** The schema calls `allowedSubjects` "list
of subjects which this provider is allowed to provide consent for". There is no
consent record to write and nothing to grant per user.

Verified against a client with **`isConsentImplied: false`**: the exchange
succeeded with a `scope` claim in the assertion _and_ without one. So:

- `isConsentImplied` on the client is **irrelevant** to this grant. No need to
  set it, and no need to warn when it's off.
- `consentedScopesClaim` is an optional **narrowing** control, not the thing
  that makes consent happen. Requesting `scope=openid profile` while the
  assertion claims `"scope": "openid"` yields `"scope": "openid"` — the claim
  caps the grant. Omitting the claim grants the requested scopes in full.

The security boundary is therefore `allowedSubjects` alone. Setting it to a
specific user's UUID hard-limits the issuer to that user; leaving it blank makes
the private key equivalent to "log in as anyone in this realm". Verified: with
`allowedSubjects` naming one user, a different `sub` fails with `invalid_grant`
"Issuer is not authorized to grant consent for this subject".

## Quirks

- **`aud` must include `:443`.** The realm's OIDC issuer is
  `https://<tenant>:443/am/oauth2/realms/root/realms/alpha` — with the explicit
  port. Constructing the audience from `TENANT_BASE_URL` (no port) fails with
  `invalid_grant` "incorrect audience in JWT", which reads like a path problem
  and is not. **Read `issuer` from the discovery document.** Either the
  discovery `issuer` or the discovery `token_endpoint` is accepted as `aud`; the
  root-realm SA endpoint is not.
- **`sub` must be the UUID, not the username.** Both mint a token — AM resolves
  either — but a token minted with `sub: "testuser"` carries `sub: "testuser"`
  verbatim, and **IDM then refuses it**: `GET /openidm/managed/alpha_user/{id}`
  returns `500 "Access Denied"` because IDM matches `sub` against the managed
  object `_id`. The UUID form returns 200 on the user's own record. `userinfo`
  works for both, so a username-based token looks fine until it touches IDM.
  Resolve username → `_id` before signing.
- **A nonexistent subject is rejected at mint time** — `invalid_grant` "Not able
  to read user information." So there's no silent phantom-user token.
- **Non-standard JWK members survive.** A JWK carrying `aic_owner`,
  `aic_created` and `aic_host` alongside `kty`/`n`/`e`/`kid` round-tripped
  through `PUT`/`GET` byte-for-byte and verified normally. Ownership metadata
  does **not** have to be smuggled into the `kid`. Verified 2026-08-06.
- **`kid` tolerates `:` and `@`.**
  `aic:dsbalmain@agiledigital.com.au:2026-08-06` worked as a key id.
- **`kid` is optional but authoritative.** With no `kid` header AM tries the
  keys in the set and succeeds. With a `kid` that names a _different_ key than
  the one that signed, it fails with "JWT signature is invalid" rather than
  falling back.
- **Multiple keys work.** Two independent key pairs in one `jwkSet`, each with
  its own `kid`, both minted tokens. This is what makes one shared issuer config
  viable for a team.
- **One issuer serves every client in the realm.** The config has no client
  binding; the same issuer + key minted tokens through two different clients. So
  it's one issuer per **realm** (per tenant), not per client.
- **Per realm, and not in root.** `alpha` and `bravo` each have their own
  `agents/TrustedJwtIssuer` collection (both 200). The **root realm returns
  403** — consistent with root being locked down on AIC.
- **Client authentication is still required.** Omitting `client_secret` on a
  `client_secret_post` client fails with `invalid_client` "Client authentication
  failed". The assertion authenticates the _user_, not the client.

## Current implementation

The `src/jwtbearer/` vertical implements setup and named-issuer creation/show
commands. Setup creates or reuses one RSA key pair per local install, stores the
private record in the agent vault, merges the public JWK into the realm's shared
`jwkSet` by `kid`, and verifies that AM retained the key after the write.
Issuer writes explicitly set `allowedSubjects: []`,
`consentedScopesClaim: "scope"`, and `resourceOwnerIdentityClaim: "sub"` so
they do not depend on template defaults.

- The private half is stored under the tenant name in the agent vault, while
  setup adds the public half to the realm's shared `jwkSet`. The read-modify-
  write merges by `kid` rather than replacing the set, so concurrent operators
  do not clobber each other's keys.
- **`kid` is an opaque random string** (decided 2026-08-06), not a composed
  `owner:host` label. Attribution rides in the non-standard JWK members instead
  — verified above to survive the round trip byte-for-byte — which is what makes
  the opaque form viable. Reasons: a composed id would put a person's email in a
  tenant config object readable by any `fr:am:*` holder and carried along when
  config is promoted between environments; it needs a sanitiser for characters
  never tested on the wire; and it can collide, which matters because `kid`
  _selects_ the verification key, so a repeat setup on the same host would
  silently overwrite rather than rotate.
- The `aic_owner`, `aic_host`, and `aic_created` members are load-bearing: they
  are the tenant-side attribution record, and the local record stores the same
  opaque `kid` used to identify that key.
- **`allowedSubjects` stays empty by default** (decided 2026-08-06). Blank means
  the issuer may mint for any user in the realm, which is the point: this exists
  so a tester can become an arbitrary user without a journey or a password.
  Requiring an explicit subject list would defeat the convenience the feature is
  for.
The setup command refuses production-themed tenants because an empty
`allowedSubjects` plus a client with the grant enabled is a realm-wide
capability. Minting a user token from the stored key, exporting a public JWKS,
and key rotation/removal remain future work; export must be an explicit command,
not a setup side effect.

## Planned shape

- aic auth --as-id <uuid> --client-id <id> or
  aic auth --as-username <name> --client-id <id> now resolves the subject,
  signs with the stored tenant key, exchanges at the tenant-relative realm
  token path, and supports repeatable --scope, --realm, --tenant,
  --client-secret-stdin, and bare-token --token output. Discovery's `issuer`
  remains the source of the audience claim; its `token_endpoint` is not used
  as an outbound URL.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`, realm `alpha`
- Date: 2026-08-06
- Auth: service-account bearer only (`fr:am:*` + `fr:idm:*`), no admin session.
- Calls: `GET agents/TrustedJwtIssuer?_queryFilter=true` (200, empty) on alpha
  and bravo, 403 on root; `POST …?_action=template` and `?_action=schema` (200);
  `PUT agents/TrustedJwtIssuer/test_aic_agent_issuer` (201 create, 200 update
  adding a second key and blanking `allowedSubjects`);
  `PUT agents/OAuth2Client/test_jwtbearer_probe` and `…probe2` (201 each, both
  with `isConsentImplied: false`); `GET .well-known/openid-configuration` (200);
  ~20 × `POST /am/oauth2/realms/root/realms/alpha/access_token` covering the
  audience candidates, present/absent/narrowed `scope` claim, UUID vs username
  vs nonexistent `sub`, allowed vs disallowed subject, wrong `iss`, missing
  `client_secret`, both key ids, absent `kid`, mismatched `kid`, and both
  clients; `GET userinfo` and `GET /openidm/managed/alpha_user/{id}` with the
  minted user tokens. Both probe clients and the probe issuer were `DELETE`d and
  confirmed 404. Local key material was shredded.
- Subject used: the pre-existing sandbox `testuser` (`22bc823c-…`). No user was
  created or modified.

## Source citations

- Ping docs:
  <https://docs.pingidentity.com/pingoneaic/latest/am-oauth2/oauth2-jwt-bearer.html>
  (grant overview). Field semantics here come from the tenant's own
  `_action=schema`, which is more reliable.
- frodo-lib: `src/api/AgentApi.ts` (generic `realm-config/agents/{type}` CRUD).
- Not covered by fr-config-manager.

## Open questions

- `resourceOwnerIdentityClaim` was left at `sub` throughout. Pointing it at a
  custom claim might let a username-keyed assertion resolve properly, which
  would remove the lookup — untested.
- `jwksUri` untested (we have nowhere to host a JWKS).
- Whether an issuer can be restricted to specific clients. Nothing in the
  `TrustedJwtIssuer` schema suggests it, and the probe showed cross-client
  reuse. **`advancedOAuth2ClientConfig.acceptedJwtIssuers` is not it** — its
  schema description (read 2026-08-06) is "List of JWT issuers that will be
  accepted in addition to client_id for **private key JWT authentication**", so
  it governs client auth, not the authorization grant. Don't chase it. If a
  per-client restriction exists it is somewhere else, and the working assumption
  should be that there is none: any client in the realm with the grant type
  enabled can be driven by any trusted issuer in that realm.
- Refresh tokens: `refresh_token` was never requested alongside `jwt-bearer`, so
  whether the grant issues one is unknown. The 1-hour access token made it moot.
