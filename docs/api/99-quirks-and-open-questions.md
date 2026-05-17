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

**Lesson:** Both libraries' research summaries had errors. Always verify
endpoints + shapes against the live tenant before writing code.

---

## Changelog

- **2026-05-17** — Initial verification pass; Q1-Q4 + secret stores resolved.
