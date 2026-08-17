# 17 — JWT bearer user tokens (Trusted JWT Issuer)

Implemented in: [`src/jwtbearer/`](../../src/jwtbearer/) (`aic jwt-bearer setup`
and `aic jwt-bearer issuer`).

## Purpose

Mint an OAuth2 access token **on behalf of an end user** without a journey, a
password, or a browser. A locally-held RSA key signs an assertion naming the
user as `sub`; AM verifies it against a realm-level **Trusted JWT Issuer**
config that holds one public JWK per person/machine, and issues a user token.

This is the tool for "I need to reproduce what the customer's app sees" and for
agent-driven testing. An issuer with a blank `allowedSubjects` can mint a token
for **any user in the realm**, so in lower environments — where that blank list
is the intended default — the whole capability is one.

On a **production**-themed tenant the blank case is what is forbidden, not the
capability: `aic` refuses to leave an issuer unrestricted there, and refuses to
mint against one that already is. See "Production" below.

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
`advancedOAuth2ClientConfig.grantTypes`. Add it to an existing client with
`aic oauth grant add <client-id> urn:ietf:params:oauth:grant-type:jwt-bearer`.
**That is the only per-client change.** On the sandbox, no existing client had
it (2026-08-06: 44 clients in alpha, all `client_credentials` /
`authorization_code` / `password`).

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
grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer
assertion=<signed JWT>
scope=openid profile fr:idm:*
```

Client authentication defaults to `client_secret_post`: `aic auth` adds
`client_secret=<secret>` to the form and sends no `Authorization` header. Use
`--client-auth client-secret-basic` for a client whose `tokenEndpointAuthMethod`
is `client_secret_basic` — AM's own template default — and it sends
`Authorization: Basic base64(form_encode(client_id):form_encode(secret))`
without duplicating the secret in the body. Omitting `--client-secret-stdin`
sends neither credential under either method, for a public client. The flag is
enum-valued so `private-key-jwt` can be added later; it is not implemented yet.

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

### The exact restriction rule (verified 2026-08-14)

Code that gates on this field must use the rule AM actually implements, not the
obvious one. **The issuer is restricted if and only if the list contains at
least one entry that is non-empty after trimming.** Everything else about the
field follows from that plus literal string matching against the assertion's
`sub` claim:

| `allowedSubjects` | Mint for a listed user | Mint for an unlisted user | Reading                                   |
| ----------------- | ---------------------- | ------------------------- | ----------------------------------------- |
| `[]`              | ✓                      | ✓                         | unrestricted — the documented blank case  |
| `[""]`            | ✓                      | ✓                         | **also unrestricted** — see below         |
| `["   "]`         | ✗                      | ✗                         | a literal subject nobody has; not trimmed |
| `["<uuid>"]`      | ✓                      | ✗                         | restricted                                |
| `["", "<uuid>"]`  | ✓                      | ✗                         | restricted; the blank does not weaken it  |
| `["*"]`           | ✗                      | ✗                         | **not** a wildcard                        |
| `["<username>"]`  | ✗                      | ✗                         | matching is literal — see below           |

Two of these rows are traps:

- **`[""]` is indistinguishable from `[]` at runtime but not to a naive check.**
  A gate written as `!allowed_subjects.is_empty()` passes a list holding one
  empty string and reports the issuer as restricted while it is minting for
  every user in the realm. Trim and discard empty entries before deciding.
- **Matching is against the raw `sub` claim, so a username never matches.**
  `allowedSubjects: ["user.0"]` refused a mint for user.0's own UUID. AM
  resolves either form in `sub` (see Quirks) but does _not_ resolve before
  comparing against this list, and `aic auth --as-username` signs the UUID. A
  username here is a lockout, not a restriction — resolve to `_id` first.

Enforcement is **immediate**: a mint issued straight after the write was
refused, with no settle time. Whatever caches `jwkSet` (see Open questions) does
not cache this field, so narrowing a list takes effect at once.

## Quirks

- **`allowedSubjects: [""]` is not a restriction.** A list holding only empty
  strings behaves exactly like a blank list and mints for any user, so any code
  gating on this field must trim and discard empty entries before testing it —
  see "The exact restriction rule" above for the full table.
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
- **A missing grant and a bad secret are indistinguishable.** Both fail with
  `invalid_client` "Invalid authentication method for accessing this endpoint."
  The error mapper names both checks and preserves AM's own `error_description`.
  Note this differs from the _omitted_ `client_secret` case above, which says
  "Client authentication failed".
- **AM appears to accept either client-auth method regardless of the client's
  `tokenEndpointAuthMethod`.** Measured 2026-08-07 on two settled clients
  differing only in that field, against the same published key: **all four**
  method × client combinations minted. So a "mismatch" is not known to fail, and
  `--client-auth` exists for explicitness and for future methods rather than to
  work around an AM restriction. `aic auth` defaults to `client-secret-post` and
  `aic oauth create` writes the matching value, so the pair agrees without a
  flag; that deviates from AM's template default (`client_secret_basic`) on
  purpose.

  **Two earlier readings on this same day are retracted.** A run where a
  default-Basic client was refused while a post-seeded client minted was taken
  as proof that the method must match — it is not reproducible, and it was made
  while the realm's JWKS cache was in a known-stale state, so elapsed time
  cannot be separated from the auth method. A separate agent-run 2×2 reporting
  both crossed pairings failing did not reproduce either. Treat the auth method
  as _not_ established to be load-bearing until someone probes it on a realm
  with a known-good cache.

## Current implementation

The `src/jwtbearer/` vertical implements setup, named-issuer creation/show,
default-issuer key listing/removal/rotation, and local key transfer commands.
Setup creates or reuses one RSA key pair per local install, stores the private
record in the agent vault, merges the public JWK into the realm's shared
`jwkSet` by `kid`, and verifies that AM retained the key after the write. Issuer
writes explicitly set `allowedSubjects: []`, `consentedScopesClaim: "scope"`,
and `resourceOwnerIdentityClaim: "sub"` so they do not depend on template
defaults.

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
- **`allowedSubjects` stays empty by default outside production** (decided
  2026-08-06). Blank means the issuer may mint for any user in the realm, which
  is the point: this exists so a tester can become an arbitrary user without a
  journey or a password. Requiring an explicit subject list would defeat the
  convenience the feature is for. Minting a user token from the stored key and
  exporting a public JWKS remain separate explicit commands; neither is a setup
  side effect.
- **`allowedSubjects` is preserved by every write** (changed 2026-08-14). It
  used to be forced to `[]` on each `PUT`, which meant a colleague's next
  `setup` or `key rotate` silently re-opened a restricted issuer to the whole
  realm. `aic jwt-bearer subjects list/add/rm` edits the list; `--username`
  resolves to the user's `_id` first, because AM matches the raw `sub` claim and
  a username would be a lockout.

### Production

The 2026-08-06 decision was that the whole feature is refused on a
production-themed tenant, on the grounds that an empty `allowedSubjects` plus a
client with the grant enabled is a realm-wide capability. That reasoning was
sound about the _blank list_ and wrong about the _feature_: it also refused
`key list`, a read, and `key remove`, the revocation you most want available on
production. Superseded 2026-08-14 by a narrower rule.

**No `aic` write may leave an issuer unrestricted on a production-themed
tenant.** `setup` and `issuer create` take `--id`/`--username` so a first run
has a subject list; `key rotate` refuses against an already-unrestricted issuer;
`subjects rm` will not remove the last real subject. `key remove` is exempt —
withdrawing a signing key strictly reduces capability, and refusing a revocation
over an unrelated field is the mistake being corrected here. `aic auth` reads
the issuer on production only and refuses to mint against an unrestricted one.

That mint check is not a boundary against a determined operator, who can call AM
directly with the private key. It exists so the realm-wide _configuration_ never
exists on production in the first place, which is why the write rule carries the
weight. Ordinary production writes still need `--yes`, as everywhere else.

**None of this has run against a live production-themed tenant.** No configured
tenant carries that theme, so every refusal above is covered by unit tests and
by construction only — the tenant-side facts the rule rests on were probed on
the sandbox, but the gate itself has never fired in anger. Onboarding the first
production tenant should therefore include a deliberate `setup --id <uuid>` and
a `subjects list` before anyone needs either under time pressure.

- `aic jwt-bearer key export` emits the stored private JWK as one standard JWK
  object, retaining `kid` and any `aic_*` attribution members. With `--out` it
  creates a new mode-600 file and refuses to overwrite an existing path; without
  it, JSON goes to stdout. `aic jwt-bearer key import` accepts only an RSA
  private JWK with a non-empty `kid`, stores it in the same vault record, and
  refuses to replace an existing record without `--force`. After import it makes
  one best-effort read of the default issuer and warns if the imported `kid` is
  not published; publication is intentionally not performed by this local
  transfer command.
- `aic jwt-bearer key list` reads the default issuer's public `jwkSet`, shows
  each key's `aic_owner`, `aic_host`, and `aic_created` attribution, and marks
  the key whose private half is in the current tenant vault. `--json` emits the
  public key array without the local marker or any private material.
- `aic jwt-bearer key remove <kid> --force` removes one key from the default
  issuer after displaying its attribution. It refuses an unknown `kid` and
  reports the published KIDs, warns when the key is the local one, and permits
  an empty resulting set.
- `aic jwt-bearer key rotate` requires an existing local key and a
  non-placeholder operator. It publishes a newly generated public key alongside
  the old key, stores the new private record, then removes the old public key.
  This ordering leaves the install usable if any individual step fails; a failed
  final removal leaves an old public key for `key remove` to clean up.

## CLI shape

- aic auth --as-id <uuid> --client-id <id> or aic auth --as-username <name>
  --client-id <id> now resolves the subject, signs with the stored tenant key,
  exchanges at the tenant-relative realm token path, and supports repeatable
  --scope, --realm, --tenant, --client-secret-stdin, enum-valued --client-auth,
  and bare-token --token output. Discovery's `issuer` remains the source of the
  audience claim; its `token_endpoint` is not used as an outbound URL.

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

### 2026-08-07 — OAuth grant remediation probe

- Purpose: find out whether AM's `invalid_client` response distinguishes a
  client that lacks the JWT-bearer grant from one given a wrong secret, so
  `aic auth` can name the right remedy.
- Client: `test_authprobe`, created with
  `aic --no-prompt oauth create test_authprobe --client-type Confidential --generate-secret --scope openid --default-scope openid --grant authorization_code --grant urn:ietf:params:oauth:grant-type:jwt-bearer`,
  then `DELETE`d at the end.
- Three
  `aic --no-prompt auth --as-username testuser --client-id test_authprobe --client-secret-stdin --scope openid`
  calls, one variable changed at a time: grant present + correct secret **minted
  a token**; grant present + wrong secret and (after
  `aic --no-prompt oauth grant remove test_authprobe urn:…:jwt-bearer`) grant
  absent + correct secret both returned `invalid_client` with the identical
  `error_description`
  `Invalid authentication method for accessing this endpoint.`
- Also exercised on the same client, earlier the same day:
  `oauth grant list/add/remove`, add and remove idempotence, and live-schema
  rejection of an invalid grant value (`not_a_grant`, refused against the
  tenant's eleven allowed values).
- Subject used: the pre-existing sandbox `testuser`. No user was created or
  modified.

### 2026-08-07 — client authentication method probe

- `POST …/OAuth2Client?_action=schema` returned 200 and exposed
  `tokenEndpointAuthMethod.enum` with `client_secret_post`,
  `client_secret_basic`, `private_key_jwt`, `tls_client_auth`,
  `self_signed_tls_client_auth`, and `none`.
- Two 2×2 probes were run, by different operators, and **they disagree**. An
  agent run created `test_auth_basic_0807_codex` / `test_auth_post_0807_codex`
  and reported both crossed pairings failing. A reviewer re-ran the same design
  (`test_m_basic` / `test_m_post`, same secret per client, same published key,
  50-second settle, four `POST /am/oauth2/realms/root/realms/alpha/access_token`
  calls) and got **all four minting**, including both crossed pairings. Only the
  second run's raw results were observed directly by the person writing this
  entry. The disagreement is unexplained; the reviewer's run is the one
  reflected in the Quirks section, and neither run should be cited as
  establishing that a method mismatch fails.
- Both clients were `DELETE`d, and a subsequent client list confirmed neither id
  remained. Subject: the pre-existing sandbox `testuser`; no user or published
  JWT key was created, modified, rotated, or removed.

### 2026-08-14 — `allowedSubjects` semantics probe

- Purpose: decide whether "the issuer names its permitted subjects" is a sound
  precondition for allowing a mint on a production-themed tenant. It only is if
  AM enforces the field, enforces it promptly, and has no value that reads as
  restrictive while behaving as open.
- Tenant/realm: sandbox `alpha`, the existing shared default issuer `aic-agent`
  (one published key, `jwksCacheTimeout` 60000). Client
  `test_jwtbearer_subjects`, created with `aic --no-prompt oauth create` with
  `--grant urn:ietf:params:oauth:grant-type:jwt-bearer`, `--generate-secret`.
- Method: `allowedSubjects` was rewritten seven times by `PUT` on the issuer
  (every other field preserved, read wrappers unwrapped, server fields
  stripped), and after each write a mint was attempted with
  `aic --no-prompt auth --as-id … --client-secret-stdin --scope openid`. Two
  pre-existing sandbox users were used as subject and non-subject: `user.0`
  (`45565631-…`) and `user.3` (`2a9d3074-…`).
- Cases, in order: `[]` with a UUID subject (**positive control — minted**,
  establishing that the client, key and grant all work before any restriction
  was applied); `["<uuid user.0>"]` with `sub` = user.0 (minted) and with `sub`
  = user.3 (**refused**, `invalid_grant` "Issuer is not authorized to grant
  consent for this subject"); `["user.0"]` — the username — with `sub` = the
  matching UUID (refused) and via `--as-username user.0` (refused, same
  assertion since the CLI resolves first); `["*"]` with both subjects (both
  refused); `[""]` with both subjects (**both minted**); `["", "<uuid user.0>"]`
  with user.0 (minted) and user.3 (refused); `["   "]` with both subjects (both
  refused).
- The `[""]` and `["*"]` cases are the ones the conclusion rests on, and they
  are why they were run: a wildcard would have made "non-empty" insufficient,
  and the empty-string row shows that it is insufficient anyway. `["", uuid]`
  separates "a blank entry disables the list" from "a list of only blanks is no
  list", and it is the latter.
- No settle time was allowed anywhere; every refusal followed its write
  immediately, which is what establishes the promptness claim.
- Cleanup: `allowedSubjects` restored to `[]` and confirmed by re-read; the
  probe client `DELETE`d and confirmed absent from `aic oauth list`; minted
  tokens shredded. The published `jwkSet`, its single key, and both users were
  left untouched — no user was created or modified.

### 2026-08-14 — `aic jwt-bearer subjects` round trip

Exercising the new editor against the same sandbox issuer, to confirm AM accepts
the body it builds — unit tests cannot catch a body the tenant rejects.

- `subjects list` on the blank issuer printed the "unrestricted" line rather
  than an empty one; `subjects add --id <uuid>` then listed that subject.
- `subjects add --username user.0` for the user already added by `--id` reported
  **no change**, confirming the username was resolved to the same `_id` before
  comparison rather than appended as a second entry.
- `subjects add --id user.0` warned that the value is not UUID-shaped and
  proceeded; `subjects add --id "   "` was refused.
- `subjects rm` removed a present subject, reported no change for an absent one,
  and returned the list to empty.
- After all six writes the issuer re-read identical to its starting state:
  `allowedSubjects: []`, `issuer: aic-agent`, `jwksCacheTimeout: 60000`,
  `consentedScopesClaim: scope`, `resourceOwnerIdentityClaim: sub`, and the one
  published `kid` unchanged — the property that a subject edit must not disturb
  the shared key set, checked on the wire and not only in a unit test.

## Source citations

- Ping docs:
  <https://docs.pingidentity.com/pingoneaic/latest/am-oauth2/oauth2-jwt-bearer.html>
  (grant overview). Field semantics here come from the tenant's own
  `_action=schema`, which is more reliable.
- frodo-lib: `src/api/AgentApi.ts` (generic `realm-config/agents/{type}` CRUD).
- Not covered by fr-config-manager.

## Open questions

- **Does removing a key from `jwkSet` revoke it promptly? Unresolved — assume
  not.** Probing on 2026-08-07 confirmed the _write_ lands: after
  `aic jwt-bearer key remove`, AM stores `jwkSet` as `{"keys":[]}` and
  `key list` shows nothing. But an assertion signed with the removed key still
  minted a token immediately afterwards, and follow-up probes gave
  **inconsistent** results — including tokens minted from RSA material never
  published to any issuer, on some runs but not others. At least three
  confounders are in play and none was isolated: the issuer's own
  `jwksCacheTimeout` (3600000 ms) and `jwkStoreCacheMissCacheTime` (60000 ms), a
  propagation delay on freshly created OAuth2 clients (a client used within ~20s
  of creation returns `invalid_client`), and AM's documented fallback of trying
  every key in the set when the assertion's `kid` names none of them. Until
  someone characterises this properly, **do not treat `key remove` as immediate
  revocation** — rotate the affected clients' secrets too, or delete the issuer
  outright. Deliberately no `## Verified against` entry: the calls were made but
  they do not support a conclusion.
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
