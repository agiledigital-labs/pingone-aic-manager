# 00 — Authentication

## Purpose

Mint an OAuth2 access token from a service account using the JWT bearer grant
(RFC 7523). The token is used as `Authorization: Bearer …` for every AIC API
call except `/monitoring/logs/*` (see [08-logs.md](08-logs.md)).

## Token endpoint

| Op         | Method | Path                      | Notes                                                   |
| ---------- | ------ | ------------------------- | ------------------------------------------------------- |
| Mint token | `POST` | `/am/oauth2/access_token` | Root realm only. Do **not** add `/realms/...` segments. |

Form body (`application/x-www-form-urlencoded`):

```
client_id=service-account
grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer
assertion=<signed-JWT>
scope=fr:idm:* fr:am:* fr:idc:esv:* fr:idc:cookie-domain:*
```

## JWT structure

Header:

```json
{ "alg": "RS256", "typ": "JWT" }
```

Claims:

```json
{
  "iss": "<SERVICE_ACCOUNT_ID>", // tenant service-account UUID
  "sub": "<SERVICE_ACCOUNT_ID>", // same as iss
  "aud": "https://<tenant>/am/oauth2/access_token",
  "exp": 1779019160, // now + ≤180s
  "jti": "<uuid>" // unique per request
}
```

Signed with **RS256** using the service account's RSA private key (delivered as
a JWK at account creation).

## Token response

```json
{
  "access_token": "eyJ...",
  "scope": "fr:am:* fr:idc:esv:* fr:idm:* fr:idc:cookie-domain:*",
  "token_type": "Bearer",
  "expires_in": 898
}
```

- TTL is **898 seconds** (~15 min). Refresh proactively at ~60s before expiry.
- `scope` may be a re-ordered subset of requested scopes — compare as a set.

## Scopes (observed working in sandbox)

| Scope                    | Grants                                                           |
| ------------------------ | ---------------------------------------------------------------- |
| `fr:am:*`                | Full AM (scripts, OAuth2 clients, SAML, journeys, realm config). |
| `fr:idm:*`               | Full IDM (managed objects, mappings, schedules).                 |
| `fr:idc:esv:*`           | ESV CRUD + startup/restart.                                      |
| `fr:idc:cookie-domain:*` | Cookie domain management.                                        |
| `fr:idc:esv:read`        | Read-only ESV.                                                   |
| `fr:idc:esv:update`      | ESV write (no restart).                                          |
| `fr:idc:esv:restart`     | Trigger startup/restart only.                                    |

`SA_SCOPES` in `src/onboard/bootstrap.rs` is what onboarding actually grants:
`fr:idm:*`, `fr:am:*`, `fr:idc:esv:*`, `fr:idc:cookie-domain:*`. Everything the
tool does today fits inside those four.

### Other `fr:idc:*` scopes a service account can hold

A service account can be granted considerably more than we ask for. These were
granted to an SA and **confirmed present in the minted token** via
`GET /am/oauth2/realms/root/tokeninfo` (2026-06-24, during the `/keys`
investigation in [08-logs.md](08-logs.md)):

`analytics`, `telemetry`, `dataset`, `certificate`, `content-security-policy`,
`custom-domain`, `promotion`, `release`, `sso-cookie`, `cookie-domain`, `esv` —
each as `fr:idc:<name>:*`.

Two caveats, both important before you reach for one:

- **Holding a scope is not the same as it granting anything.** All that was
  established is that the SA can carry them. What each authorises has not been
  exercised, and the probe that granted them was proving a _negative_ — the log
  `/keys` API stayed 403 with every one of them held, because it is gated to a
  scope no service account can have.
- **The count does not reconcile.** Both [08-logs.md](08-logs.md) and
  [99-quirks-and-open-questions.md](99-quirks-and-open-questions.md) say "all 13
  `fr:idc:*` scopes" but only ever name the 11 above. Either two were granted
  and not written down, or 13 is wrong. Unresolved as of 2026-08-07 —
  re-enumerate from the console's service-account scope picker before relying on
  the number.

Log API uses a **separate** `x-api-key`/`x-api-secret` pair generated in the
admin console — not a bearer token. See [08-logs.md](08-logs.md).

## Object shapes

The JWK in `.envrc` has the standard RSA private fields: `kty=RSA`, `e`, `n`,
`d`, `p`, `q`, `dp`, `dq`, `qi`. Convert to PKCS#8 PEM for signing libraries
that need PEM (most do); `josekit` accepts JWK directly.

## Examples

```bash
# Sign JWT (Python, using pyjwt + cryptography):
import jwt, json, time, uuid
claims = {
    "iss": SA_ID, "sub": SA_ID,
    "aud": f"{TENANT}/am/oauth2/access_token",
    "exp": int(time.time()) + 180,
    "jti": str(uuid.uuid4()),
}
assertion = jwt.encode(claims, pem, algorithm="RS256")
```

```bash
# Exchange for bearer token:
curl -sS "$TENANT_BASE_URL/am/oauth2/access_token" \
  -d client_id=service-account \
  -d grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer \
  -d assertion="$ASSERTION" \
  --data-urlencode "scope=fr:am:* fr:idm:* fr:idc:esv:* fr:idc:cookie-domain:*"
```

## Quirks

- **`aud` must match the actual POST URL.** Mismatch → 400.
- **`exp` is enforced strictly.** Use ≤180s; longer is rejected by some
  deployments.
- **Realm-prefixed token paths (`/am/oauth2/realms/root/access_token`)** also
  exist, but the root form `/am/oauth2/access_token` is the documented and
  observed-working one. Don't use the realm-prefixed form.
- **`client_id=service-account`** is a fixed string, not the service account
  UUID. The UUID goes in `iss`/`sub`.
- **Never persist tokens to disk.** In-memory cache only. The verify script
  writes `.token-cache` for the dev sandbox; that file is gitignored.

## Verified against

- Tenant: `<your-tenant>.forgeblocks.com`
- Date: 2026-05-17
- Calls: `POST /am/oauth2/access_token` (200 OK, 898s TTL).
- Date: 2026-08-06 (principal resolution, SA bearer only)
- Calls: `GET /am/json/realms/root/users/{id}` for six principals taken from
  alpha `scripts.lastModifiedBy` — four humans, two service accounts (200 each);
  `GET /am/oauth2/realms/root/userinfo` and `/tokeninfo` with the SA bearer
  (200, SA UUID as `sub`/`subname`); `GET /openidm/managed/svcacct/{sa_id}`
  (200) and `GET /openidm/managed/teammember/{sa_id}` (403).
- Date: 2026-08-13 (service-account deletion, SA bearer only)
- Calls: `DELETE /openidm/managed/svcacct/00000000-0000-0000-0000-000000000000`
  → **403** (nonexistent id chosen deliberately: a 403 there is the access layer
  refusing the method, and no object could have been destroyed by the probe);
  `DELETE /am/json/realms/root/realms/alpha/realm-config/agents/TrustedJwtIssuer/aic-probe-does-not-exist`
  → **404** `{"code":404,"reason":"Not Found"}`, the same probe shape showing
  permission passes on the issuer route. Both on the SA bearer from
  `aic whoami --token`. Self-deletion of a **live** SA was not probed: the only
  discriminating call destroys the credential the project runs on.

## Resolving the onboarding admin's username (verified 2026-06-25)

To name onboarding-created credentials after the human who onboarded (not a
generic alias), resolve the admin user from the `idmAdminClient` bearer that
`session_to_bearer` already mints (scope `openid fr:idm:*`):

1. `GET /am/oauth2/realms/root/userinfo` (Bearer) → `{ "sub": "<uuid>", ... }`.
   This client returns **only `sub`/`subname`** — `profile`/`email` (and
   `fr:am:*`) scopes are **rejected** with `invalid_scope`, so userinfo never
   carries a readable name. The `sub` is the admin's user UUID.
2. `GET /openidm/managed/teammember/{sub}` (Bearer) →
   `{ "userName": "dsbalmain@agiledigital.com.au", "mail": ..., "cn": ... }`.
   **AIC tenant admins are IDM `managed/teammember` objects**, and this read
   works with the admin bearer's `fr:idm:*` scope — no SA, no `fr:am:*`, and it
   works _before_ any SA exists, so it can name the SA too.

Both calls succeed on every onboarding path (cookie / userpass / log-only) since
all hold the admin bearer. Use `userName` (fall back to `mail`); on any failure
fall back to a non-identifying name — never block onboarding on it.

Note: the `who-changed` reference script resolves principals via
`GET /am/json/realms/root/users/{id}`
(`Accept-API-Version: protocol=2.1,resource=3.0`), but that AM path needs an
**`fr:am:*` SA token** (the admin `fr:idm:*` bearer gets **401** there) — the
`teammember` route above is preferred because it needs nothing extra.

## Resolving any principal, and telling a human from an SA (verified 2026-08-06)

Audit-ish fields (`lastModifiedBy`, `createdBy` on scripts; `payload.userId` in
am-access logs) hold a DN, not a name:

```
id=ad604d54-ef8e-454c-b3f3-c2f8197b56f5,ou=user,ou=am-config
```

Extract the `id=` segment and `GET /am/json/realms/root/users/{id}` with
`Accept-API-Version: protocol=2.1,resource=3.0`. **This works with our own SA
bearer** — no admin session. Two distinct shapes come back:

| Principal                | `username`       | `mail`    | `cn`                                                |
| ------------------------ | ---------------- | --------- | --------------------------------------------------- |
| **Human** (tenant admin) | the real email   | populated | the email, doubled                                  |
| **Service account**      | the UUID, echoed | `null`    | the SA's name, e.g. `DaveBalmain-fr-config-manager` |

So the discriminator is **`username == _id` → service account**, and its
readable name is `cn`. A human's readable name is `username`/`mail`.

Consequences:

- A change made with SA credentials traces back to the **service account's
  name**, not to whoever was holding the JWK. On a tenant where each person has
  their own SA this is a good proxy for a person; it is a naming convention, not
  an identity. Observed on the sandbox: `DaveBalmain-fr-config-manager`
  (person), `Frodo-SA-1735012367301` (tool, no person in the name at all).
- The **token itself carries no operator identity**.
  `GET /am/oauth2/realms/root/userinfo` and `/tokeninfo` with an SA bearer
  return the SA's UUID as both `sub` and `subname`, `username: null`,
  `client_id: service-account`. Don't reach for `userinfo` to find out who is
  running `aic` — it structurally cannot say.
- The SA can also read its own IDM record:
  `GET /openidm/managed/svcacct/{sa_id}` (200) returns `name`, `scopes`,
  `accountStatus`. The same SA gets **403** on `managed/teammember/{id}`, so the
  teammember route really is admin-bearer-only.
- `dsameuser` and `amadmin` also appear as principals on stock content. Neither
  resolves to a person; treat them as "the platform".

## Deleting a service account (verified 2026-08-13)

**A service-account bearer cannot delete a service account.**
`DELETE /openidm/managed/svcacct/{id}` returns **403**, and it does so for an id
that does not exist — so the refusal comes from the access layer before any
object lookup, not from the object being someone else's. Read-your-own
(`GET /openidm/managed/svcacct/{sa_id}` → 200, above) is a narrow exception and
does not extend to writes.

Consequence for tenant offboarding: removing an SA is **admin-bearer work**, the
same plane as `/keys` (`docs/api/08-logs.md`) — mint one via the
`idmAdminClient` PKCE flow `session_to_bearer` uses, or delete it in the
console. `aic ctx rm` therefore reports the `sa_id` for console cleanup rather
than deleting the account itself; only the local private JWK is in its reach.

By contrast a Trusted JWT Issuer **is** deletable on the SA bearer:
`DELETE /am/json{realm-path}/realm-config/agents/TrustedJwtIssuer/{id}` on a
nonexistent id returns **404**, so permission passes and only the object was
missing (`docs/api/17-jwt-bearer-user-tokens.md` has the successful-delete row).

## Source citations

- frodo-lib: `src/ops/OAuth2OidcOps.ts` (`accessTokenRfc7523AuthZGrant`),
  `src/api/OAuth2OIDCApi.ts`, `src/ops/JoseOps.ts`.
- fr-config-manager: `packages/fr-config-common/src/authenticate.js`.
- Ping docs:
  <https://docs.pingidentity.com/pingoneaic/latest/developer-docs/authenticate-to-rest-api-with-access-token.html>

## Open questions

- None. Verified end-to-end.
