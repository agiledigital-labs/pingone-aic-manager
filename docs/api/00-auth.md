# 00 — Authentication

## Purpose
Mint an OAuth2 access token from a service account using the JWT bearer grant
(RFC 7523). The token is used as `Authorization: Bearer …` for every AIC API
call except `/monitoring/logs/*` (see [08-logs.md](08-logs.md)).

## Token endpoint

| Op | Method | Path | Notes |
|----|--------|------|-------|
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
  "iss": "<SERVICE_ACCOUNT_ID>",       // tenant service-account UUID
  "sub": "<SERVICE_ACCOUNT_ID>",       // same as iss
  "aud": "https://<tenant>/am/oauth2/access_token",
  "exp": 1779019160,                    // now + ≤180s
  "jti": "<uuid>"                       // unique per request
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

| Scope | Grants |
|-------|--------|
| `fr:am:*` | Full AM (scripts, OAuth2 clients, SAML, journeys, realm config). |
| `fr:idm:*` | Full IDM (managed objects, mappings, schedules). |
| `fr:idc:esv:*` | ESV CRUD + startup/restart. |
| `fr:idc:cookie-domain:*` | Cookie domain management. |
| `fr:idc:esv:read` | Read-only ESV. |
| `fr:idc:esv:update` | ESV write (no restart). |
| `fr:idc:esv:restart` | Trigger startup/restart only. |

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

## Resolving the onboarding admin's username (verified 2026-06-25)

To name onboarding-created credentials after the human who onboarded (not a
generic alias), resolve the admin user from the `idmAdminClient` bearer that
`session_to_bearer` already mints (scope `openid fr:idm:*`):

1. `GET /am/oauth2/realms/root/userinfo` (Bearer) → `{ "sub": "<uuid>", ... }`.
   This client returns **only `sub`/`subname`** — `profile`/`email` (and
   `fr:am:*`) scopes are **rejected** with `invalid_scope`, so userinfo never
   carries a readable name. The `sub` is the admin's user UUID.
2. `GET /openidm/managed/teammember/{sub}` (Bearer) → `{ "userName":
   "dsbalmain@agiledigital.com.au", "mail": ..., "cn": ... }`. **AIC tenant
   admins are IDM `managed/teammember` objects**, and this read works with the
   admin bearer's `fr:idm:*` scope — no SA, no `fr:am:*`, and it works *before*
   any SA exists, so it can name the SA too.

Both calls succeed on every onboarding path (cookie / userpass / log-only) since
all hold the admin bearer. Use `userName` (fall back to `mail`); on any failure
fall back to a non-identifying name — never block onboarding on it.

Note: the `who-changed` reference script resolves principals via
`GET /am/json/realms/root/users/{id}` (`Accept-API-Version:
protocol=2.1,resource=3.0`), but that AM path needs an **`fr:am:*` SA token**
(the admin `fr:idm:*` bearer gets **401** there) — the `teammember` route above
is preferred because it needs nothing extra.

## Source citations

- frodo-lib: `src/ops/OAuth2OidcOps.ts` (`accessTokenRfc7523AuthZGrant`),
  `src/api/OAuth2OIDCApi.ts`, `src/ops/JoseOps.ts`.
- fr-config-manager: `packages/fr-config-common/src/authenticate.js`.
- Ping docs: <https://docs.pingidentity.com/pingoneaic/latest/developer-docs/authenticate-to-rest-api-with-access-token.html>

## Open questions

- None. Verified end-to-end.
