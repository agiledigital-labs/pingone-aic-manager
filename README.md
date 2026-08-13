# pingone-aic-manager

A Rust + Ratatui **TUI** _and_ a `kubectl`-style **CLI** (`aic`) for managing
PingOne Advanced Identity Cloud (AIC, formerly ForgeRock Identity Cloud) tenant
configuration. One binary, two surfaces: run it with no arguments for the
interactive TUI, or with a subcommand for scripting. Both share an
`ssh-agent`-shaped background daemon that holds your decrypted service-account
key in memory and mints/refreshes bearer tokens on demand, so you authenticate
once and every tenant call goes through one path.

> **New here? Start with [Quick start](#quick-start), then the
> [CLI reference](docs/CLI.md).** For the living roadmap (done / in flight /
> next) see [PLAN.md](PLAN.md).

## What it does

Working today, via the CLI and (mostly) the TUI:

- **ESVs** — list, edit, and apply environment variables and secrets (full
  versioned-secret lifecycle).
- **Scripts** — two-way sync to a local **typed workspace** (`.d.ts` +
  ESLint/TypeScript) with a file watcher and **content-based** conflict
  detection. Covers AM scripts, IDM endpoints, scheduled jobs, and
  managed-object hooks.
- **IDM managed objects** — inspect the per-tenant schema (`aic managed`), and
  **sync records into a local SQLite store to query with SQL** (`aic idm`),
  including joins into nested arrays.
- **OAuth2 clients** — list, pull, push, delete.
- **Journeys** (auth trees) — list, pull/push as JSON, inspect node types.
- **Secret mappings** — re-point AM secret labels at ESV secrets.
- **Logs** — fetch, sync, search, compact, and roll up journeys from audit/debug
  logs.
- **Fast environment switching** with per-env theme colours (sandbox=green,
  development=blue, staging=yellow, production=red + ⚠) and an automatic
  **"you're writing to PROD" guard** on every mutation.

Planned / stretch: SAML 2.0, and log sync with compression + search for offline
history beyond AIC's 30-day retention.

**Why not [Frodo CLI](https://github.com/rockcarver/frodo-cli)?** Frodo is
excellent but command-line only, not interactive — and it runs its auth-callback
chain in-terminal, so it can't support WebAuthn / passkey 2FA, a hard
requirement for the maintainer.

## Install

**Ubuntu / WSL (recommended).** Download the latest prebuilt binary and drop it
in `~/.local/bin`:

```bash
curl -fsSL https://raw.githubusercontent.com/agiledigital-labs/pingone-aic-manager/main/install.sh | bash
```

Re-run the same command any time to **update** to the newest release. Useful
knobs:

```bash
# pin a version, install elsewhere, or force a source build
AIC_VERSION=0.1.0 curl -fsSL .../install.sh | bash
AIC_INSTALL_DIR=/usr/local/bin curl -fsSL .../install.sh | bash
curl -fsSL .../install.sh | bash -s -- --from-source
```

**From crates.io** (compiles locally; needs a Rust toolchain):

```bash
cargo install pingone-aic-manager   # installs the `aic` binary
```

Either way the command is **`aic`**. Prefer building from a checkout? See
[Build & run](#4-build--run) below.

> **Runtime dependency.** Security-key unlock uses `libudev` (via `hidapi`).
> It's present on stock Ubuntu/WSL; if a build or launch complains about it,
> install `libudev1` (runtime) or `libudev-dev` (to build from source):
> `sudo apt-get install -y libudev1`.

## Quick start

### 1. Get an AIC tenant + service account

You need a PingOne AIC tenant (sandbox is fine) and a service account with at
least these scopes:

```
fr:am:* fr:idm:* fr:idc:esv:* fr:idc:cookie-domain:*
```

Create one in the admin console: **Tenant Settings → Service Accounts → New
Service Account**. Save the JWK private key when prompted — it's shown only
once.

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

With [direnv](https://direnv.net), run `direnv allow`; otherwise `source .envrc`
in each shell.

### 3. Verify the tenant is reachable

```bash
scripts/verify-endpoint.sh                          # mints + caches a token
scripts/verify-endpoint.sh "/environment/variables"  # smoke test
```

First run bootstraps a Python venv at `.venv-tools/` (used only for JWT signing
in this bash helper — the Rust app does it natively).

### 4. Build & run

Requires Rust 1.85+ (Rust 2024 edition).

```bash
cargo build
cargo run                 # launch the TUI
cargo run -- --help       # CLI subcommands
cargo run -- session login # unlock the agent for this session
cargo run -- esv list     # …then start issuing commands
```

Once built, use the `aic` binary directly (`target/debug/aic`, or install it on
your PATH).

## Using the CLI

The CLI mirrors and extends the TUI. A taste across feature areas:

```bash
aic session status               # is the agent running/unlocked? which tenant?
aic ctx use development          # switch the active tenant context

aic esv list                     # environment variables
aic esv secret create esv-api-key  # versioned secret (no-echo prompt)

aic managed list                 # IDM managed-object schema
aic idm sync                     # pick objects → sync records into a local SQLite store
aic idm query "SELECT userName FROM obj_alpha_user WHERE accountStatus='active'"

aic script pull bravo/MyNode     # sync scripts to a typed local workspace, then edit + push
aic oauth list                   # OAuth2 clients
aic journey list                 # authentication trees
aic access list                  # IDM authorization rules (add/edit/rm/apply)
```

Mutations to a production-themed tenant require `--yes`; `--tenant <name>`
overrides the active context for one call. **Full command reference, flags, and
examples: [docs/CLI.md](docs/CLI.md).**

## The TUI

> The TUI is a **work in progress** — the CLI is the more complete surface.

Run `aic` with no arguments. On first launch the agent starts locked and you'll
see the **Unlock** screen (unless you set up with no master password). Press
**`Ctrl-P`** to open the **function selector** — a fuzzy-searchable modal that
switches between the feature views (ESVs, Scripts, Managed, Mappings, Access,
Query, OAuth). Within a view, `/` searches, `Ctrl-R` refreshes, and `?` shows
the keymap. Adding, editing and deleting individual authorization rules can be
done from the Access tab; `aic access` remains the surface for applying a whole
document from a file, addressing rules by digest, and removing several at once.
Design rules live in [docs/DESIGN.md](docs/DESIGN.md), which also states the
supported minimum terminal width.

## The agent

Every tenant call — TUI or CLI — goes through a small background process called
the **agent**, modelled on `ssh-agent`. You don't normally start it yourself;
the TUI and CLI spawn it automatically the first time they need it.

**Why it exists.** AIC bearer tokens expire after ~15 minutes, and minting a new
one needs your decrypted service-account private key. Rather than ask for your
master password on every action, the agent holds the decrypted key in memory and
mints/refreshes tokens on demand. You authenticate once; everything that follows
reuses the same unlocked agent — including a second TUI window or a CLI command
in another terminal, since they all share one agent.

**Locked vs. unlocked.** The agent is always one of two states:

- **Unlocked** — your key is decrypted in memory and it can mint tokens.
- **Locked** — it's running, but holds _nothing sensitive_: no keys, no tokens.
  Every request gets "locked" back until you log in again.

When you first launch, the agent starts locked and the TUI shows the **Unlock**
screen (or the CLI's `aic session login` prompts you). If your key is stored
unencrypted (you chose "no master password" during setup), the agent unlocks
itself and you're never prompted.

| Command              | What it does                                                           |
| -------------------- | ---------------------------------------------------------------------- |
| `aic session login`  | Unlock the agent (prompts for your master password).                   |
| `aic session logout` | **Lock** the agent — wipe keys + tokens from memory, leave it running. |
| `aic session stop`   | **Stop** the agent — shut the process down entirely.                   |
| `aic session status` | Show running/unlocked state, active tenant, and token expiry.          |

**Why `logout` doesn't kill the process.** This trips people up: `logout`
_locks_ the agent, it doesn't stop it. Locking already removes everything
sensitive from memory, so for security it's equivalent to killing it — but it
keeps the process (and its shared connection) alive, so the next
`aic session login` re-unlocks instantly instead of paying to spawn a fresh
process. Use `logout` when stepping away; use `stop` when you want the agent
gone.

**Auto-lock.** If left idle, the agent locks itself after **1 hour** by default
(same effect as `logout`). Override in `.aic/settings.toml` or with
`aic agent --idle-timeout <seconds>`.

**Where it lives.** A Unix socket at `.aic/agent.sock`, restricted to your user
(mode 0600). It's per-project: each checkout with its own `.aic/` gets its own
agent.

## Repo tour

| Path                                                                                         | What's in it                                                                                                                    |
| -------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| [`docs/CLI.md`](docs/CLI.md)                                                                 | **Full CLI reference** — every command, flag, and example.                                                                      |
| [`PLAN.md`](PLAN.md)                                                                         | Living roadmap: done / in progress / next.                                                                                      |
| [`docs/api/`](docs/api/)                                                                     | Verified AIC API reference. **Read before writing code that hits a tenant.** Each file has a "Verified against" date.           |
| [`docs/sharing-code-between-am-and-idm.md`](docs/sharing-code-between-am-and-idm.md)         | Pattern for sharing a small implementation between AM and IDM through a custom endpoint and thin AM library.                    |
| [`CLAUDE.md`](CLAUDE.md) / [`AGENTS.md`](AGENTS.md)                                          | Workflow rules for AI-assisted edits (docs-first, verify-before-update, credential hygiene; routing map in §9). Kept identical. |
| [`docs/DESIGN.md`](docs/DESIGN.md)                                                           | TUI design rules (palette, layout, keybindings).                                                                                |
| [`scripts/verify-endpoint.sh`](scripts/verify-endpoint.sh)                                   | Mints a token from `.envrc` and curls any AIC path — the verify-before-document loop.                                           |
| `src/aic/`, `src/agent/`                                                                     | HTTP core + the background daemon — the only path TUI/CLI use for tenant HTTP.                                                  |
| `src/esv/`, `src/scripts/`, `src/managed/`, `src/idmstore/`, `src/oauth/`, `src/journey/`, … | One directory per feature (api/state/ops/screen/view/cli seams). Routing map: CLAUDE.md §9.                                     |
| `src/app/`, `src/tui/`                                                                       | App shell (event loop, dispatch, prod guard, the `Ctrl-P` selector) and shared TUI chrome.                                      |
| `src/cli/mod.rs`                                                                             | CLI root: clap parser + session commands. Feature subcommands live in each vertical's `cli.rs`.                                 |

## Working on this codebase

Read [`CLAUDE.md`](CLAUDE.md) (AI assistants: it's also mirrored as
`AGENTS.md`). The three rules that matter most:

1. **Always read `docs/api/` before writing AIC API code.** Don't guess paths,
   headers, or API versions — the bootstrapping research had real errors caught
   only by live verification; `docs/api/` is the verified version.
2. **Verify before updating docs.** Use `scripts/verify-endpoint.sh` against a
   sandbox tenant, update `docs/api/{file}.md` with today's date, and note any
   contradiction in `docs/api/99-quirks-and-open-questions.md`.
3. **Credentials hygiene.** Never commit `.envrc`, `.env*`, `.token-cache`, any
   JWK or PEM, or any access token. `.gitignore` covers these.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at
your option. Unless you explicitly state otherwise, any contribution
intentionally submitted for inclusion in this project shall be dual licensed as
above, without any additional terms or conditions.
