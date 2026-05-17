# Step 2 — Foundation: app skeleton, encryption, in-TUI tenant onboarding

Step 1 (research + verified API docs + cargo skeleton) is complete. See
[CLAUDE.md](CLAUDE.md) and [docs/api/](docs/api/) for that output. This file
is the approved plan for Step 2.

## Context

We're starting the TUI implementation for `aic-edit` (Rust + Ratatui, managing
PingOne AIC tenants). Step 2 is **foundation only** — what every later
capability needs: app skeleton, async event loop, tenant config + encryption,
in-TUI onboarding, tab/env chrome. The first capability tab (ESVs) is
deferred to Step 3 so this step doesn't sprawl.

Four requirements shape the design:

1. **Project-local config, not global.** The user has multiple concurrent AIC
   projects and `cd`s between them. Config lives in `./.aic-edit/` in the
   current working directory.
2. **In-TUI tenant onboarding** — like Frodo CLI's `frodo conn save`, but
   one better: support **WebAuthn/passkey 2FA via browser handoff**. Frodo's
   in-terminal callback chain can't do this; the user has accounts where
   passkey is the *only* second factor. Also keep a **manual override** path
   for restricted environments (WSL is one).
3. **Encryption at rest.** Master password → Argon2id → AES-256-GCM around
   service-account JWKs and other secrets. Optional OS keychain remember to
   skip the password on subsequent launches per machine. Yubikey deferred.
4. **Prod-write confirm.** Any mutation on a tenant themed `prod` triggers
   a "You're writing to PROD — Are you sure?" modal.

## Architecture

- **Async tokio + Ratatui.** Required because we make HTTP calls. Pattern: a
  background task drives `crossterm::event::EventStream` + tick/render
  intervals via `tokio::select!`, sending unified `AppEvent`s through an
  `mpsc::unbounded_channel`. Background HTTP runs in `tokio::spawn` tasks
  that send their result as another `AppEvent`. `JoinHandle::abort()` for
  cancellation when the user navigates away.
- **`ratatui::init()` / `ratatui::restore()`** (0.30 idiom) — handles raw
  mode, alternate screen, and panic hook.
- **Inherit `tally`'s structure** (the user's existing personal-finance TUI
  at `~/w/tally` — referenced patterns):
  central `App` struct + `InputMode` enum for modal dispatch + per-tab state
  + `FilteredList<T>` + `try_mutation`/`load_or_show` patterns + XDG-aware
  `logging.rs`. The big departure: tally is sync, we're async; mutations
  dispatch background tasks and react to `AppEvent::ApiResponse(...)`.

## Module layout

```
src/
├── main.rs                     # tokio runtime; ratatui::init/restore; calls app::run
├── lib.rs                      # re-exports
├── error.rs                    # thiserror — port from tally
├── logging.rs                  # XDG file logging — port from tally
├── event.rs                    # AppEvent enum + EventHandler
├── app.rs                      # App struct + run loop with tokio::select!
├── config/
│   ├── mod.rs                  # ProjectConfig: load/save .aic-edit/config.toml
│   ├── tenant.rs               # Tenant struct (name, base_url, sa_id, scopes, theme)
│   └── crypto.rs               # Argon2id + AES-256-GCM wrapper; keys.enc read/write
├── keychain.rs                 # `keyring` wrapper for "remember unlocked key per machine"
├── aic/
│   ├── mod.rs                  # AicClient — reqwest wrapper with auto-bearer + Accept-API-Version
│   ├── auth.rs                 # JWK → EncodingKey; TokenCache; mint_token
│   ├── onboard/
│   │   ├── mod.rs              # high-level "add tenant" entry points
│   │   ├── manual.rs           # paste mode — parse user input → Tenant
│   │   └── browser.rs          # OIDC PKCE flow: localhost server + browser open + SA create
│   └── svcacct.rs              # POST /openidm/managed/svcacct?_action=create
├── theme.rs                    # 4 env themes (sandbox/dev/staging/prod); chip styles
└── ui/
    ├── mod.rs                  # top-level draw() dispatcher
    ├── header.rs               # tab strip + realm chip + env chip
    ├── toast.rs                # top-right toast stack
    ├── modal.rs                # generic modal helpers (centered_rect, etc.)
    ├── unlock.rs               # master password prompt screen
    ├── onboard.rs              # add-tenant modal (path picker + forms)
    ├── env_picker.rs           # env switcher modal
    └── widgets/
        └── filtered_list.rs    # port from tally
```

## Subsystem detail

### Project-local config

`./.aic-edit/` in cwd:

```
.aic-edit/
├── config.toml         # non-secret: tenant index, default tenant, theme overrides
├── keys.enc            # AES-256-GCM ciphertext of the JWK map; gitignored
├── .gitignore          # auto-written: keys.enc, local-config/, *.log
└── local-config/       # script-sync target (Step 4+); gitignored
```

`config.toml` shape:

```toml
project = "my-aic-project"
default_tenant = "sandbox"

[[tenant]]
name = "sandbox"
base_url = "https://<your-tenant>.forgeblocks.com"
theme = "sandbox"
sa_id = "<service-account-uuid>"
scopes = ["fr:idm:*", "fr:am:*", "fr:idc:esv:*", "fr:idc:cookie-domain:*"]

[[tenant]]
name = "prod"
base_url = "https://<your-tenant>.forgeblocks.com"
theme = "prod"
sa_id = "<service-account-uuid>"
scopes = [...]
```

Service-account JWKs live separately, encrypted, in `keys.enc`. Decrypted
shape:

```json
{ "sandbox": { "kty": "RSA", "n": "...", "e": "AQAB", "d": "...", ... },
  "prod":    { "kty": "RSA", ... } }
```

First-run convenience: if `./.aic-edit/` doesn't exist but `./.envrc` does
and looks like ours (has `SERVICE_ACCOUNT_KEY`), offer to import it as a
"sandbox"-themed tenant — keeps the existing dev loop working unchanged.

### Encryption

- Master password → `argon2::Argon2` (Argon2id, m=64MiB, t=3, p=4) with a
  16-byte random salt stored alongside ciphertext → 32-byte key.
- `aes-gcm::Aes256Gcm` with random 12-byte nonce per encrypt.
- File layout (binary): `magic(4) | version(1) | salt(16) | nonce(12) |
  ciphertext(...) | tag(16)`.
- `keyring` crate for "remember": after first successful unlock, offer to
  store the derived key in OS keychain (Secret Service / Keychain / Cred
  Manager). Subsequent launches try keychain first, fall back to prompt.
- Re-key on master password change: re-encrypt, re-store in keychain.

### Async event loop

`src/event.rs`:

```rust
pub enum AppEvent {
    Key(crossterm::event::KeyEvent),
    Tick,
    Render,
    TokenMinted { tenant: String, expires_at: i64 },
    TokenError  { tenant: String, error: String },
    ApiResponse { request_id: u64, result: Result<serde_json::Value, ApiError> },
    OnboardCallback(Result<OauthCode, String>),  // from localhost server
    Toast(ToastKind, String),
}

pub struct EventHandler {
    pub tx: mpsc::UnboundedSender<AppEvent>,
    rx: mpsc::UnboundedReceiver<AppEvent>,
    _task: tokio::task::JoinHandle<()>,
}
```

`src/app.rs` main loop:

```rust
loop {
    terminal.draw(|f| ui::draw(f, &app))?;
    if app.should_quit { break; }
    if let Some(ev) = app.events.recv().await {
        app.handle_event(ev).await?;
    }
}
```

### Tab strip + env chrome

- Header: tabs left (just `ESVs` shown in Step 2; more added per step), realm
  chip + env chip right-aligned.
- `R` toggles realm; `T` opens env picker modal; `Ctrl-N` opens "Add tenant"
  modal.
- `theme.rs`: returns `(fg, bg, glyph)` for each theme.
  `sandbox`: green/black/`▪`, `dev`: blue/black/`▪`, `staging`: yellow/black/`▪`,
  `prod`: white/red/`⚠`.

### Tenant onboarding

`Ctrl-N` opens a modal with three choices:

1. **Paste service account JSON** (works everywhere, including WSL).
   - Form: name, base URL, theme picker, paste-area for SA UUID, paste-area
     for JWK private key. Validates JWK shape + checks the base URL is
     reachable + mints a test token. Success → encrypt + persist.
2. **Log in via browser** (the headline feature; needs localhost reachability).
   - Form: name, base URL, theme picker.
   - aic-edit binds `127.0.0.1:0` (random port), opens the user's browser via
     `webbrowser` crate to `{base_url}/am/oauth2/authorize?…&redirect_uri=
     http://127.0.0.1:{port}/cb` using OIDC PKCE.
   - User authenticates in browser (whatever 2FA — passkey, TOTP, push, SSO).
   - Browser redirects to the localhost callback with `?code=…&state=…`.
   - Localhost server captures the code, exchanges for an access token at
     `/am/oauth2/access_token`, sends `OnboardCallback(Ok(token))` to App.
   - App uses that token to: generate a local RSA-4096 JWK pair (takes
     ~10–30s; show progress), `POST /openidm/managed/svcacct?_action=create`
     with `name`, `description`, `accountStatus=active`, `scopes=[…]`,
     `jwks=JSON.stringify({keys:[<public_jwk>]})`. Response gives back the
     SA UUID. Encrypt + persist the **private** JWK.
3. **Import from .envrc** (convenience, shown only when `./.envrc` exists).

**Browser-handoff open question (flag in plan, resolve in implementation):**
the OAuth client we use needs `http://127.0.0.1:*/cb` whitelisted as a valid
redirect URI. Frodo uses `idmAdminClient` (built-in to AIC) but with a
`/platform/...` redirect, not localhost. We need to verify whether
`idmAdminClient` accepts a localhost redirect; if not, we'll need a
documented one-time setup step ("create a public OAuth client in your AIC
admin console with redirect URI `http://127.0.0.1:*/cb`") to make
browser-handoff work. Manual paste mode always works regardless.

### Prod-write confirm pattern

`AicClient::write(method, path, body)` checks `tenant.theme == Prod`; if so,
returns `Err(ApiError::ProdConfirmRequired)` unless caller passes
`ConfirmedProdWrite` token. UI catches that error, raises a centered modal
("You're writing to PROD — confirm? (y/n)"), and on `y` retries with the
token. Stays consistent across tabs without per-tab logic.

Step 2 exercises this on the SA-creation call when adding a prod-themed
tenant — first real prod write the app makes.

### Master password unlock UX

- Launch: if `.aic-edit/keys.enc` exists, look in OS keychain
  (`keyring::Entry::new("aic-edit", &project_path)`) for cached key.
  - Found and decrypts → straight to main UI.
  - Not found / decrypt fails → show unlock screen, prompt for master password,
    derive + decrypt + (optionally) store in keychain.
- Launch with no `.aic-edit/`: skip straight to "no tenants — Ctrl-N to add"
  state.
- First "Add tenant" with no existing store: also prompt to set master
  password.

## Critical files to be created (Step 2)

- `src/main.rs`, `src/lib.rs`, `src/error.rs`, `src/logging.rs`, `src/event.rs`,
  `src/app.rs`
- `src/config/{mod,tenant,crypto}.rs`
- `src/keychain.rs`
- `src/aic/{mod,auth,svcacct}.rs`, `src/aic/onboard/{mod,manual,browser}.rs`
- `src/theme.rs`
- `src/ui/{mod,header,toast,modal,unlock,onboard,env_picker}.rs`
- `src/ui/widgets/filtered_list.rs`

Plus `Cargo.toml` additions (see below).

## Reused from tally (~/w/tally on the user's machine)

Port these verbatim with minimal adjustments. Tally is the user's existing
Ratatui app and the closest match in style + structure to what aic-edit will
be. If you don't have access to that source tree, the patterns below are
described in [docs/DESIGN.md](docs/DESIGN.md) at a high level:

- `error.rs` — thiserror enum + `Result<T>` alias.
- `logging.rs` — XDG file path, daily rotation, env-var level.
- `FilteredList<T>` widget (from tally's `tui/filtered_list.rs`) — generic,
  well-tested.
- `try_mutation` / `load_or_show` patterns — adapted to async (return Future).
- The `InputMode` enum approach for modal dispatch.

## Cargo.toml additions

```toml
# Encryption
argon2          = "0.5"
aes-gcm         = "0.10"
rand            = "0.8"
zeroize         = { version = "1", features = ["derive"] }

# OS keychain
keyring         = "3"

# Browser handoff
webbrowser      = "1"
hyper           = { version = "1", features = ["server", "http1"] }
hyper-util      = { version = "0.1", features = ["tokio"] }
http-body-util  = "0.1"
url             = "2"

# JWT — explicit pure-Rust backend (v10 requires this)
jsonwebtoken    = { version = "10", default-features = false, features = ["rust_crypto"] }

# RSA / PKCS1 / numbers (for JWK ↔ EncodingKey conversion)
num-bigint-dig  = "0.8"     # rsa v0.9 uses this
pkcs1           = { version = "0.7", features = ["pem"] }

# Misc
uuid            = { version = "1", features = ["v4", "serde"] }
chrono          = "0.4"
toml            = "0.8"
directories     = "5"       # XDG for log file location only
```

## Verification

End-to-end smoke for Step 2 (sandbox tenant required; populate `.envrc` per
the SETUP section in `README.md`):

1. `cargo check` passes; `cargo build` passes.
2. In a directory with no `.aic-edit/` but with `.envrc` → app shows
   "no tenants yet" + offers `i` to import .envrc + `Ctrl-N` to add.
3. Import .envrc → master password prompt → set password → tenant appears,
   `.aic-edit/` directory created, `keys.enc` 600 perms.
4. App mints a token in the background (visible in top-right toast).
   Token cache reuse on second mint (no network).
5. `R` toggles realm; chip updates color (alpha and bravo both dim grey).
6. `T` opens env picker with the one tenant; selecting it does nothing yet
   (just confirms switching works).
7. **Paste-mode onboarding:** `Ctrl-N` → choose paste → fill form with the
   .envrc values under a different name → save → it appears in env picker.
   Master password is not re-asked.
8. **Browser-handoff onboarding:** `Ctrl-N` → choose browser → enter a new
   tenant name and the sandbox base URL → browser opens to AIC login →
   user authenticates → returns to TUI with success toast. SA visible in
   AIC admin console under "Service Accounts".
9. **Prod confirm:** add the same tenant a third time with theme=prod. Try
   to add — confirm modal pops, asks "you're writing to PROD". `y` proceeds.
10. Restart the app: keychain unlock works silently. Delete the keychain
    entry → master password prompt returns; correct password proceeds.

## Out of scope (Step 3+)

- **ESVs tab** — list + edit variables + apply restart. Step 3.
- **Scripts tab** with two-way file sync + content-equality conflict
  detection. Step 4.
- **OAuth2 / SAML / Journeys tabs.** Later.
- **Yubikey** for master-key unlock. Later.
- **Log sync + compression + search.** Stretch.
- **CLI subcommands** (`aic-edit init`, `aic-edit tenant add` etc.). All
  Step-2 functionality is reachable via the TUI; CLI niceties can wait.

## Risks & open items

- **Browser-handoff redirect URI whitelisting.** The OAuth client used for
  `?redirect_uri=http://127.0.0.1:*/cb` must allow that URI. Frodo uses the
  built-in `idmAdminClient` with a hosted redirect helper — we may need to
  either (a) confirm `idmAdminClient` accepts localhost, (b) use a
  different built-in client, or (c) document a one-time admin-console
  setup step. If none of those work cleanly, browser mode degrades to
  "open browser to admin console, then come back to TUI in paste mode" —
  still better than no help at all.
- **4096-bit RSA keypair generation in Rust is slow** (10–30s on commodity
  CPUs). Show progress UI; consider doing it before the form is submitted
  so it's ready when the user finishes typing.
- **`keyring` crate platform behaviour varies.** Linux Secret Service
  requires `gnome-keyring` or `kwallet` to be running; in headless envs
  (some WSL setups) it fails. Fall back gracefully to "prompt every time".
- **First-launch UX with no tenants** needs to be obviously navigable —
  not a blank screen. Centered "Welcome — press Ctrl-N to add your first
  tenant" message in the empty body.
