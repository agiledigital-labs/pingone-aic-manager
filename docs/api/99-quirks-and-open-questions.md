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
  shape). `/environment/esv` (the path Ping docs sometimes use) returns 404 on
  the sandbox.
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
  "This operation is not available in PingOne Advanced Identity Cloud." ESVs are
  the only AIC secret-management surface.
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

### Managed config relationship reciprocity (resolved 2026-06-14)

- **Status:** Resolved.
- **Answer:** `PUT /openidm/config/managed` performs no cross-object
  relationship-reciprocity validation on write. A relationship property with
  `validate: true` and a dangling `reversePropertyName` returned 200 and was
  stored.
- **Implication:** Treat `validate` / `reverseRelationship` as runtime
  relationship-integrity flags, not config-schema write gates. Tooling must
  validate reciprocity itself if it wants to prevent one-way or dangling
  relationships.
- **Documented in:** `10-managed-objects.md`.

### Managed-object paging counts (resolved 2026-06-21)

- **Status:** Resolved.
- **Answer:** `totalPagedResults` is an optional hint for managed-object lists,
  not a safe completeness bound. Empty objects can return `-1` / policy `NONE`
  even when `_totalPagedResultsPolicy=EXACT` is requested; populated objects can
  return policy `ESTIMATE`.
- **Implication:** Full sync must walk cursor cookies until the cookie is empty
  or absent. Do not use `_pagedResultsOffset` for bulk record reads: it re-runs
  the query per page, can skip or duplicate records under concurrent backend
  changes, and deep offsets are costly. `totalPagedResults` is only an optional
  progress hint.
- **Documented in:** `10-managed-objects.md`.

### Managed query-filter negation spelling (resolved 2026-07-03)

- **Status:** Resolved.
- **Answer:** Managed-object `_queryFilter` accepts symbolic negation with `!`,
  for example `!(/description eq "lkj")`. The word form
  `not (/description eq "lkj")` is rejected with HTTP 400.
- **Implication:** Static query-filter validation should accept `!` but must not
  treat `not` as an alias.
- **Documented in:** `10-managed-objects.md`.

### Journey `_rev` and write semantics (resolved 2026-06-14)

- **Status:** Resolved.
- **Answer:** Journey trees and nodes accept plain `PUT` for both create and
  update; no `If-Match` or `If-None-Match` header is required. Their `_rev`
  values are content-derived: re-PUTting byte-identical content returned the
  same `_rev`.
- **Implication:** Use a local content snapshot for push conflict detection,
  stripping `_rev` before comparison. Do not treat `_rev` as a monotonic
  optimistic-lock counter for journeys.
- **Documented in:** `09-journeys.md`.

### OAuth2 client write semantics (resolved 2026-06-14)

- **Status:** Resolved.
- **Answer:** OAuth2 clients accept plain `PUT` for create and update; no
  `If-Match` header is required or available through the agent transport. A
  `PUT` body containing server-managed top-level fields `_id`, `_rev`, `_type`,
  or `_provider` is rejected with `400 "Invalid attribute specified."`.
- **Implication:** Strip those fields before every PUT, strip `*-encrypted` keys
  recursively, and use local content snapshots for conflict detection. Treat
  `_rev` as opaque metadata and ignore it in content comparisons.
- **Documented in:** `05-oauth2-oidc.md`.

### Journey join key (resolved 2026-07-01)

- **Status:** Resolved.
- **Answer:** Group journey progress on the full `payload.trackingIds[0]` value,
  verbatim, with no stripping or transformation.
- **Documented in:** `08-logs.md`.

### Log key minting requires admin-user bearer (resolved 2026-06-24)

- **Status:** Resolved.
- **Answer:** `GET /keys` and `POST /keys?_action=create` require an admin-user
  bearer; the service-account bearer 403s regardless of scopes.
- **Documented in:** `08-logs.md`.

---

## Open

### Q5. Log API keys not present in `.envrc`

- **Status:** Resolved.
- **Answer:** The sandbox has no `LOG_API_KEY_ID` / `LOG_API_KEY_SECRET`, and
  `POST /keys?_action=create` is not service-account-accessible. Log-key minting
  requires an admin-user bearer; the SA bearer gets 403 regardless of scopes.
- **Documented in:** `08-logs.md`.

### Q15. Can a log API key delete itself? (2026-08-13)

- **Question:** `DELETE /keys/{id}` needs an admin-user bearer (`08-logs.md`,
  verified 2026-06-24). If the api-key **pair** were accepted on its own
  `/keys/{id}`, tenant offboarding could revoke a log key without an admin
  session. Worth knowing before anyone builds around the admin requirement.
- **Status: not established.** The probe ran, and the control failed, so the
  result is void: `DELETE /keys/00000000-0000-0000-0000-000000000000` with the
  stored sandbox pair returned **400 Bad Request**, but the positive control
  `GET /monitoring/logs/sources` with that same pair returned **401**. The
  credential was not authenticating at all, so the 400 says nothing about
  whether the pair is permitted on `/keys`. Recorded here rather than in
  `08-logs.md` precisely because it is not a finding.
- **To settle it:** store a known-good pair (`aic logs key set`), confirm
  `aic logs sources` → 200 as the control, mint a throwaway key, then try to
  delete **that** key with the pair. A 401/403 answers the question; a 404 on a
  nonexistent id after a green control answers it too.
- **Incidental:** the sandbox's stored `api_key_id` and the `API_KEY_SECRET` in
  `.envrc` no longer agree (or the key was rotated in the console) — that 401 is
  a live config problem, not a quirk.

### Q14. `config/managed` reads appeared to go backwards (2026-08-05)

- **Observed:** During CLI verification of `--default`, two throwaway objects
  lost properties that had already been confirmed written. `test_defaults3` was
  created and given `flag` then `tags`; a `GET` showed both. Roughly a minute
  later, with no write in between, `aic managed field edit test_defaults3.flag`
  failed `field 'flag' no longer exists`, and the next `field add` —
  read-modify-writing off that state — persisted an object holding only the new
  property. Same shape on `test_defaults4`. The `.aic/undo.log` write count
  matches the commands issued exactly, so `aic` made no phantom writes.
- **Not reproducible on demand.** The identical command sequence run
  back-to-back in one shell (`test_defaults5`: create → add → add → edit → edit
  → add) preserved every property, checked against the raw API after each step.
  A single write then polled every 5s for 70s stayed put. Twenty consecutive raw
  reads agreed with each other every time — no per-request flapping.
- **Reproduced the same day** by `scripts/experiment-managed-lost-updates.sh`,
  which removes the guesswork: create an object, add N fields one at a time,
  read the raw config after each.

  ```
  after add f1   ["f1"]
  after add f2   ["f2"]          <- f1 silently lost
  ...
  after add f8   ["f2".."f8"]
  settled (+10s) ["f2".."f7"]    <- f8 vanished with no write at all
  ```

  Every call returned 2xx. Two distinct failures are visible: a read backing
  write N returning the pre-write-(N-1) state (so the RMW discards it), and a
  property confirmed present immediately after its write being absent from a
  later read.

- **Not the daemon, and not a second writer.** The observing reads go straight
  to the tenant with `curl`, bypassing the `aic agent` that proxies the CLI's
  own calls, and they still show the loss. No other session was writing.
- **Independently observed in the AIC admin console** (Dave, 2026-08-05). That
  settles the layer: the console is a separate client doing its own
  read-modify-write against the same endpoint, so nothing about our transport,
  our daemon, or our request shape is implicated — it is the tenant's config
  store. It also means a user can lose a schema change made in the console, with
  no indication, which is worth knowing before blaming this tool.
- **The instantiation window is one cause, not the whole story.** Run without a
  wait after `object create` and the _first_ `field add` is the casualty every
  time — the new managed type is instantiated asynchronously and a write landing
  during that window does not survive. Waiting for the type to answer queries
  (9s) saved that first field, but f7 of 8 was still lost later in the same run.
- **This corrects `10-managed-objects.md`.** Its "config read-back is
  effectively immediate … strong consistency for the stored config" came from
  one ~164ms observation on 2026-06-14. `config/managed` is **not
  read-your-writes consistent**, and a 200 on the `PUT` does not mean the change
  is durable.
- **Consequence for tooling:** every `aic managed` write, and the admin
  console's equivalent, can silently discard a change it just made. A write path
  that cares has to re-read and confirm its own change landed, with a bounded
  retry, instead of trusting the status code. `aic` does not do this yet — see
  the note in `10-managed-objects.md`.
- **Confirm-after-write helps but does not close it (2026-08-05).**
  `api::replace_managed_confirmed` now PUTs, reads back, and retries up to six
  times (500ms–8s) until the change is visible, erroring instead of reporting
  success if it never becomes visible. That reliably fixes the deterministic
  case: the first `field add` after `object create`, previously lost 100% of the
  time, now survives every run. It does **not** make longer sequences safe — a
  14-field run still loses one or two writes that `aic` reported as successful.

  That residue is platform-side, and the reasoning matters: `aic` only returns
  success after reading the change back, and a stale read can return old data
  but cannot invent a property that was never written. So a field that was
  confirmed and is later absent _was_ stored. Either the document was rolled
  back afterwards, or the confirming read and the observing read hit replicas
  that disagree. Corroborating the rollback reading: **managed objects deleted
  hours earlier reappeared** in the config document (`test_defaults4`,
  `test_defaults5`), with `/environment/startup` reporting `ready` throughout,
  so this is not a restart.

- **Recommendation:** raise it with Ping. No client-side retry closes a
  rollback, and the same symptom is visible in their own console. Until then,
  treat a batch of managed schema writes as needing verification afterwards —
  the shape of that check is `scripts/experiment-managed-lost-updates.sh` — and
  prefer fewer, larger writes to many small ones.

- **Still open:** which of rollback or replica divergence it is. The behaviour
  is intermittent — short runs (8–10 fields) often pass cleanly — and sampling
  the agent-proxied and direct read paths side by side during a clean run showed
  no disagreement at all, so the simple divergence story does not obviously fit
  either.

### Q6. PUT on resources without `_rev`

- For ESV variables and scripts (both lack `_rev`), what is the server's
  conflict behaviour? Last-write-wins, presumably. Verify by deliberately racing
  two PUTs on a throwaway script.

### Q7. `X-Requested-With: XMLHttpRequest`

- frodo-lib sends this on all AM requests. Is it actually required (CSRF guard
  on PUT/POST/DELETE?), or just defensive? Test by omitting.

### Q8. PUT response shape

- For `PUT /am/json/.../scripts/{id}` — does the server echo the full object, or
  return a thin `{_id, _rev}`? Test on a throwaway script before implementing
  the Rust client (affects whether we need an extra GET after PUT to refresh the
  in-memory model).

### Q9. ESV ID prefix rules

- Does AIC enforce the `esv-` prefix on user-created variables/secrets? Test by
  `PUT /environment/variables/foobar` (no prefix) on a throwaway.

### Q10. Custom-domain URL form

- With a custom domain, do `/am/json/...` paths still need the
  `/realms/root/realms/{realm}` segment, or does the hostname imply the realm?
  Not testable in sandbox (no custom domain configured).

### Q13. IDM script engine — does `openidm.query` take a `fields` argument? (2026-07-03)

- Ping docs claim `openidm.query(resourceName, params, fields)` in IDM-side
  scripts (endpoint/schedule/hooks), but we have never exercised that arity. The
  workspace typings deliberately omit it (`idm/types/common.d.ts` `query` has no
  `fields`; the generated managed overloads match). Verify by pushing a custom
  endpoint script that calls
  `openidm.query("managed/alpha_user", {_queryFilter: "true"}, ["userName"])`
  and checking whether results are trimmed to the requested field. If verified,
  add `fields` to the IDM `query` fallback (conditional `managed/…` pattern, see
  nextgen-common.d.ts) and to `render_openidm_overloads` (Engine::Idm), and bump
  `TEMPLATES_VERSION`.

---

## Contradictions discovered during research (for future-Claude awareness)

| Claim                                                                             | Source                               | Reality (verified)                                                                                                                                                                        |
| --------------------------------------------------------------------------------- | ------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Script bodies are plain text                                                      | frodo-lib research summary           | False — base64 in both directions.                                                                                                                                                        |
| Scripts have `_rev`                                                               | frodo-lib research summary           | False — no `_rev` anywhere.                                                                                                                                                               |
| ESV path is `/environment/esv`                                                    | Ping docs research summary           | 404 — use `/environment/variables` and `/environment/secrets`.                                                                                                                            |
| Secret stores API works in AIC                                                    | both library summaries               | 403 "not available in PingOne Advanced Identity Cloud".                                                                                                                                   |
| Secret `PUT` is an upsert                                                         | 03-esvs.md (transcribed)             | False — create-only; re-PUT → 400 "already exists". Change value via new version.                                                                                                         |
| Version status `changestatus` accepts DESTROYED                                   | 03-esvs.md (transcribed)             | False — only ENABLED/DISABLED. DESTROYED is via `DELETE …/versions/{v}` (one-way).                                                                                                        |
| Secret version objects have `_id`                                                 | 03-esvs.md (transcribed)             | False — versions are a bare array of `{version, createDate, loaded, status}`.                                                                                                             |
| `useInPlaceholders` defaults to true                                              | 03-esvs.md (transcribed)             | False — required on create, no default, and immutable afterwards.                                                                                                                         |
| Journey `_rev` means use `If-Match`                                               | 09-journeys.md (earlier note)        | Superseded — `_rev` is content-derived, and plain `PUT` works for create/update. Use content snapshots, not `If-Match`.                                                                   |
| OAuth2 client `_rev` means use `If-Match`                                         | 05-oauth2-oidc.md (earlier note)     | False — plain `PUT` works for create/update, and `_rev` must be stripped from the body. Use content snapshots, not `If-Match`.                                                            |
| Managed-object `totalPagedResults` is exact with `_totalPagedResultsPolicy=EXACT` | 10-managed-objects.md (earlier note) | False — empty objects can report `NONE`/`-1`, and populated objects can report `ESTIMATE`. Use cursor cookies for bulk record reads; do not use offset paging as a completeness strategy. |
| Script `evaluatorVersion` defaults to `"2.0"` on create                           | 04-scripts.md (earlier note)         | False — omitting it yields `"1.0"` (legacy engine) on both create routes. Always send it explicitly.                                                                                      |
| Relationship `resourceCollection[].query` is cosmetic                             | 10-managed-objects.md (earlier note) | False — the API stores an entry without it, but the console then cannot open the owning object at all. Always emit `query`, on both ends of a pair.                                       |

**Lesson:** Both libraries' research summaries had errors. Always verify
endpoints + shapes against the live tenant before writing code.

The last three rows are our own notes, not transcribed claims — a live probe
that only exercises the API can still leave a wrong conclusion behind if the
console is the real consumer.

### Q11. PKCE browser flow — superseded (resolved 2026-05-20)

- **Status:** Superseded by Q12. Original investigation kept for posterity.
- **Original finding:** `idmAdminClient` rejects `http://localhost:*/callback`
  loopback redirect URIs, so we tried provisioning a dedicated `AicEdit` OAuth2
  client in the alpha realm. That worked for alpha-realm identities, but
  platform admins (the users who actually need to bootstrap pingone-aic-manager)
  live in the **root realm** in AIC, and AIC explicitly blocks root-realm OAuth2
  client management API
  (`403 "This operation is not available in PingOne Advanced Identity Cloud"`).
  DCR is exposed but rejects SA bearers. Device code is in
  `grant_types_supported` but no `device_authorization_endpoint` is advertised.
  → no path to a loopback PKCE flow for the actual users.
- **Replacement:** Three bootstrap patterns documented in Q12 below. The
  `AicEdit` client is no longer required and can be deleted from tenants where
  it was provisioned.

### Q12. Bootstrap auth flows (resolved 2026-05-20)

- **Status:** Resolved end-to-end; verified by
  `scripts/verify-pattern1-cookie.sh` and `scripts/verify-pattern2-userpass.sh`.

Three patterns produce an initial Bearer that can
`POST /openidm/managed/svcacct` to mint a long-lived service account. After
bootstrap, the SA's JWK takes over; the bootstrap credentials are discarded.

**Pattern 1 — Paste session cookie.** User logs into the AIC admin console in
their normal browser (full SSO/MFA/passkey/SAML stack). They copy the AM session
cookie value from DevTools → Application → Cookies. pingone-aic-manager drives
the OAuth flow server-side using the cookie.

**Pattern 2 — Username/password in-app.** pingone-aic-manager walks AM's
authentication journey via `POST /am/json/realms/root/authenticate`, handling
each callback round (NameCallback, PasswordCallback, ConfirmationCallback,
etc.). Works for username+password and TOTP. Does NOT work for
passkey/push/CAPTCHA — those require a real browser.

**Pattern 3 — Paste SA details.** User already has a service account JWK and
client_id. pingone-aic-manager stores them directly. (Same path as
`scripts/verify-endpoint.sh`.)

#### Pattern 1 wire details

1. `GET /am/json/serverinfo/*` → `cookieName` (per-tenant hex, e.g.
   `da4bb2cc51f31d3`). Do NOT hardcode; AIC randomises per tenant.
2. PKCE: `code_verifier` random URL-safe 32 bytes,
   `code_challenge = base64url(SHA256(verifier))`.
3. `GET /am/oauth2/realms/root/authorize` with:
   - `client_id=idmAdminClient`
   - `response_type=code`
   - `scope=openid fr:idm:*` _(other `fr:_` scopes rejected by this client — see
     scope rules below)\*
   - `redirect_uri=https://{TENANT_BASE}/platform/appAuthHelperRedirect.html`
     _(must be exactly this URL; same host as AM, NOT `id.forgerock.io`)_
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
   - **ConfirmationCallback**: hidden "Submit" button. Send the `defaultOption`
     value from the callback's `output` array (typically `0`). Do NOT prompt the
     user — this is the form's submit button, not a question.
   - **BooleanAttributeInputCallback** (e.g. "Trust this device"): default
     `false`.
   - **TextOutputCallback**, **HiddenValueCallback**: leave as-is.
   - **PollingWaitCallback**: passkey/push — abort with a clear message.
3. POST the modified callback array back to the same URL. AIC may return more
   rounds (HAR shows 3 rounds for u/p+TOTP). Loop until the response contains
   `tokenId`.
4. `tokenId` is the session cookie value. Continue with Pattern 1 step 3 (skip
   step 1 — we already minted a session, just need the cookie name).

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

- **2026-08-10** — **The short realm path forms do NOT 404.**
  `01-realms-and-paths.md` had said since 2026-05-17 "Do **not** use the short
  form `/realms/alpha` — it 404s", and `CLAUDE.md` §4 repeated it. Verified
  live: `/am/json/alpha/scripts`, `/am/json/realms/alpha/scripts` and the
  canonical `/am/json/realms/root/realms/alpha/scripts` **all** return 200 with
  an identical 121 results, and realm resolution is genuine (a bravo-only script
  id is 404 under the alpha short form, 200 under bravo; a nonexistent realm
  404s with a form-specific message, which is the control proving 200 is not a
  catch-all). The long form stays the project convention — one canonical
  spelling — but the stated reason was false, and code that treated the short
  form as broken was defending against nothing. The practical bite:
  `~/w/aic/who-changed` has been using `/am/json/$realm/scripts` successfully
  all along, and `am-access` audit records store `http.request.path` **as
  sent**, so a single window contains both spellings for one resource. Match
  audit paths on the resource id, never on a realm-path prefix. Documented in
  `01-realms-and-paths.md` and `04-scripts.md`.

- **2026-08-10** — **AM scripts report "no author" as the literal string
  `"null"`, not JSON `null`.** `04-scripts.md` described
  `createdBy`/`lastModifiedBy` as "null" on product-internal scripts, which read
  as JSON null. It is not: the wire value is the four-character string `"null"`,
  with `creationDate`/`lastModifiedDate` set to `0`. Across 405 scripts in
  alpha + bravo, **zero** JSON nulls appear in either field — all 810 values are
  typed `string` — and `"null"` accounts for 277 `createdBy` and 184
  `lastModifiedBy` values. `Option<String>` therefore yields `Some("null")` and
  any absence test passes it straight through to the display layer as if it were
  a DN. Two further wrinkles: the sentinel is **not** confined to product
  defaults (38 `default: true` and 42 `default: false` in alpha alone carry it),
  and it does not always coincide with a zero date — the two
  `ForgeRock Internal: …` scripts pair `createdBy: "null"` with a real 2015
  `creationDate`, so "author unknown" and "date unknown" are independent tests.
  Same field, third representation: on `description`, `"null"` (19 records) and
  genuine JSON `null` (11) both occur. Documented in `04-scripts.md`.

- **2026-08-10** — Internal roles have a **non-round-trippable read**: a bare
  `GET /openidm/internal/role/{id}` returns `temporalConstraints`, and `PUT`ting
  that value back — including the empty array the read itself produced — returns
  `403 "Policy validation failed"` naming the field as invalid on write.
  Isolated against a positive control: `condition` may be written back (200),
  `_id` and `_rev` may stay in the body (200), only `temporalConstraints` must
  be dropped (403 when retained). Naive read-modify-write on this collection
  therefore fails on the _first_ write, not subtly later.

  This corrects `18-internal-roles.md` for the third time in one day. The
  preceding correction had just replaced "use `_fields`" with "do a bare `GET`
  so you hold the whole object" — advice that is itself wrong for exactly this
  reason. Found by an implementation agent probing the body shape before writing
  the feature, not by review of the doc.

- **2026-08-10 (same day, self-correction)** — `18-internal-roles.md` was
  published hours earlier claiming internal roles have **no `_rev` and no
  conditional-write support**. Both are wrong. Reads carry a `_rev`, and
  `If-Match` is honoured: current revision → 200, superseded revision → **412**,
  `If-Match: *` → 200. Internal roles are therefore a conditional-write family
  in the `CLAUDE.md` §5 sense, and amend-and-write should send `If-Match`. The
  false claim came from generalising `config/access` — which genuinely has no
  `_rev` — onto internal roles a few paragraphs later in the same document,
  while probe output showing `_rev` was on screen. It was caught by an
  implementation agent that was told the doc was ground truth and stopped rather
  than coding around the contradiction; that instruction is the only reason it
  surfaced before shipping.

  Two further schema/reality divergences found while correcting it: `privileges`
  is returned on bare reads and bare list queries **despite**
  `returnByDefault: false` (whereas `authzMembers` does honour the flag), and
  `_fields=privileges` drops `name`/`description`, so feeding a projected read
  into a full-replace `PUT` erases them.

- **2026-08-10** — `GET /openidm/schema/internal/role` declares the privilege
  access-flags key as **`accessflags`** (lowercase `f`); the API requires
  **`accessFlags`**. A `PUT` using the schema's own spelling returns
  `403 "Policy validation failed"` with a `REQUIRED` policy requirement, and the
  role is **not** created (a follow-up `GET` 404s) — so this is loud rejection,
  not silent field loss. The admin console sends `accessFlags`, which is what
  round-trips. Trust the API over its published schema here. Full detail in
  `18-internal-roles.md`.

  Method note: the first read of this behaviour recorded it as "silently
  dropped", because the probe discarded the `PUT` status and only inspected the
  read-back. The correction came from re-running with the status visible. A
  related batch of seven privilege-field probes initially returned a uniform 403
  and was nearly written up as seven findings; adding a **positive control**
  showed the shared confound. Both are the same lesson as the 2026-08-07
  retractions: one observation is not a mechanism, and a batch without a control
  is not evidence.

- **2026-08-04** — Relationship `resourceCollection[].query` verified **required
  by the admin console** even though `config/managed` accepts an entry without
  it. A relationship created by `aic managed relationship set` on `test_from`
  round-tripped through the API cleanly and every runtime read succeeded (record
  list, `_fields`/`_queryFilter`/`_sortKeys` over the new property, and
  `GET /openidm/schema/managed/test_from` — all 200), but the console could no
  longer open `test_from` at all. Adding
  `"query": {"fields": [], "queryFilter": "true", "sortKeys": []}` to each
  `resourceCollection` entry restored it. This corrects our **own**
  `10-managed-objects.md` claim of 2026-07-27 that the console's extra fields
  are all cosmetic — that was verified by config PUT and read-back, i.e. against
  the API only, never against the console rendering the result. General lesson:
  for schema config, "the API stored it verbatim" is not the same as "the
  console can display it"; a shape the console writes deserves the benefit of
  the doubt. `build_relationship_node` in `src/managed/ops.rs` now always emits
  `query`, pinned by a test.
- **2026-07-31** — AM script `default` verified **server-owned on write**: a
  client-sent `"default": true` is silently dropped to `false` on `PUT`-create,
  `POST ?_action=create`, and `PUT`-update. Clients cannot mint an undeletable
  script or promote one to a product default. Same probe run showed
  `evaluatorVersion` defaults to **`"1.0"`**, not `"2.0"` — see the
  contradictions table. Updated `04-scripts.md`.
- **2026-07-15** — IDM schedule console visibility verified: a schedule with
  `persisted:false` can exist and run through the API while being hidden from
  the AIC console. Changing only `persisted:true` made it visible with
  `enabled:false` unchanged. Manual-only schedules that should be visible use
  `persisted:true, enabled:false`.
- **2026-07-15** — IDM manual schedule trigger header behaviour verified:
  `POST /openidm/scheduler/job/<name>?_action=trigger` returns 501
  `Not Implemented` when sent `Accept-API-Version: resource=1.0`, but the
  identical headerless call returns 200 `{"success":true}`. Updated
  `11-idm-endpoints.md`; scheduler-trigger curl examples must omit the header.
- **2026-05-17** — Initial verification pass; Q1-Q4 + secret stores resolved.
- **2026-05-18** — Step 2 implemented (TUI skeleton, crypto, onboarding). Q11
  (PKCE redirect URI) resolved; `AicEdit` OAuth2 client provisioned.
- **2026-05-20** — Q11 superseded by Q12. Three bootstrap patterns verified
  end-to-end via `scripts/verify-pattern1-cookie.sh` and
  `scripts/verify-pattern2-userpass.sh`. `AicEdit` OAuth2 client no longer
  required.
- **2026-05-30** — Full ESV **secrets** lifecycle verified live
  (create/version/changestatus/destroy/delete on throwaway `esv-aicedit-sec*`).
  Corrected four wrong transcribed claims in `03-esvs.md` (see contradictions
  table). Key facts: secret PUT is create-only; value changes go through
  versions; `useInPlaceholders` required + immutable and gates the restart
  (`false` ⇒ never pending); `changestatus` is ENABLED/DISABLED only; latest
  version can't be disabled; encodings `generic`/`pem`/`base64hmac`/`base64aes`
  with the last two double-base64-encoded.
- **2026-05-31** — Secret `setDescription` returns **`200` with a zero-byte
  body**, _not_ the echoed object the doc previously claimed (verified on
  throwaway `esv-secret-test1`). A strict `resp.json()` on the success body
  fails with "error decoding response body". Fixed generically in
  `AicClient::check_response`: an empty success body now maps to JSON `null`
  instead of erroring, so any empty-`200` write action is handled. Corrected the
  `03-esvs.md` table row.
- **2026-05-31** — IDM custom endpoints verified for the script-sync feature
  (full CRUD on throwaway `endpoint/aicedit-verify`): `PUT` create → 201, `PUT`
  replace → 200, `DELETE` → 200 (echoes object), `GET` after → 404. Key facts:
  **no `_rev`** (so content-based conflict detection, same as AM scripts),
  `source` is **plain text** (not base64, unlike AM scripts), **no
  `Accept-API-Version`** header required for `/openidm` config, list
  (`/openidm/config?_queryFilter=true`) is unfiltered (filter `endpoint/` ids
  client-side), no `name` field (name = `_id` suffix). New doc
  `11-idm-endpoints.md`. Also: added an optional per-call `Accept-API-Version`
  override to the agent `ApiCall` protocol so AM scripts can send
  `protocol=2.0,resource=1.0` while everything else keeps the `resource=1.0`
  default.
- **2026-06-01** — AM scripts list paging: `_queryFilter=true` returns **all**
  results when `_pageSize` is omitted (alpha 107, bravo 283 in one response).
  With `_pageSize` set, the response caps at that size but `pagedResultsCookie`
  comes back **`null`** — cookie paging silently truncates.
  `_pagedResultsOffset` + `remainingPagedResults` work, so `am::list` now pages
  by offset (PAGE=1000, stop when `remainingPagedResults == 0`). This replaces
  the earlier (buggy) cookie loop. `docs/api/04-scripts.md` updated.
- **2026-06-01** — IDM **scheduled jobs** added as a syncable script kind
  (`config/schedule/*`). Verified live: only `invokeService:"script"` schedules
  carry an inline script (at `invokeContext.script.source`);
  `taskscanner`/`sync` ones don't. Same `/openidm/config` CRUD as endpoints (no
  `_rev`, PUT 201/200, DELETE 200). A source-only push merges just
  `invokeContext.script.source` and round-trips the rest (cron `schedule`,
  `enabled`, `globals`). See `11-idm-endpoints.md`.
- **2026-06-10** — Managed-object create paths + "lock object" viability
  (investigating duplicate `reconById` objects). Verified live on throwaway
  `alpha_role` records: bare `PUT /managed/{t}/{id}` is an **upsert** (201
  create, **200 silent update** on repeat); `PUT … If-None-Match: *` is
  create-only (201/412); `POST ?_action=create` and
  **`openidm.create(container,id,content)`** are CREST `CreateRequest`s with
  **no update fallback** — duplicate → `PreconditionFailedException` "Entry
  Already Exists" (DJ entry `uid=…,ou=role,…`). `openidm.create` with `id=null`
  → server-assigned UUID (confirms it's POST/create, not PUT). **Key finding:**
  the `If-None-Match: *` lock is NOT reliable in clustered prod — Dave observed
  1×201 / 4×**200** / 15×412 for 20 concurrent same-`_id` PUTs; the 200s are
  silent updates (precondition not honored for those requests), so multiple
  callers get a 2xx and all think they hold the lock. Single-node sandbox can't
  reproduce (always 1×201 / N−1×412). Create-based paths avoid the 200-upsert
  mode but remain theoretically exposed to DS multi-master add-add replication
  conflicts. See `10-managed-objects.md`; repro harness
  `scripts/experiment-lock-uniqueness.sh`.
- **2026-06-17** — **Secret mappings** (ESV secret → AM secret label) verified
  live; new doc `15-secret-mappings.md`. **Corrects `07-secret-stores.md`**: the
  AM secret-store _collection_ API stays 403 (`nextdescendents`, bare
  `secrets/stores`, `secrets/types`, `global-config/secrets/**`), but the
  **per-store-type subpaths are open** with our `fr:am:*` SA token. Mappings
  live at
  `…/realm-config/secrets/stores/GoogleSecretManagerSecretStoreProvider/ESV/mappings`
  (`protocol=2.0,resource=1.0`), per realm (alpha+bravo each). Each mapping is
  `{secretId,aliases:[<one esv id>],_id,_rev}`; `_rev` content-derived; PUT body
  needs only `{"aliases":["…"]}` and (unlike trees/OAuth) tolerates
  `_id`/`secretId`. Store type allows **exactly one alias**
  (`400 "Only a single alias per mapping…"`); creating a mapping to an unbacked
  alias → `400 "Secret value is missing"`. Schema enum lists 190 valid labels
  but supplies **no descriptions** (`enumNames`/`enum_titles` repeat the raw id)
  — helper text is curated client-side from the label taxonomy (≈132
  per-OAuth2-client labels derivable structurally; ≈58 platform purposes
  curated). **Create/update/delete all verified (create→201, update→200,
  delete→200) — but the PUT body MUST include `secretId` (the label); a body of
  only `{"aliases":[…]}` fails with
  `400 "Invalid config: Secret value is missing"` every time. That 400 is NOT
  eventual consistency/value-staging (an earlier mis-diagnosis) — adding
  `secretId` to the body fixes it for any label/alias pairing.**
  pingone-aic-manager validates the alias is an existing ESV secret before
  writing (the API itself accepts any string, creating dangling mappings).
- **2026-06-24** — **Logs API (`08-logs.md`) partial live pass.** (1) Real
  source list captured (no `idm-recon`; includes `am-core`, `ctsstore*`,
  `userstore*`); `am-everything`/`idm-everything` are the roll-ups → CLI
  defaults. (2) `transactionId=<id>` is a **direct top-level query param** on
  `/monitoring/logs` (the working `gt` path), not `_queryFilter`. (3) **`/keys`
  is not SA-accessible at all.** `GET /keys` AND `POST /keys?_action=create` →
  **403 insufficient scope** with our SA — and _still_ 403 after granting the SA
  **all 13** `fr:idc:*` scopes (`analytics`, `telemetry`, `dataset`,
  `certificate`, `content-security-policy`, `custom-domain`, `promotion`,
  `release`, `sso-cookie`, `cookie-domain`, `esv` + `fr:am:*`/`fr:idm:*`;
  confirmed each made it into the token via `/am/oauth2/realms/root/tokeninfo`).
  Bearer is accepted (not 401), so the endpoint is gated to a scope **no
  service-account can hold** → log-key management needs an **admin-user token**
  (cookie/AppAuth), not an SA token. The doc's frodo-derived "SA mints log keys"
  claim is stale. Probed alternates: `/monitoring/logs/keys` → 401 (api-key
  family), `/environment/logs/keys` → 404, `/dashboard/logs/keys` → 500.
  **Design impact:** mint-on-demand only works while we hold an admin session
  (onboarding); default is paste console-created keys into the vault. **Admin
  path confirmed live (2026-06-24):** an admin-user bearer from `idmAdminClient`
  PKCE (scope `openid fr:idm:*`, no extra scope) does GET `/keys` (200), POST
  create (200 → `{name, api_key_id, api_key_secret, created_at}`, secret once),
  DELETE `/keys/{id}` (204). So Phase 1 auto-mints at cookie/userpass
  onboarding + the log-only env flow; SA-only tenants paste. (4) Couldn't get a
  200 from `/monitoring/logs` — the only key on hand (`<client-checkout>/logs/gt`) had
  been rotated (401). Auth model (api-key headers, not bearer) reconfirmed.
- **2026-07-01** — \*\*[SUPERSEDED 2026-07-01 (same day) — see the next entry
  - `08-logs.md`; the "corrected" key in this bullet is ALSO WRONG]\*\*
    **Journey-progress join key corrected (`08-logs.md`).** My own 2026-06-30
    taxonomy claim "node and tree events share `transactionId`; join on that" is
    **wrong**. Verified against `<client-checkout>/logs/prod-logs.json` (14,470 events;
    3,538 `AM-NODE-LOGIN-COMPLETED`, 138 `AM-TREE-LOGIN-COMPLETED`):
    `transactionId` is a **per-HTTP-request** id (`Root=1-…/0`, `…-request-2/0`)
    that differs within one journey execution. The correct grouping key is the
    **journey tracking UUID** = strip trailing `-<digits>` (`-\d+$`) from node
    `payload.trackingIds[0]` and from tree `payload._id`; both yield the same
    5-group UUID (3/3 attempts joined, 0 `treeName` mismatches), and it equals
    the `TrackingId` prefix in AIC's `Journey-Node-History` export. Tree event
    carries `result` (`SUCCESSFUL`→COMPLETED / `FAILED`→FAILED; node group with
    no tree event → ABANDONED), `principal[0]` (username) and `userId` (DN);
    node events carry no principal. Module/service-account logins
    (`AM-LOGIN-MODULE-COMPLETED`/`AM-LOGIN-COMPLETED`,
    `authIndex=module_instance`, no `treeName`) are OAuth2 client auth, not
    journeys — skip them. The pingone-aic-manager sandbox has only these module
    logins in the synced window, so journey rollup was verified against the client-a
    prod capture, not the sandbox.
- **2026-07-01 (later)** — **Journey join key RE-corrected — the same-day fix
  above was also wrong.** Stripping the trailing `-<digits>` and keying the tree
  event off `_id` (the fix in the prior bullet) MERGES thousands of executions:
  cross-checked against AIC's own `Journey-Node-History` export (146,159 rows),
  a single base UUID `a3c45e03-…` spans **3,226 distinct executions / 2,502
  users / multiple journeys** — the `<uuid>` prefix is an AM server/cluster
  instance id, not an execution. The symptom in `aic logs compact` was a
  rolled-up attempt with `node_count` 1,136 (a real login is ~20 nodes).
  **Correct key: the full `payload.trackingIds[0]`, verbatim, no stripping**;
  node AND tree events of one execution share it (tree `trackingIds[0]` ∈ node
  `trackingIds[0]` → 138/138; tree `_id` match → 0). Re-verified on
  `prod-logs.json`: 322 executions, median 19 nodes (max 49), 114 users, 8
  journeys, 127 SUCCESSFUL / 11 FAILED. Lesson: "3 attempts, 0 mismatches"
  looked like confirmation but was 3 giant merged blobs — validate the
  _cardinality and shape_ (nodes/execution, distinct users) against an
  independent source, not just internal self-consistency. `08-logs.md` join-key
  note rewritten.
- **2026-07-15** — IDM schedule scripts support root-level `const` and `let`,
  `new Set()` (`size`/`has`), and template literals. Verified in a disabled,
  manually triggered throwaway schedule that wrote the expected interpolated
  value with `openidm.create`; the schedule and record were removed afterward.
  **Exception:** a `const` declared inside a loop body parsed but silently
  terminated the loop after its first iteration (created sum 0, expected 3). The
  same `for (let ...)` loop without a body `const` completed correctly. Schedule
  scripts must use `let` for bindings declared in repeated loop bodies;
  root-level immutable bindings may remain `const`.
- **2026-07-15** — Managed-object naming convention corrected: `alpha_` and
  `bravo_` identify realm-owned data; they are not a blanket prefix for every
  custom object. Tenant-global service/configuration data should use a
  descriptive non-realm prefix, such as `idr_name_variants`. The earlier
  convention in `10-managed-objects.md` was too broad.
- **2026-07-31 (Q-enum, RESOLVED)** — Managed-object properties support an
  `enum` constraint, and it is a **constraint on a scalar, not a distinct
  property type**: the property keeps its `type` and gains a sibling `enum`
  array. Probed end-to-end against the sandbox with a throwaway
  `test_enum_probe` object (schema and records removed afterwards; the managed
  document was diffed back to byte-identical). All four originally-open
  questions answered:

  1. `options: { enum_titles: [...] }` **does** round-trip, positionally matched
     to `enum`.
  2. `enum` on an array's `items` **is** honoured and enforced.
  3. Numeric `enum` (`{"type":"number","enum":[1,2,3]}`) **is** accepted and
     enforced.
  4. The constraint **is enforced on record write**, not UI-only metadata: a
     value outside the set gives `403` / `"Policy validation failed"` with
     `policyRequirement: "VALID_ENUM_VALUE"` and `params.enumValues`.

  The operationally surprising part, and the reason this took a live probe
  rather than a doc read: **narrowing an enum on a populated field breaks
  read-modify-write but nothing else.** With a record still holding a removed
  value, `GET` returns 200 and a `PATCH` of an _unrelated_ field returns 200 —
  policy validates only the properties actually being written — but a `PUT` of
  the whole record exactly as it was read back returns 403 on the stale
  property. So the failure surfaces later, in whichever integration does
  read-modify-write, not at the point the schema changed. Widening is safe;
  narrowing needs the affected records migrated first. Full table in
  `10-managed-objects.md` → "Enum constraints".

- **2026-08-12** — **SAML circle-of-trust membership is stored in two places and
  the REST API exposes only one.** AM records "entity E is in CoT C" both in the
  CoT document's `trustedProviders` (what
  `realm-config/federation/circlesoftrust` returns) and in a `cotlist` attribute
  inside each entity's _extended_ metadata (never returned by
  `realm-config/saml2/{location}/{id64}`). **The runtime trust check reads the
  second one**: `SAML2Utils` resolves the hosted entity's `cotlist`, then looks
  up each named CoT _by name_ — it does not scan CoTs for the entity. So an
  entity with an empty `cotlist` rejects every assertion from its peer while the
  REST API reports a fully healthy CoT listing both parties.

  Found diagnosing a UAT `bravo` failure where `ClientBLogin` failed 15/15 with
  `Issuer in Response is invalid` while the structurally identical `ClientALogin`
  succeeded. Every REST-visible input matched: both entities present, entity IDs
  byte-identical to the assertion's `Issuer` (trailing slash and all), CoT
  `active` and listing both providers, SP and IdP documents diffed against the
  working pair and identical but for names. The fault was only visible in
  `am-core` debug logs, as an **absence**: the working ACS transaction resolves
  a CoT by name (`componentName = LIBCOT`, `configName = client-a`, `COTUtils`
  checking each provider); the failing one goes straight from reading the SP's
  entity config to `Issuer in Response is not valid` with no `LIBCOT` lookup at
  all — i.e. there was no CoT _name_ to resolve.

  Lessons worth keeping. **(1)** When every visible input matches and the
  outputs differ, the discriminating state is somewhere the API does not show
  you — go to the runtime logs rather than re-reading the same config harder.
  **(2)** The diagnosis was a _missing_ log line, which no amount of grepping
  for errors would have surfaced; it needed a known-good control captured under
  the same conditions (same tenant, realm, hour) and a line-by-line diff of the
  two traces. Finding that control cost one cheap query against the low-volume
  `am-authentication` source. **(3)** The tree-level error
  (`Saml2Node: AuthConsumer endpoint reported error code samlVerify …`,
  URL-encoded) is second-hand: the real verification runs in the browser's POST
  to `/am/AuthConsumer/…`, a separate transaction that `aic logs tx` will not
  reach and that `am-access` does not audit at all.

  Consequence for this project: **never present REST `trustedProviders` as the
  authoritative CoT membership**, and treat `PUT` on a CoT as unsafe until
  someone verifies it syncs the entity-side `cotlist` — writing one side and not
  the other manufactures exactly this failure. Same hazard on `importEntity`,
  which can drop a `cotlist` added after the entity was first imported.
  Documented in `06-saml.md`, which also carries the corrected object shapes
  (the pre-existing `serviceProvider: null` / `attributeQueryProvider` shape was
  library-research inference and is wrong) and the log signature for each case.

- **2026-08-12** — Logs API `_queryFilter` is barely usable:
  `payload/message co "…"` and `payload/logger eq "…"` both return **500
  Internal server error**, on a 20-second window as readily as a 6-hour one, so
  it is the filter and not the range being rejected. Only
  `payload/transactionId` is known to work. Any feature that selects log events
  by logger, level or message content must fetch the window and filter
  client-side. `08-logs.md` updated.

- **2026-08-12 (later)** — Follow-up to the CoT finding above:
  `SAML2Utils.verifyResponse` checks **`InResponseTo` against the CTS before it
  checks issuer trust**, and the ordering matters more than it sounds. After the
  `client-b` CoT was recreated, the next login still failed — but at the _earlier_
  gate, with `CTS: Token did not exist` /
  `InResponseTo attribute in Response is invalid`. The trust check was never
  reached, so the run says nothing about whether the fix worked. **A later gate
  cannot be confirmed by a run that fails at an earlier one**; the log just
  stops sooner and reads as "still broken".

  Cause of the second failure: the test **replayed a captured `AuthnRequest`**.
  AM stores an `AuthnRequestInfo` in the CTS when it issues a request and
  requires it at verification time; it is short-lived. Replaying yesterday's
  request gets a freshly minted, correctly signed `Response` from the IdP that
  echoes the _old_ request ID — the ID in the failing log was 7½ hours old and
  traceable to the previous day's capture. **A SAML capture is not a reusable
  test fixture**; federation changes must be tested with a fresh login. The tell
  is an `InResponseTo` you can find in an older capture.

  Second-order trap: a gate-2 failure also breaks the error path
  (`Saml2Proxy: getUrlWithError: Unable to determine AuthURL`), because the
  redirect target comes from the same missing per-request state. So the browser
  never returns to the tree, **no tree execution is recorded at all**, and the
  SDK's retry loop stops. The symptom change — "it stopped looping" — is not
  evidence of progress. Check which gate the ACS transaction reached. Gate table
  in `06-saml.md`.

- **2026-08-15** — OAuth2 client template vs schema, and PUT raw vs GET
  wrapped. The live `?_action=template` body still has 115 fields in six
  groups, but `?_action=schema` omits
  `advancedOAuth2ClientConfig.introspectionPolicySets` (present on the
  template as `[]` and on 9/45 alpha clients as
  `{"inherited":false,"value":[]}`). Catalogues that trust only the schema
  will reject those clients. Separately, a create `PUT` of the **raw**
  template (no `{inherited,value}` wrappers) returns 201; the subsequent
  `GET` wraps every field in the five non-override groups and leaves
  `overrideOAuth2ClientConfig` raw. Writers can send template-shaped raw
  values; readers must unwrap. Probe client
  `Terraform_oauth_probe_<ts>` was deleted. Documented in
  `05-oauth2-oidc.md`.

- **2026-08-15** — ESV ids are regex-enforced and `_pageSize` maxes at 100.
  `PUT /environment/variables/Terraform_esv-…` is 400
  (`^esv-[a-z0-9_-]{1,124}$`); `esv-terraform-…` is 200 and comes back
  `loaded:false` until a tenant restart, which we did not trigger. List
  `_pageSize=1000` is 400. Documented in `03-esvs.md`.

- **2026-08-15** — Concurrent `config/managed` writers drop each other's
  inserts. Two parallel GET-append-PUTs of `Terraform_test_from` and
  `Terraform_test_to` left only the latter; the first create had already
  confirmed. Serialising the RMW in-process recovered both; originals
  `test_from`/`test_to` were untouched. Documented in `10-managed-objects.md`.

- **2026-08-15** — Custom managed types accept the same lifecycle-hook
  shapes as Ping-shipped user/role objects, including a file-backed
  `onDelete` pointing at `roles/onDelete-roles.js`. A throwaway
  `Terraform_lifecycle_probe` stored inline copies of `alpha_user.onCreate`
  / `onUpdate` and `alpha_role.postCreate`; originals were byte-identical
  after the write. No probe records were created, so hook runtime was not
  re-fired. Inventory: only `*_user` and `*_role` carry hooks. Documented
  in `10-managed-objects.md`.

- **2026-08-15** — Schedule scripts live at two depths, and `taskscanner`
  schedules do have them. A read-only sweep of all six
  `config/schedule/*` documents in the alpha realm (captured 2026-08-14,
  committed as fixtures in `terraform-provider-pingone-aic`
  `internal/client/testdata/schedules/`) contradicts the note in
  `11-idm-endpoints.md` that only `invokeService: "script"` schedules carry
  an inline script: all three `taskscanner` documents hold 123–195 bytes of
  JavaScript at `invokeContext.task.script.source`, not
  `invokeContext.script.source`. A fourth value,
  `org.forgerock.openidm.script`, behaves as `script` and would be missed by
  an equality filter. Three of six emit `globals: {}` beside `source`, at
  whichever depth the script sits — a decoder that models the script as
  `{source, type}` and re-encodes deletes that key on the next
  whole-document write. Non-empty `globals` was **not** observed, so its
  value shape is still open. Observation, not a probe: nothing was written
  to the tenant. `11-idm-endpoints.md` corrected.
