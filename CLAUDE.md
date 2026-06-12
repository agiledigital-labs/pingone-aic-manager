# aic-edit — instructions for Claude

`aic-edit` is a Rust + Ratatui TUI for managing PingOne Advanced Identity Cloud
(AIC, formerly ForgeRock Identity Cloud) tenant configuration: ESVs, scripts,
OAuth2/OIDC, SAML, with fast environment switching and (stretch) log
sync+search. The dev sandbox is in `.envrc`.

## 1. Always read `docs/api/` before calling AIC

Before generating any code that hits an AIC endpoint:

1. Open the relevant `docs/api/{file}.md`. Don't guess paths, headers, or API
   versions. The library research we did to bootstrap this project contained
   real errors (see `docs/api/99-quirks-and-open-questions.md`); the verified
   docs are the source of truth.
2. If the doc says "verified against … 2026-05-17" you can trust it. If the
   doc says "open question" or "not yet exercised", verify with the script
   below before writing code.

`docs/api/README.md` is the index.

## 2. Keep `docs/api/` current — but only after verification

When you discover a new endpoint, new field, new header requirement, or
non-obvious behaviour:

1. **Verify with a live call.** Run:
   ```bash
   scripts/verify-endpoint.sh "<path>" [--header "<...>"]
   ```
   The script mints a service-account access token (cached in `.token-cache`,
   gitignored), then `curl`s the path with `Authorization: Bearer …`. First
   run bootstraps a Python venv at `.venv-tools` for JWT signing.
2. **Update the relevant `docs/api/*.md`** file. Bump "Verified against"
   to today's date. Add the endpoint to the table.
3. **If observed behaviour contradicts a doc**, trust observation. Update the
   doc. Add a dated note in `docs/api/99-quirks-and-open-questions.md` so
   future-you knows which library/source was wrong.
4. **Never** transcribe a frodo-lib, fr-config-manager, or Ping docs claim
   without verifying it. They have stale claims today (Q1 — script encoding,
   Q2 — ESV paths, secret stores availability).

## 3. Credentials hygiene — non-negotiable

- **Never commit** `.envrc`, `.env*`, `.token-cache`, `*.jwk`, `*.pem`,
  service-account secrets, log API key secrets, or any access token. The
  `.gitignore` covers these; never override with `git add -f` on them.
- The JWK in `.envrc` is the production RSA private key for the sandbox
  service account. Treat as secret even though it's a sandbox.
- **Tokens stay in memory** during the Rust app's runtime — no disk caching
  in production code. (`.token-cache` is a dev-only convenience for the
  bash verify script.)
- Log API uses **separate** `x-api-key` / `x-api-secret` — these are
  console-issued and `api_key_secret` is shown only once at creation.

## 4. Realm path convention

All realm-scoped AM URLs use `/realms/root/realms/{realm}`:

```
/am/json/realms/root/realms/alpha/scripts?_queryFilter=true
```

Never use `/realms/alpha` (short form) — it 404s. ESVs, logs, and IDM managed
config have no realm in the path. Full table in `docs/api/01-realms-and-paths.md`.

## 5. Conflict-detection rule (for the script-sync feature)

The user explicitly wants: *compare script content, not `_rev`*. Rationale:
revision drift doesn't matter if the content is back to what we have locally.

For each script we sync, store the **last-synced remote content** (decoded
bytes) locally. Before pushing a local change:

1. `GET` the remote script and base64-decode the `script` field.
2. If `decoded(remote) == decoded(last_synced_cache)`, push the local change
   (overwrite is safe — content matches what we forked from).
3. Otherwise, remote has drifted; surface a 3-way diff
   (`last_synced` ↔ `remote` ↔ `local`) and prompt the user.
4. Update the cache after any successful pull or push.

Scripts have **no `_rev`** anyway (verified 2026-05-17 — see
`docs/api/04-scripts.md`), so this is the only viable algorithm. The same rule
applies to ESV variables (also no `_rev`).

For resources that DO have `_rev` (OAuth2 clients, journeys, OIDC provider,
SAML entities, CoT), use both: send `If-Match: <_rev>` AND keep a
content-snapshot fallback for the same "revert detection" reason.

## 6. Tokens

- TTL: 898 seconds. Refresh ≥60s before expiry, proactively.
- Single endpoint: `POST /am/oauth2/access_token` (root, no realm segment).
- `client_id=service-account` (fixed string).
- See `docs/api/00-auth.md` for the full JWT shape.

## 7. Development workflow

```bash
# Once per shell (direnv handles this automatically if installed):
source .envrc

# Sanity-check tenant connectivity (also primes the token cache):
scripts/verify-endpoint.sh

# Hit an endpoint to inspect a shape:
scripts/verify-endpoint.sh "/environment/variables"

# Build / run / verify:
cargo check
cargo test            # unit tests are co-located in the modules they test
cargo fmt
cargo run             # no args → TUI; subcommands → CLI (see `aic --help`)
```

For TUI work, follow the visual + interaction rules in `docs/DESIGN.md`
(borderless panels, tally-style tabs, semantic colors). Don't redebate them.

For script-template work (`src/scripts/templates/`), the runtime ground
truth is `scripts/rhino-script-tester/` — paired probe fixtures run against a
live journey. New syntax/binding claims get a fixture pair before they get a
lint rule or a doc row.

## 8. Things to NOT do

- **Don't add a "Secret Stores" UI tab.** AIC returns 403 on the entire
  secret-stores API. Use ESVs instead. (`docs/api/07-secret-stores.md`.)
- **Don't send `-encrypted` fields back on OAuth2 client `PUT`.** They contain
  cluster-local AES-wrapped values; round-tripping corrupts secrets. Strip
  any key ending in `-encrypted` from the body. (`docs/api/05-oauth2-oidc.md`.)
- **Don't trust `creationDate` / `lastModifiedDate` types are consistent.**
  Scripts use epoch-ms ints; ESVs use ISO-8601 strings. Don't assume.
- **Don't try to create new realms.** AIC only allows `alpha` + `bravo` + root.
- **Don't poll `/environment/startup?_action=restart` aggressively** —
  rate limits are tighter than the read endpoints.
- **Don't expect `src/agent/` code changes to take effect while an agent is
  running.** `aic logout` only *locks* the daemon — the old binary stays
  resident. Run `aic stop`, then relaunch, before testing agent changes.
- **Don't edit `src/scripts/templates/` without bumping
  `TEMPLATES_VERSION`** in `src/scripts/workspace.rs` — otherwise
  scaffolded workspaces never receive the update.

## 9. Project layout — routing map

Updated 2026-06-13. A feature-vertical restructure is in progress
(plan + status: `docs/orthogonality-review.md`); this table tracks **current
reality** and must be updated as each phase lands. Pull only the rows you
need into context.

| To change… | Code lives in | Read first |
|---|---|---|
| ESV variables | `src/screens/esv.rs`, ESV rendering inline in `src/ui/mod.rs`, `src/aic/esv.rs`, `aic esv` in `src/cli/mod.rs` | `docs/api/03-esvs.md` |
| ESV secrets | `src/screens/secret.rs`, `src/ui/secret.rs`, `aic esv secret` in `src/cli/mod.rs` | `docs/api/03-esvs.md` |
| Script sync (pull/push/sync/watch/diff) | `src/scripts/` (engine, screen, view, CLI) | `docs/api/04-scripts.md`, `11`, `12`, `13` |
| Script workspace templates (lint/types) | `src/scripts/templates/` + `TEMPLATES_VERSION` in `src/scripts/workspace.rs` | `docs/api/12-script-bindings-matrix.md` |
| Tokens / HTTP transport / daemon | `src/aic/api.rs`, `src/aic/auth.rs`, `src/agent/` | `docs/api/00-auth.md`, `01`, `02`; `src/agent/mod.rs` header |
| Local credential vault / unlock | `src/screens/{unlock,auth_setup,auth_settings}.rs` + `src/ui/` twins, `src/security_key.rs`, `src/config/{crypto,wraps}.rs` | — (local-only, no AIC docs) |
| Onboarding (add tenant) | `src/aic/onboard/`, `src/screens/onboard.rs`, `src/ui/onboard.rs` | `docs/api/00-auth.md`, `99-…` Q11/Q12 |
| Undo | `src/undo.rs`, `src/screens/undo_history.rs`, `src/ui/undo_history.rs` | — |
| TUI look & feel / keybindings | `src/ui/`, `src/theme.rs`, `src/keymap.rs` | `docs/DESIGN.md` |
| CLI plumbing (clap, ctx, login) | `src/cli/mod.rs` | — |

Global registration points that **every new feature** must touch (one arm
each): `app::InputMode`, `event::AppEvent`, `keymap::dispatch`, `ui::draw`,
`cli::Command`.

## 10. When unsure

- **Default to reading `docs/api/`** rather than searching the web. The web
  sources we already mined had errors (Q1, Q2 in 99-…); our verified docs
  win.
- **If the docs don't cover it, verify before coding.** Use
  `scripts/verify-endpoint.sh` and update the relevant file.
- **If the user asks for a feature not yet in `docs/api/`** (e.g. themes,
  email templates, audit), do a verification pass first and add a new doc
  file before writing implementation code.
