# aic-edit

A Rust + Ratatui TUI for managing PingOne Advanced Identity Cloud (AIC,
formerly ForgeRock Identity Cloud) tenant configuration.

**Status:** Step 1 complete (API research + verified docs + cargo skeleton).
Step 2 (TUI implementation) is planned but not yet started — see
[PLAN.md](PLAN.md).

## What it does (target scope)

- **ESVs** — list, edit and apply environment variables and secrets.
- **Scripts** — two-way sync to a local directory with a file watcher,
  with **content-based** conflict detection (not `_rev`, which AIC doesn't
  return for scripts).
- **OAuth2 / OIDC** — manage clients and the provider service.
- **SAML 2.0** — manage hosted/remote entities and circles of trust.
- **Fast environment switching** with per-env theme colours
  (sandbox=green, dev=blue, staging=yellow, prod=red + ⚠) and an
  automatic "you're writing to PROD" guard on every mutation.
- **Stretch:** log sync with compression + search for offline history
  beyond AIC's 30-day retention.

Why not [Frodo CLI](https://github.com/rockcarver/frodo-cli)? Frodo is
excellent but command-line, not interactive. Frodo also implements its
auth-callback chain in-terminal, which means it can't support WebAuthn /
passkey 2FA — a hard requirement for the maintainer.

## Repo tour

| Path | What's in it |
|------|--------------|
| [`PLAN.md`](PLAN.md) | The approved Step 2 implementation plan. Start here. |
| [`CLAUDE.md`](CLAUDE.md) | Workflow rules for AI-assisted edits (docs-first, verify-before-update, credential hygiene). |
| [`docs/DESIGN.md`](docs/DESIGN.md) | TUI design rules (palette, layout, keybindings). |
| [`docs/api/`](docs/api/) | Verified AIC API reference. **Read before writing any code that hits a tenant.** Each file has a "Verified against" date for trust. |
| [`scripts/verify-endpoint.sh`](scripts/verify-endpoint.sh) | Mints a service-account access token from `.envrc` and curls any AIC path. Used to verify endpoints before documenting them. |
| `Cargo.toml`, `src/main.rs` | Compilation skeleton (stub binary; `cargo check` passes). |

## Local setup

### 1. Get an AIC sandbox tenant + service account

You need a PingOne AIC tenant (sandbox is fine) and a service account on it
with at least these scopes:

```
fr:am:* fr:idm:* fr:idc:esv:* fr:idc:cookie-domain:*
```

Create one via the AIC admin console: **Tenant Settings → Service
Accounts → New Service Account**. Save the JWK private key when prompted —
it's only shown once.

### 2. Create `.envrc` (gitignored)

```bash
export TENANT_BASE_URL=https://<your-tenant>.forgeblocks.com
export REALMS='["alpha", "bravo"]'
export SERVICE_ACCOUNT_ID=<service-account-uuid>
export SERVICE_ACCOUNT_SCOPE='fr:idm:* fr:am:* fr:idc:esv:* fr:idc:cookie-domain:*'
export SERVICE_ACCOUNT_CLIENT_ID=service-account
export SERVICE_ACCOUNT_KEY='{
  "kty": "RSA", "n": "...", "e": "AQAB", "d": "...",
  "p": "...", "q": "...", "dp": "...", "dq": "...", "qi": "..."
}'
```

If you use [direnv](https://direnv.net), run `direnv allow`. Otherwise
`source .envrc` each shell.

### 3. Verify your tenant is reachable

```bash
scripts/verify-endpoint.sh                              # mints + caches a token
scripts/verify-endpoint.sh "/environment/variables"      # smoke test
```

First run bootstraps a Python venv at `.venv-tools/` (used only for JWT
signing in the bash helper — the Rust app does this natively).

### 4. Build

```bash
cargo build       # compiles the stub
cargo run         # prints a placeholder; Step 2 implementation hasn't started
```

## Implementation status

| Step | Status | Output |
|------|--------|--------|
| **1.** Research + verified API docs + cargo skeleton | ✅ done | `docs/api/`, `CLAUDE.md`, stub `src/main.rs` |
| **2.** App skeleton, encryption, in-TUI tenant onboarding | 📋 planned (see [PLAN.md](PLAN.md)) | Working tab strip, env chrome, "add tenant via browser or paste" flow, encrypted local store |
| **3.** ESVs tab — list, edit, apply | not yet planned | |
| **4.** Scripts tab — two-way file sync with content-based conflict detection | not yet planned | |
| Later | OAuth2 / OIDC, SAML, Journeys, Logs, Yubikey unlock | |

## How to work on this codebase (for AI assistants)

Read [`CLAUDE.md`](CLAUDE.md). The three rules that matter most:

1. **Always read `docs/api/` before writing AIC API code.** Don't guess
   paths, headers, or API versions. The research that bootstrapped this
   project contained real errors that were caught only by live verification;
   `docs/api/` is the verified version.
2. **Verify before updating docs.** Use `scripts/verify-endpoint.sh` against
   a sandbox tenant. Update `docs/api/{file}.md` with today's date. If
   observed behaviour contradicts the doc, trust observation and add a
   dated note in `docs/api/99-quirks-and-open-questions.md`.
3. **Credentials hygiene.** Never commit `.envrc`, `.env*`, `.token-cache`,
   any JWK or PEM, or any access token. The `.gitignore` covers these.

## License

To be decided. Repo is currently public for collaboration; treat as
all-rights-reserved until a license file is added.
