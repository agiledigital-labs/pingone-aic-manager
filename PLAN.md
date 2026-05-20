# aic-edit — Plan

Step 1 (research + verified API docs + cargo skeleton) and Step 2 (TUI
foundation: app skeleton, encryption, in-TUI tenant onboarding) are complete.
Step 3 (ESVs tab) is next.

## Step 2 status — complete

The plan in the previous revision of this file is mostly accurate for what got
built. The big deviations:

### Tenant onboarding — three verified patterns, no PKCE-to-loopback

The original "browser-handoff PKCE to localhost" flow turned out not to work
for AIC platform admins. The investigation is in
[`docs/api/99-quirks-and-open-questions.md`](docs/api/99-quirks-and-open-questions.md)
(Q11 / Q12). Headlines:

- Platform admins live in the **root realm**. AIC blocks the root-realm
  OAuth2-client-management API (`403 "not available in PingOne Advanced
  Identity Cloud"`), so we can't register a new client with a localhost
  redirect.
- The built-in `idmAdminClient` rejects localhost redirects.
- Device code grant is advertised in `grant_types_supported` but no
  `device_authorization_endpoint` is exposed.
- DCR is exposed but rejects SA bearers.

What we ship instead — three patterns proven end-to-end by
`scripts/verify-pattern1-cookie.sh` and `scripts/verify-pattern2-userpass.sh`:

1. **Paste session cookie** — user logs into the admin console in their real
   browser (full SSO/MFA/passkey/SAML stack), copies the AM session cookie name
   and value from DevTools, pastes both into aic-edit. aic-edit drives the
   OAuth2 PKCE flow server-side (the cookie carries the session;
   `idmAdminClient` returns a 302 with the code in the Location header which
   we intercept without following the redirect). The Bearer creates the SA
   via `/openidm/managed/svcacct`.
2. **In-app username + password** — aic-edit walks AM's authentication journey
   via `POST /am/json/{realm-path}/authenticate`, handling each callback
   round. Works for username/password + optional TOTP. Polling/push/passkey is
   rejected with a message pointing the user to pattern 1.
3. **Paste service-account details** — user already has an SA UUID + JWK
   somewhere; this is just a save-it-locally flow.

All three forms share a `TextField` widget
([`src/ui/widgets/text_field.rs`](src/ui/widgets/text_field.rs)) with prebuilt
factories in `text_field::fields` so a label change is a single edit.

### Master password — unified screen, no opt-in

The opt-in setup screen was dropped. There is now one unlock screen for both
first-run and subsequent launches. The first submission creates an empty
encrypted `keys.enc` as a verifier so that on next launch a wrong password is
actually rejected (previously, with no `keys.enc`, anything was accepted).

The unlock work runs on a `spawn_blocking` task so Argon2id+AES doesn't freeze
the UI; while it's in flight the password field reads "Unlocking…". On error
the field becomes editable again with the failure message below it.

### Other UX additions worth knowing

- **`Tenant hostname`** input rather than `Base URL`; on focus-leave we strip
  `https://`, `http://`, and any trailing path (e.g. `/am`) so users pasting
  a URL from frodo or curl just get the hostname.
- **Duplicate tenant name → overwrite confirm modal.** `persist_new_tenant` in
  `src/app.rs` checks for collisions and routes to `OverwriteConfirm` if it
  finds one.
- **Toasts wrap.** The Auth signing error was being clipped at 6 chars; toasts
  now grow to fit (up to 8 rows) and error toasts stick for 30 ticks. All
  background-task errors are also logged at error level
  (`~/.local/share/aic-edit/aic-edit.<date>.log`).
- **kid mismatch fix.** `header.kid` in `mint_token` now comes from the JWK
  itself (not `tenant.sa_id`), because the JWK we register with the SA uses a
  random UUID kid distinct from the SA UUID. Also
  `EncodingKey::from_rsa_der` wants PKCS#1, not PKCS#8 — we now call
  `to_pkcs1_der()`.
- **Textarea scroll-to-bottom + fixed-size mask.** Multi-line textareas scroll
  vertically so the cursor stays visible on overflow. Masked single-line
  fields render a fixed `head••••••••tail  (N chars)` summary so any-size
  paste fits.

## What's in the repo now

Files added since the original plan; the layout matches except for the
onboarding submodule and the new TextField widget:

```
src/
├── aic/onboard/
│   ├── mod.rs                # domain normalisation helpers + path enum
│   ├── bootstrap.rs          # shared OAuth2 + RSA + SA-create helpers
│   ├── cookie.rs             # Pattern 1 form state
│   ├── userpass.rs           # Pattern 2 form state + AM callback walker
│   └── paste.rs              # Pattern 3 form state
├── ui/
│   ├── unlock.rs             # single-field unlock screen with busy state
│   ├── onboard.rs            # menu + three single-page form draws
│   └── widgets/
│       ├── filtered_list.rs  # unchanged (used by Step 3+)
│       └── text_field.rs     # bordered SingleLine / Masked / TextArea
└── ...
```

Files removed: `src/ui/master_password.rs`, `src/aic/onboard/browser.rs`,
`src/aic/onboard/manual.rs`. `Settings` (encrypt-keys toggle) was dropped from
`src/config/mod.rs`.

## Verification (Step 2)

End-to-end smoke, sandbox tenant required (populate `.envrc`):

1. `cargo build` + `cargo clippy --no-deps` are green.
2. Fresh start (no `.aic-edit/`): app opens to the unlock screen with intro
   "Set a master password…". Enter password → spinner-less but visible busy
   state → Normal mode with empty-tenants welcome message.
3. `Ctrl-N` opens the add-tenant menu with three (or four if `.envrc` exists)
   options. Tab/Shift-Tab moves between fields in any form. Enter on Submit
   validates + persists or kicks off bootstrap.
4. Pattern 1: paste a fresh session cookie name + value from your admin
   console → SA created → token mints in background → "Token ready" toast.
5. Pattern 2: enter username + password (root realm) → if TOTP is set, an
   inline "Enter verification code" prompt appears; type code → SA created.
6. Pattern 3: enter SA UUID + JWK JSON → saved.
7. Quit + restart: the unlock screen accepts the same password (and rejects
   a wrong one). Wrong password no longer slides through when no tenants
   exist yet.
8. Adding a tenant with an existing name surfaces the `⚠ Tenant already
   exists` modal with `y`/`n` confirm.

## Step 3 — ESVs tab (next)

Goal: list / create / edit / delete environment-specific variables, plus the
"apply changes" restart flow. Read
[`docs/api/03-esvs.md`](docs/api/03-esvs.md) before coding.

### Surface

- New `Tab::Esvs` content (the existing placeholder body in `src/ui/mod.rs`
  is where this lives).
- Lists `/environment/variables` and `/environment/secrets` side by side or
  in two sub-tabs. Reuse `FilteredList` from `src/ui/widgets/filtered_list.rs`.
- Per-row actions: view (modal with the full value/details), edit (TextArea
  for variable; placeholder-set for secrets since they're write-only), delete.
- Top-level action: "Apply changes" → `POST /environment/startup?_action=restart`.
  Triggers the prod-confirm modal when tenant is themed prod.

### Wire details (from 03-esvs.md)

- Variables: `GET/PUT /environment/variables[/{id}]` — JSON value field.
- Secrets: `GET /environment/secrets`, `PUT /environment/secrets/{id}` with
  `valueBase64`. No GET-with-value — secrets are write-only.
- `lastChangeDate` is the staleness signal; **no `_rev`** so use content
  equality (or `lastChangeDate` if writes need optimistic locking).
- The `esv-` prefix is enforced by the server for user-created variables.

### Reuse from Step 2

- `AicClient::get / put / post / write` already handle bearer minting, the
  prod-confirm error, and `Accept-API-Version`. Don't reinvent — just call
  `client.write(method, path, body, confirmed_prod)` where appropriate.
- The error → toast pipeline (`AppEvent::Toast` / `AppEvent::TokenError`)
  is sized for full-length errors now; use it.
- `TextField` for the value editor (TextArea variant). Existing scroll-to-end
  behaviour will work; if you need cursor-position editing rather than
  append-only, that's a widget upgrade — flag it in the plan first.

### Not in Step 3

- Scripts tab. Step 4.
- OAuth2 / SAML / Journeys / managed-objects. Later steps.
- Log sync. Stretch.
- CLI subcommands.

## Risks / open items carried forward

- **No browser-handoff for headless / SSO-only admins.** Pattern 1 still
  requires the admin to log into the admin console manually once per session
  to copy the cookie. Acceptable for now; if a tenant wants automated
  rotation, see `docs/api/99` Q11/Q12 for options that weren't viable.
- **`keyring` on Linux** still depends on Secret Service running. Pattern 1
  / unlock both store the password best-effort; failures are silent.
- **RSA-2048 keygen** is fast enough not to need a progress indicator (the
  original plan was 4096, the implementation uses 2048 — see
  `bootstrap::generate_rsa_jwk`). If we want 4096 later, run it in
  `spawn_blocking` and show a busy state in the cookie/userpass forms.

## How to hand this off

The next agent should:

1. Read `CLAUDE.md` and `docs/api/README.md`.
2. Skim `docs/api/03-esvs.md` and `docs/api/99-quirks-and-open-questions.md`.
3. Run `scripts/verify-endpoint.sh "/environment/variables"` to confirm
   sandbox reachability before coding.
4. Build on the existing `AicClient` and `TextField` abstractions. Don't
   reach into `app.rs` for new input-mode plumbing without checking whether
   the existing `InputMode` enum + per-mode key handler pattern fits.
