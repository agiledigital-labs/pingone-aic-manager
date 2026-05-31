# 99 — Cross-cutting quirks & open questions

This file collects findings that span multiple capability files, contradictions
the research process surfaced, and TODOs. Append-only with dated entries when
new things are learned.

---

## Resolved (verified live 2026-05-17)

### Q1. Script body encoding
- **Status:** Resolved.
- **Answer:** Script bodies ARE base64-encoded over the wire, both directions
  (GET returns base64, PUT must send base64).
- **Evidence:** `GET /am/json/realms/root/realms/alpha/scripts/{id}` returned
  `"script": "LyoKICogQ29weXJpZ2h0..."` which base64-decodes to a JS comment
  header. Matches Ping docs and fr-config-manager's push code.
- **Why it mattered:** The frodo-lib research summary I received claimed scripts
  are NOT base64. That summary was wrong — verify before trusting library
  research summaries.
- **Documented in:** `04-scripts.md`.

### Q2. ESV endpoint shape
- **Status:** Resolved.
- **Answer:** `/environment/variables` and `/environment/secrets` (frodo-lib
  shape). `/environment/esv` (the path Ping docs sometimes use) returns 404
  on the sandbox.
- **Documented in:** `03-esvs.md`.

### Q3. Token endpoint path
- **Status:** Resolved.
- **Answer:** `POST /am/oauth2/access_token` at the root (no `/realms/...`
  segment). `client_id=service-account` (a fixed string, not the SA UUID).
- **Documented in:** `00-auth.md`.

### Q4. `Accept-API-Version` per family
- **Status:** Resolved for the families we use.
- **Documented in:** `02-headers-and-versioning.md` (full table).

### Secret-stores availability
- **Status:** Resolved (negative).
- **Answer:** AIC explicitly refuses the secret-stores API. 403 with message
  "This operation is not available in PingOne Advanced Identity Cloud."
  ESVs are the only AIC secret-management surface.
- **Implication for UX:** Don't add a "Secret Stores" tab.
- **Documented in:** `07-secret-stores.md`.

### Scripts have no `_rev`
- **Status:** Resolved.
- **Answer:** Confirmed — neither list nor per-script GET returns `_rev`. The
  Scripts API uses no optimistic-locking header. Sync engine must use content
  equality. (See `04-scripts.md` for the algorithm.)

### ESV variables have no `_rev`
- **Status:** Resolved.
- **Answer:** Same as scripts — no `_rev`. `lastChangeDate` is the only
  staleness signal, and `lastChangeDate` precision can collide if two writes
  land in the same microsecond. Use content equality for variables too.

### Scripts: full context enumeration
- **Status:** Resolved.
- **Captured in:** `04-scripts.md` (41 contexts as of 2026-05-17, including
  `_NEXT_GEN` variants the libraries' research summaries missed).

---

## Open

### Q5. Log API keys not present in `.envrc`
- The sandbox has no `LOG_API_KEY_ID` / `LOG_API_KEY_SECRET`. Live verification
  of `/monitoring/logs/*` is deferred.
- **Action:** Either generate a pair in the admin console and add to `.envrc`,
  or programmatically mint one via `POST /keys?_action=create` (which uses
  the SA bearer — already works).

### Q6. PUT on resources without `_rev`
- For ESV variables and scripts (both lack `_rev`), what is the server's
  conflict behaviour? Last-write-wins, presumably. Verify by deliberately
  racing two PUTs on a throwaway script.

### Q7. `X-Requested-With: XMLHttpRequest`
- frodo-lib sends this on all AM requests. Is it actually required (CSRF guard
  on PUT/POST/DELETE?), or just defensive? Test by omitting.

### Q8. PUT response shape
- For `PUT /am/json/.../scripts/{id}` — does the server echo the full object,
  or return a thin `{_id, _rev}`? Test on a throwaway script before
  implementing the Rust client (affects whether we need an extra GET after PUT
  to refresh the in-memory model).

### Q9. ESV ID prefix rules
- Does AIC enforce the `esv-` prefix on user-created variables/secrets? Test
  by `PUT /environment/variables/foobar` (no prefix) on a throwaway.

### Q10. Custom-domain URL form
- With a custom domain, do `/am/json/...` paths still need the
  `/realms/root/realms/{realm}` segment, or does the hostname imply the realm?
  Not testable in sandbox (no custom domain configured).

---

## Contradictions discovered during research (for future-Claude awareness)

| Claim | Source | Reality (verified) |
|---|---|---|
| Script bodies are plain text | frodo-lib research summary | False — base64 in both directions. |
| Scripts have `_rev` | frodo-lib research summary | False — no `_rev` anywhere. |
| ESV path is `/environment/esv` | Ping docs research summary | 404 — use `/environment/variables` and `/environment/secrets`. |
| Secret stores API works in AIC | both library summaries | 403 "not available in PingOne Advanced Identity Cloud". |
| Secret `PUT` is an upsert | 03-esvs.md (transcribed) | False — create-only; re-PUT → 400 "already exists". Change value via new version. |
| Version status `changestatus` accepts DESTROYED | 03-esvs.md (transcribed) | False — only ENABLED/DISABLED. DESTROYED is via `DELETE …/versions/{v}` (one-way). |
| Secret version objects have `_id` | 03-esvs.md (transcribed) | False — versions are a bare array of `{version, createDate, loaded, status}`. |
| `useInPlaceholders` defaults to true | 03-esvs.md (transcribed) | False — required on create, no default, and immutable afterwards. |

**Lesson:** Both libraries' research summaries had errors. Always verify
endpoints + shapes against the live tenant before writing code.

### Q11. PKCE browser flow — superseded (resolved 2026-05-20)
- **Status:** Superseded by Q12. Original investigation kept for posterity.
- **Original finding:** `idmAdminClient` rejects `http://localhost:*/callback`
  loopback redirect URIs, so we tried provisioning a dedicated `AicEdit` OAuth2
  client in the alpha realm. That worked for alpha-realm identities, but
  platform admins (the users who actually need to bootstrap aic-edit) live in
  the **root realm** in AIC, and AIC explicitly blocks root-realm OAuth2 client
  management API (`403 "This operation is not available in PingOne Advanced
  Identity Cloud"`). DCR is exposed but rejects SA bearers. Device code is in
  `grant_types_supported` but no `device_authorization_endpoint` is advertised.
  → no path to a loopback PKCE flow for the actual users.
- **Replacement:** Three bootstrap patterns documented in Q12 below. The
  `AicEdit` client is no longer required and can be deleted from tenants where
  it was provisioned.

### Q12. Bootstrap auth flows (resolved 2026-05-20)
- **Status:** Resolved end-to-end; verified by `scripts/verify-pattern1-cookie.sh`
  and `scripts/verify-pattern2-userpass.sh`.

Three patterns produce an initial Bearer that can `POST /openidm/managed/svcacct`
to mint a long-lived service account. After bootstrap, the SA's JWK takes over;
the bootstrap credentials are discarded.

**Pattern 1 — Paste session cookie.** User logs into the AIC admin console in
their normal browser (full SSO/MFA/passkey/SAML stack). They copy the AM
session cookie value from DevTools → Application → Cookies. aic-edit drives the
OAuth flow server-side using the cookie.

**Pattern 2 — Username/password in-app.** aic-edit walks AM's authentication
journey via `POST /am/json/realms/root/authenticate`, handling each callback
round (NameCallback, PasswordCallback, ConfirmationCallback, etc.). Works for
username+password and TOTP. Does NOT work for passkey/push/CAPTCHA — those
require a real browser.

**Pattern 3 — Paste SA details.** User already has a service account JWK and
client_id. aic-edit stores them directly. (Same path as `scripts/verify-endpoint.sh`.)

#### Pattern 1 wire details

1. `GET /am/json/serverinfo/*` → `cookieName` (per-tenant hex, e.g.
   `da4bb2cc51f31d3`). Do NOT hardcode; AIC randomises per tenant.
2. PKCE: `code_verifier` random URL-safe 32 bytes, `code_challenge =
   base64url(SHA256(verifier))`.
3. `GET /am/oauth2/realms/root/authorize` with:
   - `client_id=idmAdminClient`
   - `response_type=code`
   - `scope=openid fr:idm:*`  *(other `fr:*` scopes rejected by this client —
     see scope rules below)*
   - `redirect_uri=https://{TENANT_BASE}/platform/appAuthHelperRedirect.html`
     *(must be exactly this URL; same host as AM, NOT `id.forgerock.io`)*
   - `code_challenge`, `code_challenge_method=S256`, `state`
   - Header: `Cookie: <cookieName>=<sessionValue>; amlbcookie=01`
   - HTTP client MUST NOT follow redirects.
4. Response is `HTTP 302` with `Location: <redirect_uri>?code=...&state=...`.
   Parse the `code` from the Location header — never navigate there.
5. `POST /am/oauth2/realms/root/access_token` (form-encoded):
   - `grant_type=authorization_code`
   - `code`, `client_id=idmAdminClient`, `redirect_uri` (same as above),
     `code_verifier`
6. Response JSON has `access_token` (a JWT) with scope `openid fr:idm:*`,
   audience `idmAdminClient`, `expires_in=3600`.

#### Pattern 2 wire details

1. `POST /am/json{realm-path}/authenticate` with NO body and header
   `Accept-API-Version: resource=2.0, protocol=1.0`. Do NOT specify
   `authIndexValue=Login` — AIC's root and alpha realms have NO named "Login"
   tree (returns `"No Configuration found"`). The realm's default journey is
   used when no `authIndexValue` is provided.
2. Response is a JSON `{ "authId": "...", "callbacks": [...] }`. Walk callbacks:
   - **NameCallback**: prompt `"User Name"` → username. Prompt
     `"Enter verification code"` (or anything containing
     `otp/code/token/verification`) → **OTP**. AIC reuses NameCallback for OTP
     entry; do not assume it's always the username.
   - **PasswordCallback**: password (or OTP if prompt contains an OTP keyword).
   - **ConfirmationCallback**: hidden "Submit" button. Send the
     `defaultOption` value from the callback's `output` array (typically `0`).
     Do NOT prompt the user — this is the form's submit button, not a question.
   - **BooleanAttributeInputCallback** (e.g. "Trust this device"): default
     `false`.
   - **TextOutputCallback**, **HiddenValueCallback**: leave as-is.
   - **PollingWaitCallback**: passkey/push — abort with a clear message.
3. POST the modified callback array back to the same URL. AIC may return more
   rounds (HAR shows 3 rounds for u/p+TOTP). Loop until the response contains
   `tokenId`.
4. `tokenId` is the session cookie value. Continue with Pattern 1 step 3
   (skip step 1 — we already minted a session, just need the cookie name).

#### Bearer scope asymmetry (important)

The bootstrap Bearer from `idmAdminClient` only carries `openid fr:idm:*`. The
real admin console requests 15+ `fr:idc:*` scopes but the SA-creation API only
needs `fr:idm:*`. **The SA we create can declare WIDER scopes than the token
that minted it** — request the full set in the SA's `scopes` array:

```json
["fr:idm:*", "fr:am:*", "fr:idc:esv:*", "fr:idc:cookie-domain:*"]
```

The SA's own tokens (via JWT-bearer grant) then carry those wider scopes.
Verified 2026-05-20: SA bearer minted from a Pattern-1-bootstrapped SA had
`scope: "fr:am:* fr:idc:esv:* fr:idm:*"`.

#### Service account create body

`POST /openidm/managed/svcacct?_action=create` with:
```json
{
  "name": "...",
  "description": "...",
  "scopes": ["fr:idm:*", "fr:am:*", "fr:idc:esv:*", "fr:idc:cookie-domain:*"],
  "accountStatus": "Active",
  "jwks": "<full JWKS JSON as a string>"
}
```

- `jwks` is a **string** containing JSON `{"keys":[{...public RSA JWK...}]}`.
  Not a JSON object — the API stores it as an LDAP `fr-attr-jwks` directory
  string. An empty `jwks` returns
  `"Invalid Attribute Syntax: ... zero-length value"`.
- Only the public key fields go in (`kty`, `kid`, `alg`, `use`, `n`, `e`) —
  never `d/p/q/dp/dq/qi` (those stay private).
- 2048-bit RSA is sufficient. The kid you set here must match the `kid` header
  on JWT-bearer assertions later.

#### Cookie discovery
- `GET /am/json/serverinfo/*` → `cookieName` field. Required, not constant.
- The `amlbcookie=01` load-balancer cookie should also be sent on AM requests
  for stickiness.

---

## Changelog

- **2026-05-17** — Initial verification pass; Q1-Q4 + secret stores resolved.
- **2026-05-18** — Step 2 implemented (TUI skeleton, crypto, onboarding). Q11 (PKCE redirect URI) resolved; `AicEdit` OAuth2 client provisioned.
- **2026-05-20** — Q11 superseded by Q12. Three bootstrap patterns verified end-to-end via `scripts/verify-pattern1-cookie.sh` and `scripts/verify-pattern2-userpass.sh`. `AicEdit` OAuth2 client no longer required.
- **2026-05-30** — Full ESV **secrets** lifecycle verified live (create/version/changestatus/destroy/delete on throwaway `esv-aicedit-sec*`). Corrected four wrong transcribed claims in `03-esvs.md` (see contradictions table). Key facts: secret PUT is create-only; value changes go through versions; `useInPlaceholders` required + immutable and gates the restart (`false` ⇒ never pending); `changestatus` is ENABLED/DISABLED only; latest version can't be disabled; encodings `generic`/`pem`/`base64hmac`/`base64aes` with the last two double-base64-encoded.
- **2026-05-31** — Secret `setDescription` returns **`200` with a zero-byte body**, *not* the echoed object the doc previously claimed (verified on throwaway `esv-secret-test1`). A strict `resp.json()` on the success body fails with "error decoding response body". Fixed generically in `AicClient::check_response`: an empty success body now maps to JSON `null` instead of erroring, so any empty-`200` write action is handled. Corrected the `03-esvs.md` table row.
- **2026-06-01** — AM scripts list paging: `_queryFilter=true` returns **all** results when `_pageSize` is omitted (alpha 107, bravo 283 in one response). With `_pageSize` set, the response caps at that size but `pagedResultsCookie` comes back **`null`** — cookie paging silently truncates. `_pagedResultsOffset` + `remainingPagedResults` work, so `am::list` now pages by offset (PAGE=1000, stop when `remainingPagedResults == 0`). This replaces the earlier (buggy) cookie loop. `docs/api/04-scripts.md` updated.
- **2026-06-01** — IDM **scheduled jobs** added as a syncable script kind (`config/schedule/*`). Verified live: only `invokeService:"script"` schedules carry an inline script (at `invokeContext.script.source`); `taskscanner`/`sync` ones don't. Same `/openidm/config` CRUD as endpoints (no `_rev`, PUT 201/200, DELETE 200). A source-only push merges just `invokeContext.script.source` and round-trips the rest (cron `schedule`, `enabled`, `globals`). See `11-idm-endpoints.md`.
- **2026-05-31** — IDM custom endpoints verified for the script-sync feature (full CRUD on throwaway `endpoint/aicedit-verify`): `PUT` create → 201, `PUT` replace → 200, `DELETE` → 200 (echoes object), `GET` after → 404. Key facts: **no `_rev`** (so content-based conflict detection, same as AM scripts), `source` is **plain text** (not base64, unlike AM scripts), **no `Accept-API-Version`** header required for `/openidm` config, list (`/openidm/config?_queryFilter=true`) is unfiltered (filter `endpoint/` ids client-side), no `name` field (name = `_id` suffix). New doc `11-idm-endpoints.md`. Also: added an optional per-call `Accept-API-Version` override to the agent `ApiCall` protocol so AM scripts can send `protocol=2.0,resource=1.0` while everything else keeps the `resource=1.0` default.
