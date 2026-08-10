# pingone-aic-manager — instructions for Claude

`pingone-aic-manager` is a Rust + Ratatui TUI for managing PingOne Advanced
Identity Cloud (AIC, formerly ForgeRock Identity Cloud) tenant configuration:
ESVs, scripts, OAuth2/OIDC, SAML, with fast environment switching and (stretch)
log sync+search. The dev sandbox is in `.envrc`.

## 1. Always read `docs/api/` before calling AIC

Before generating any code that hits an AIC endpoint:

1. Open the relevant `docs/api/{file}.md`. Don't guess paths, headers, or API
   versions. The library research we did to bootstrap this project contained
   real errors (see `docs/api/99-quirks-and-open-questions.md`); the verified
   docs are the source of truth.
2. If the doc says "verified against … 2026-05-17" you can trust it. If the doc
   says "open question" or "not yet exercised", verify with the script below
   before writing code.

`docs/api/README.md` is the index.

## 2. Keep `docs/api/` current — but only after verification

When you discover a new endpoint, new field, new header requirement, or
non-obvious behaviour:

1. **Verify with a live call.** Run:

   ```bash
   scripts/verify-endpoint.sh "<path>" [--header "<...>"]
   ```

   The script takes a bearer from the running agent (`aic whoami --token`, for
   the tenant in the current context) and `curl`s the path with it. **The agent
   must be unlocked — run `aic login` first**, or the script exits 3 rather than
   waiting on a password prompt.

   If verification is impossible for any reason, say so and stop. Do **not**
   write to a `## Verified against` block from inference, from figures supplied
   in a task prompt, or from a claim in a neighbouring doc — that block is the
   audit trail, and a plausible-but-wrong entry stamped "verified" is invisible
   to every later reader.

2. **Update the relevant `docs/api/*.md`** file. Bump "Verified against" to
   today's date. Add the endpoint to the table.
3. **If observed behaviour contradicts a doc**, trust observation. Update the
   doc. Add a dated note in `docs/api/99-quirks-and-open-questions.md` so
   future-you knows which library/source was wrong.
4. **Never** transcribe a frodo-lib, fr-config-manager, or Ping docs claim
   without verifying it. They have stale claims today (Q1 — script encoding, Q2
   — ESV paths, secret stores availability).

## 3. Credentials hygiene — non-negotiable

- **Never commit** `.envrc`, `.env*`, `.token-cache`, `*.jwk`, `*.pem`,
  service-account secrets, log API key secrets, or any access token. The
  `.gitignore` covers these; never override with `git add -f` on them.
- `.envrc` holds `AGENT_PASSWORD` (the vault master password) and
  `API_KEY_SECRET` (the log API secret). Treat both as secret even though the
  tenant is a sandbox. The service-account JWK used to live here too; it now
  lives in the encrypted vault, and nothing outside `src/` should need it.
- **Tokens stay in memory**, everywhere — no disk caching. `.token-cache` is a
  leftover from when `verify-endpoint.sh` signed its own assertion; the script
  now borrows the agent's in-memory bearer and writes nothing. Delete the file
  if you still have one; it stays in `.gitignore` as a guard.
- Log API uses **separate** `x-api-key` / `x-api-secret` — these are
  console-issued and `api_key_secret` is shown only once at creation.

## 4. Realm path convention

All realm-scoped AM URLs use `/realms/root/realms/{realm}`:

```
/am/json/realms/root/realms/alpha/scripts?_queryFilter=true
```

Never use `/realms/alpha` (short form) — it 404s. ESVs, logs, and IDM managed
config have no realm in the path. Full table in
`docs/api/01-realms-and-paths.md`.

## 5. Conflict-detection rule (for the script-sync feature)

The user explicitly wants: _compare script content, not `_rev`_. Rationale:
revision drift doesn't matter if the content is back to what we have locally.

For each script we sync, store the **last-synced remote content** (decoded
bytes) locally. Before pushing a local change:

1. `GET` the remote script and base64-decode the `script` field.
2. If `decoded(remote) == decoded(last_synced_cache)`, push the local change
   (overwrite is safe — content matches what we forked from).
3. Otherwise, remote has drifted; surface a 3-way diff (`last_synced` ↔
   `remote` ↔ `local`) and prompt the user.
4. Update the cache after any successful pull or push.

Scripts have **no `_rev`** anyway (verified 2026-05-17 — see
`docs/api/04-scripts.md`), so this is the only viable algorithm. The same rule
applies to ESV variables (also no `_rev`).

For resources that DO have `_rev`, use content snapshots for the same "revert
detection" reason. Only send `If-Match: <_rev>` for API families verified to
support conditional writes. OAuth2 clients and journeys have `_rev` but were
verified 2026-06-14 to use plain `PUT` without `If-Match`; strip `_rev` from
their write bodies and ignore it in content comparisons.

## 6. Tokens

- TTL: 898 seconds. Refresh ≥60s before expiry, proactively.
- Single endpoint: `POST /am/oauth2/access_token` (root, no realm segment).
- `client_id=service-account` (fixed string).
- See `docs/api/00-auth.md` for the full JWT shape.

## 7. Development workflow

```bash
# Once per shell (direnv handles this automatically if installed):
source .envrc

# Unlock the agent — verify-endpoint.sh borrows its bearer:
aic login

# Sanity-check tenant connectivity:
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

For script-template work (`src/scripts/templates/`), the runtime ground truth is
`scripts/rhino-script-tester/` — paired probe fixtures run against a live
journey. New syntax/binding claims get a fixture pair before they get a lint
rule or a doc row.

## 8. Things to NOT do

- **Don't run tenant-touching `aic` commands from an agent without
  `--no-prompt`.** A locked daemon must fail fast instead of waiting for a
  master password the agent cannot provide.
- **Don't add a "Secret Stores" UI tab.** AIC returns 403 on the entire
  secret-stores API. Use ESVs instead. (`docs/api/07-secret-stores.md`.)
- **Don't send `-encrypted` fields back on OAuth2 client `PUT`.** They contain
  cluster-local AES-wrapped values; round-tripping corrupts secrets. Strip any
  key ending in `-encrypted` from the body. (`docs/api/05-oauth2-oidc.md`.)
- **Don't trust `creationDate` / `lastModifiedDate` types are consistent.**
  Scripts use epoch-ms ints; ESVs use ISO-8601 strings. Don't assume.
- **Don't try to create new realms.** AIC only allows `alpha` + `bravo` + root.
- **Don't poll `/environment/startup?_action=restart` aggressively** — rate
  limits are tighter than the read endpoints.
- **Don't expect `src/agent/` code changes to take effect while an agent is
  running — and treat this as an upgrade hazard, not just a testing one.**
  `aic session logout` only _locks_ the daemon; the old binary stays resident.
  Run `aic session stop`, then relaunch, before testing agent changes **and
  after upgrading `aic`**. A new CLI talking to a resident old daemon used to
  fail in whatever way that particular change happened to break — verified
  2026-08-06, when a daemon 5 days old replied to one request and closed the
  connection, giving the next request a broken pipe. Protocol version 1 now
  turns a detectable mismatch into a message naming `aic session stop` as the
  remedy; it does not provide compatibility.
- **Don't edit `src/scripts/templates/` without bumping `TEMPLATES_VERSION`** in
  `src/scripts/workspace.rs` — otherwise scaffolded workspaces never receive the
  update.
- **Don't mutate a managed schema outside `managed::ops`'s `apply_*`
  transforms.** They carry key normalisation, availability checks, `FieldCaps`
  capability gating, the relationship-rename refusal and the enum-narrowing
  gate. A new caller that builds its own JSON gets none of it, and the CLI has
  no visual confirmation step to catch the difference. If a transform can't
  express what you need, widen the transform.
- **Don't narrow a managed-field `enum` without the caller's consent flag.**
  Dropping an allowed value breaks whole-record updates for records still
  holding it — later, elsewhere, on a property that code never touched
  (`docs/api/10-managed-objects.md`). Adding a value and clearing the constraint
  are both widening and need no gate.

## 9. Project layout — routing map

Updated 2026-08-01; the feature-vertical restructure is **complete** (rationale:
`docs/orthogonality-review.md`). One directory per feature, with uniform seams:
`api` (HTTP), `state`, `ops` (background work), `screen` (key handling + nested
Mode/Event), `view` (rendering), `cli`. Pull only the rows you need into
context.

A feature that has both a tab and CLI verbs needs a seventh seam: a `spec.rs` of
plain input types with no TUI state, so `cli.rs` and the tab drive the same
transforms instead of each building its own request. `src/managed/` is the
worked example.

| To change…                                                                                                                             | Code lives in                                                                                                                                                                                                                                                                     | Read first                                                   |
| -------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| ESV variables                                                                                                                          | `src/esv/`                                                                                                                                                                                                                                                                        | `docs/api/03-esvs.md`                                        |
| ESV secrets                                                                                                                            | `src/secrets/` (HTTP wrappers stay in `esv/api.rs` — same API family)                                                                                                                                                                                                             | `docs/api/03-esvs.md`                                        |
| ESV secret mappings (AM secret label → ESV secret)                                                                                     | `src/secretmap/` (surfaced as the ESVs tab's "ESV secret mappings" sub-view; sandbox/development only)                                                                                                                                                                            | `docs/api/15-secret-mappings.md`                             |
| IDM sync mappings (browse `config/sync` mappings, reconcile)                                                                           | `src/mappings/` (TUI-only; the Mappings tab) — script pull/push for embedded mapping scripts is `aic script … sync/<mapping>.<slotpath>` via `src/scripts/sync_mapping.rs`                                                                                                        | `docs/api/16-sync-mappings.md`                               |
| Journeys (auth trees: list/pull/push/delete, node-type introspection)                                                                  | `src/journey/` (CLI only — no TUI tab yet)                                                                                                                                                                                                                                        | `docs/api/09-journeys.md`                                    |
| IDM internal roles (caller-chosen ids, role CRUD, managed-object privileges)                                                         | `src/roles/` (CLI only — no TUI tab)                                                                                                                                                                                                                                              | `docs/api/18-internal-roles.md`                              |
| Trusted JWT Issuer setup (per-tenant signing key, issuer CRUD/show)                                                                    | `src/jwtbearer/` (CLI only — no TUI tab yet)                                                                                                                                                                                                                                      | `docs/api/17-jwt-bearer-user-tokens.md`                      |
| Script sync (pull/push/sync/watch/diff)                                                                                                | `src/scripts/` (one module per `Kind`: `am`, `idm`, `schedule`, `managed_hooks`, `sync_mapping`)                                                                                                                                                                                  | `docs/api/04-scripts.md`, `11`, `12`, `13`, `16`             |
| Script workspace templates (lint/types)                                                                                                | `src/scripts/templates/` + `TEMPLATES_VERSION` in `src/scripts/workspace.rs`                                                                                                                                                                                                      | `docs/api/12-script-bindings-matrix.md`                      |
| Managed-object schema: browse **and edit** (Managed tab + `aic managed` writes); hooks sync as `managed/<obj>.<hook>` via `aic script` | `src/managed/` — `spec.rs` holds the TUI-free input specs (`FieldEditSpec`, `AddFieldSpec`, `EnumChange`, …), `ops.rs` the pure `apply_*` transforms that both the tab and `cli.rs` call. Hook sync is `src/scripts/managed_hooks.rs`.                                            | `docs/api/10-managed-objects.md`                             |
| IDM managed-object record store + query                                                                                                | `src/idmstore/`                                                                                                                                                                                                                                                                   | `docs/api/10-managed-objects.md`                             |
| Logs (fetch, key mgmt, local DuckDB sync/search/compact + journey rollup)                                                              | `src/logs/` (CLI only — no TUI tab yet); log-KEY STORAGE rides the same vault path as the SA JWK: `src/config/` (`log-keys.enc`/`log-keys.plain` read/write), `src/agent/` (vault secret verbs), `src/vault/` (`unlock.rs`/`auth.rs` load the decrypted map into `App` on unlock) | `docs/api/08-logs.md`, `docs/logs-store.md`                  |
| OAuth2 clients                                                                                                                         | `src/oauth/`                                                                                                                                                                                                                                                                      | `docs/api/05-oauth2-oidc.md`                                 |
| Tokens / HTTP transport / daemon                                                                                                       | `src/aic/` (transport core), `src/agent/`                                                                                                                                                                                                                                         | `docs/api/00-auth.md`, `01`, `02`; `src/agent/mod.rs` header |
| Local credential vault / unlock                                                                                                        | `src/vault/` + `src/config/{crypto,wraps}.rs` storage                                                                                                                                                                                                                             | — (local-only, no AIC docs)                                  |
| Operator identity (name/host resolution + persistence)                                                                                 | `src/config/operator.rs` (`Settings` shape/storage in `src/config/mod.rs`; CLI prompt/settings/whoami in `src/cli/mod.rs`)                                                                                                                                                        | `docs/CLI.md`                                                |
| Onboarding (add tenant)                                                                                                                | `src/onboard/`                                                                                                                                                                                                                                                                    | `docs/api/00-auth.md`, `99-…` Q11/Q12                        |
| Undo log + history overlay                                                                                                             | `src/undo/` (executors live in each feature's `ops`)                                                                                                                                                                                                                              | —                                                            |
| App shell: event loop, mode/tab dispatch, prod guard, env picker                                                                       | `src/app/`                                                                                                                                                                                                                                                                        | —                                                            |
| Shared TUI chrome: widgets, theme, header, toasts, modals, help                                                                        | `src/tui/`                                                                                                                                                                                                                                                                        | `docs/DESIGN.md`                                             |
| CLI root (clap, login/logout/stop/status, ctx, settings, whoami)                                                                       | `src/cli/mod.rs` (feature subcommands live in each vertical's `cli.rs`)                                                                                                                                                                                                           | `docs/CLI.md`                                                |

### Adding a new feature (e.g. OAuth2)

1. Verify + document the API first (§2); new `docs/api/` file if needed.
2. Create `src/<feature>/` with the standard seams; state hangs off `App` as one
   field; `screen.rs` owns a nested `Mode` + `Event` enum.
3. Register exactly one arm in each global: `app::InputMode::<Feature>(Mode)`,
   `app::event::AppEvent::<Feature>(Event)`, `app::keymap::dispatch`,
   `app::draw::draw`, `cli::Command` (+ a variant in `app::mod::View` and its
   refresh arm in `app::selector::refresh` if it's a tab — note it's
   `enum View`, not `enum Tab`).
4. Add the routing row above and a `mod.rs` header linking the API doc.
5. If you added or changed a subcommand, update `docs/CLI.md`. Feature-internal
   changes (new modal, new background op) must touch only the feature directory
   — if you find yourself editing `src/app/` for one, the design is being
   violated.

## 10. When unsure

- **Default to reading `docs/api/`** rather than searching the web. The web
  sources we already mined had errors (Q1, Q2 in 99-…); our verified docs win.
- **If the docs don't cover it, verify before coding.** Use
  `scripts/verify-endpoint.sh` and update the relevant file.
- **If the user asks for a feature not yet in `docs/api/`** (e.g. themes, email
  templates, audit), do a verification pass first and add a new doc file before
  writing implementation code.
