# aic-edit

A Rust + Ratatui TUI **and** a `kubectl`-style CLI for managing PingOne
Advanced Identity Cloud (AIC, formerly ForgeRock Identity Cloud) tenant
configuration. Both surfaces share an `ssh-agent`-shaped background daemon
that holds decrypted service-account keys in memory and mints / refreshes
bearer tokens on demand, so every tenant call goes through one path.

**Status:** Step 1 + Step 2 + agent / CLI + ESV listing complete. Onboarding
(cookie / userpass / paste / sandbox-import) works end-to-end. Scripts /
OAuth2 / SAML / journeys are the next slices — see [PLAN.md](PLAN.md).

## What it does (target scope)

- **ESVs** — list, edit and apply environment variables and secrets.
- **Scripts** — two-way sync to a local directory with a file watcher,
  with **content-based** conflict detection (not `_rev`, which AIC doesn't
  return for scripts).
- **OAuth2 / OIDC** — manage clients and the provider service.
- **SAML 2.0** — manage hosted/remote entities and circles of trust.
- **Fast environment switching** with per-env theme colours
  (sandbox=green, development=blue, staging=yellow, production=red + ⚠) and an
  automatic "you're writing to PROD" guard on every mutation.
- **Stretch:** log sync with compression + search for offline history
  beyond AIC's 30-day retention.

Why not [Frodo CLI](https://github.com/rockcarver/frodo-cli)? Frodo is
excellent but command-line, not interactive. Frodo also implements its
auth-callback chain in-terminal, which means it can't support WebAuthn /
passkey 2FA — a hard requirement for the maintainer.

## Repo tour

| Path                                                       | What's in it                                                                                                                        |
| ---------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| [`PLAN.md`](PLAN.md)                                       | The approved Step 2 implementation plan. Start here.                                                                                |
| [`CLAUDE.md`](CLAUDE.md)                                   | Workflow rules for AI-assisted edits (docs-first, verify-before-update, credential hygiene).                                        |
| [`docs/DESIGN.md`](docs/DESIGN.md)                         | TUI design rules (palette, layout, keybindings).                                                                                    |
| [`docs/api/`](docs/api/)                                   | Verified AIC API reference. **Read before writing any code that hits a tenant.** Each file has a "Verified against" date for trust. |
| [`scripts/verify-endpoint.sh`](scripts/verify-endpoint.sh) | Mints a service-account access token from `.envrc` and curls any AIC path. Used to verify endpoints before documenting them.        |
| `src/agent/`                                               | The background daemon (Unix-socket protocol, AicClient cache, token mint).                                                          |
| `src/aic/api.rs`, `src/aic/esv.rs`                         | Surface-agnostic AIC helpers — **the only path** TUI/CLI use for tenant HTTP. New resources go here.                                |
| `src/cli/mod.rs`                                           | `aic` subcommands (login, status, ctx, esv list, …). Resource commands call `aic::api` / `aic::esv`.                                |
| `src/app.rs`                                               | TUI coordinator: state, key dispatch, ESV view, onboarding flows.                                                                   |
| `Cargo.toml`, `src/main.rs`                                | Single binary; no-args runs the TUI, any subcommand routes through `src/cli`.                                                       |

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

### 4. Build + run

```bash
cargo build
cargo run             # launches the TUI
cargo run -- --help   # CLI subcommands
cargo run -- agent    # foreground daemon (auto-spawned otherwise)
cargo run -- login    # unlock the agent for this session
```

The CLI mirrors the TUI's ESV capabilities (variables and secrets), talking to
the active tenant through the agent:

```bash
aic esv list                                  # variables
aic esv get  esv-my-var
aic esv set  esv-my-var --value hello --type string
aic esv delete esv-my-var
aic esv apply                                 # restart the runtime to apply

aic esv secret list
aic esv secret versions esv-my-secret
aic esv secret create esv-my-secret --value s3cret           # encoding generic
aic esv secret add-version esv-my-secret --value rotated
aic esv secret disable esv-my-secret 2
aic esv secret set-description esv-my-secret --description "…"
```

Mutating commands take `--yes` to confirm a write to a production-themed
tenant (the CLI equivalent of the TUI's prod-write guard), and `--tenant
<name>` to override the current context for a single call.

## The agent

Every tenant call — from the TUI or the CLI — goes through a small background
process called the **agent**, modelled on `ssh-agent`. You don't normally start
it yourself; the TUI and CLI spawn it automatically the first time they need it.

**Why it exists.** AIC bearer tokens expire after ~15 minutes, and minting a new
one needs your decrypted service-account private key. Rather than ask for your
master password on every action, the agent holds the decrypted keys in memory
and mints/refreshes tokens on demand. You authenticate once; everything that
follows reuses the same unlocked agent — including a second TUI window or a CLI
command in another terminal, since they all share one agent.

**Locked vs. unlocked.** The agent is always one of two states:

- **Unlocked** — your keys are decrypted in memory and it can mint tokens.
- **Locked** — it's running, but holds *nothing sensitive*: no keys, no tokens.
  Every request just gets "locked" back until you log in again.

When you first launch, the agent starts locked and the TUI shows the **Unlock**
screen (or the CLI's `aic login` prompts you). If your keys are stored
unencrypted (you chose "no master password" during setup), the agent unlocks
itself and you're never prompted.

**The commands:**

| Command      | What it does                                                              |
| ------------ | ------------------------------------------------------------------------- |
| `aic login`  | Unlock the agent (prompts for your master password).                      |
| `aic logout` | **Lock** the agent — wipes keys + tokens from memory, but leaves it running. |
| `aic stop`   | **Stop** the agent — shut the process down entirely.                      |
| `aic status` | Show whether it's running, unlocked, the active tenant, and token expiry. |

**Why `logout` doesn't kill the process.** This trips people up: `logout`
*locks* the agent, it doesn't stop it. The two are different on purpose. Locking
already removes everything sensitive from memory, so for security it's
equivalent to killing it — but it keeps the process (and its shared connection)
alive, so the next `aic login` re-unlocks everyone instantly instead of paying
to spawn a fresh process. Use `aic logout` when you're stepping away and want to
re-lock; use `aic stop` when you actually want the agent gone.

**Auto-lock.** If left idle, the agent locks itself automatically after **1 hour**
by default — the same effect as `aic logout`. Override the timeout in
`.aic-edit/settings.toml` or with `aic agent --idle-timeout <seconds>`.

**Where it lives.** The agent listens on a Unix socket at
`.aic-edit/agent.sock`, restricted to your user (mode 0600). It's per-project:
each checkout with its own `.aic-edit/` gets its own agent.

## Implementation status

| Step                                                            | Status      | Output                                                                                        |
| --------------------------------------------------------------- | ----------- | --------------------------------------------------------------------------------------------- |
| **1.** Research + verified API docs + cargo skeleton            | ✅ done     | `docs/api/`, `CLAUDE.md`                                                                      |
| **2.** TUI foundation, encryption, three-pattern onboarding     | ✅ done     | Unlock + onboarding screens, `keys.enc` / `wraps.toml` (master-pw + security-key envelope)    |
| **3.** Agent + CLI                                              | ✅ done     | Single-binary `aic`, Unix-socket protocol, `aic::api` + `aic::esv` shared between TUI and CLI |
| **4.** ESVs — list + fuzzy search + preview                     | ✅ done     | `/`-search with live scoring (nucleo), vertical split with JSON preview                       |
| **5.** ESVs — edit + apply (`/environment/startup?_action=…`)   | next        |                                                                                               |
| **6.** Scripts (two-way sync with content-based conflict check) | not started |                                                                                               |
| Later                                                           |             | OAuth2 / OIDC, SAML, Journeys, Logs, App.rs screen-split refactor                             |

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
