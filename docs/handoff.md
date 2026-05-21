# Handoff — Yubikey unlock work, in progress

Snapshot of where this branch sits and what the next agent needs to know.
Read this before touching the master-password / yubikey code paths.

## Where we are

Step 2 was complete and committed in `a648f24`. Step 3 (ESVs tab) is the
notional next step per [PLAN.md](../PLAN.md) but a yubikey-unlock detour
got opened up and isn't finished.

Since `a648f24` the working tree has:

- `src/config/crypto.rs` rewritten for **envelope encryption** — random 32-byte
  DEK encrypts `keys.enc`; the DEK is wrapped per-method in
  `.aic-edit/wraps.toml`.
- `src/config/wraps.rs` — schema + load/save for `wraps.toml`.
- `src/yubikey.rs` — `enroll()` + `derive_hmac()` over `ctap-hid-fido2`,
  using the FIDO2 `hmac-secret` extension.
- `src/app.rs` — DEK replaces in-memory master password; background poll task
  watches for a yubikey tap on the unlock screen; `Ctrl-Y` in Normal mode
  enrols a new yubikey.
- `src/ui/unlock.rs` shows "🔑 Tap your Yubikey, or type your password below"
  when a yubikey wrap exists.
- `src/ui/header.rs` surfaces the `^Y enrol yubikey` hint.
- `shell.nix` — pulls `systemd` (for `libudev.pc`) into the dev shell so
  `cargo build` works on bare NixOS.
- `examples/yubikey_probe.rs` — standalone CTAP2 hmac-secret probe for
  diagnosis. Survives this branch indefinitely as a debugging tool.
- The `TenantTheme` / `Theme` duplication has been folded — there's just
  `TenantTheme` now and a free `theme::style_for(t)`.

`cargo build` and `cargo clippy --no-deps` are clean (run via `nix-shell`).

## Issue 1 (open) — yubikey hmac-secret needs a PIN

The current main-app enrolment fails with
`CTAP1_ERR_INVALID_COMMAND (0x01)` because `src/yubikey.rs` calls
`MakeCredentialArgsBuilder::without_pin_and_uv()` and `hmac-secret` on a
yubikey **requires** a `pinUvAuthToken` to be established. No PIN ⇒ no
shared secret ⇒ device rejects the call.

The error code is misleadingly generic. On other authenticators we'd have
gotten `0x36 CTAP2_ERR_PIN_REQUIRED`. The yubikey 5 NFC just says "no" and
returns `0x01`.

### Evidence

`examples/yubikey_probe.rs` reproduces the issue out of the main app's way.
With PIN supplied the whole flow works end-to-end:

```
$ nix-shell --run "cargo run -q --example yubikey_probe -- info 0"
…
Extensions:  ["hmac-secret"]
Options:
  clientPin                       true
Client PIN set?  YES

$ nix-shell --run "cargo run -q --example yubikey_probe -- enroll 0"
FIDO2 PIN (blank to skip): ********
OK — credential_id = m9CoJH7MiV6SZt5Hmf-yZVMMU4QGydmKsJQWBsnWiBmwOz1cDAMJhAyDnjSqFuVoGUm_r95dqez1vqz8qE57_g
salt   = 4GMwJqj04nx4PubADyjp4ziT7OAYlVa_BZ7RPTX59Ig
hmac   = 6QmQJYPR59Yuc6P2CRViffY8-jPa2H_0Lluh1XyUCFU
```

`hmac` is deterministic for `(credential_id, salt, device)`; rerunning
`assert <cred> <salt>` produces the same value. That's what the wrap
envelope relies on.

### Next code change (small, well-defined)

Thread PIN entry through enrolment and unlock:

1. **Enrolment.** Before calling `crate::yubikey::enroll()`, push a one-row
   inline PIN prompt (similar to the OTP overlay in the userpass form) and
   read the value. Pass it via a new optional argument to
   `yubikey::enroll(pin: Option<&str>)` which threads it into
   `MakeCredentialArgsBuilder::pin(...)` and
   `GetAssertionArgsBuilder::pin(...)`. Keep `.without_pin_and_uv()` as the
   fallback only when `pin = None`.
2. **Unlock.** The yubikey poll task in `App::spawn_yubikey_poll` needs the
   PIN to call `derive_hmac`. Same plumbing: pass `Option<&str>` through
   `crate::config::unlock_with_yubikey` to `yubikey::derive_hmac`.
3. **Where does the PIN come from at unlock time?** See Issue 3.

The `examples/yubikey_probe.rs` already wires PIN entry both ways — copy
that shape.

## Issue 2 — FIDO2 vs U2F (gnubby ruling)

You asked why a Yubico-made gnubby isn't usable. Short version: it speaks
the older protocol, not the one we need.

### The two protocols

**FIDO U2F (2014, "CTAP1").** Single use case: 2-factor authentication.
The browser sends a challenge, the device signs it with a credential
keypair, the server verifies the signature. Operations are just `register`
and `authenticate`. No PIN, no resident credentials, no extensions.

**FIDO2 (2018, "CTAP2").** Supersedes U2F. Adds:
- Resident credentials (passkeys) — the device stores credentials locally
  so the server doesn't have to send them back.
- PIN-based user verification (`clientPin`, `pinUvAuthToken`).
- Extensions: `hmac-secret`, `credBlob`, `largeBlob`, etc.
- Multiple public-key algorithms.

CTAP2 devices typically also speak CTAP1 for backward compat with old U2F
sites. CTAP1 devices CANNOT speak CTAP2 — the firmware doesn't have it.

### Your gnubby specifically

vid=0x1050 pid=0x0200, "Yubico Gnubby (gnubby1)". Yubico-manufactured for
Google's internal 2FA program (Gnubby was Google's project codename — it's
where U2F came from). The hardware is **U2F-only**. The probe confirmed:

```
$ cargo run --example yubikey_probe -- info 1
Opening device [1]: vid=0x1050 pid=0x0200  Yubico Gnubby (gnubby1)
CTAPHID_ERROR Error code = 0x01
ERROR: get_info: response_status err = 0x01 CTAP1_ERR_INVALID_COMMAND
```

`get_info` is a CTAP2 command. The gnubby doesn't know what to do with it.
There's no FIDO2 applet on the device, no `hmac-secret` extension — no way
for it to act as our unlock factor.

Why can't we work around it? Because the entire scheme relies on the device
implementing `hmac-secret` and the matching `pinUvAuthToken` exchange. These
are CTAP2 features. The gnubby's firmware doesn't have them and they can't
be added — it's an immutable hardware/firmware feature set, predating the
spec.

(If you ever want a backup security key that works with aic-edit, the
cheapest option that supports `hmac-secret` is a Yubikey 5 series, a SoloKey,
or a Nitrokey 3.)

## Issue 3 — keychain vs daemon (this is the real gap)

> "I'm surprised we're using the OS keychain. I haven't seen evidence of that
> yet (I have to type my password every time) and I thought we were going to
> handle that with a daemon."

You're right. The current state is inconsistent — there are **two separate
caches** in the codebase and the TUI uses the wrong one:

### What's in the code today

- **`src/keychain.rs`** + calls from `src/app.rs` — best-effort writes to
  the OS keychain (`keyring` crate, Secret Service on Linux). After unlock,
  the master password is base64'd and stored under
  `keyring::Entry::new("aic-edit", cwd_path)`. On next launch
  `try_keychain_unlock()` reads it back and tries to unlock with it. If it
  works the user sees Normal mode immediately.
- **`src/agent/daemon.rs`** + `src/agent/client.rs` — a separate `ssh-agent`-
  style daemon that the CLI subcommands talk to over `.aic-edit/agent.sock`.
  Holds decrypted JWKs in memory after one `aic-edit unlock` invocation and
  hands out bearer tokens to subsequent `aic-edit ...` commands.

The TUI does NOT talk to the daemon. The daemon was built for the CLI side.

### Why you're typing your password every time

Almost certainly because the OS keychain path is silently failing on your
NixOS. The `keyring 3.x` crate defaults to Secret Service (D-Bus); if
`gnome-keyring`, `kwallet`, or another Secret Service provider isn't running
in your session, `store_key` errors out, we `let _ = …` the result (the call
is best-effort by design), and there's nothing stored to read back next
launch.

Quick check the next agent should run:

```bash
# Is Secret Service available?
busctl --user list 2>/dev/null | grep -i 'org.freedesktop.secrets'
# Try storing and reading something:
nix-shell --run "cargo run --bin keychain-probe -- store test test"
```

(That second command is hypothetical — there's no keychain-probe binary
today. Either write one or use `secret-tool` from `libsecret-tools`.)

### Recommended direction

Drop the OS-keychain path from the TUI and unify on the daemon:

1. On unlock, the TUI does `AgentClient::connect_or_spawn()` (already in
   `src/cli/mod.rs:103`), passes the password, then calls
   `Request::Unlock { password }`. The daemon decrypts, holds the DEK
   in memory, and answers `Request::GetToken { tenant }` for both the TUI
   and the CLI.
2. The TUI's startup check becomes: if the daemon is already running and
   has unlocked state, ask it for the JWK map directly — no password
   prompt. Otherwise the TUI's unlock screen kicks in as today.
3. The daemon needs the DEK envelope refactor too — currently it calls
   `config::unlock_with_password` and stores a `HashMap<String, JWK>` per
   tenant. That's fine for the password path; for yubikey unlock it should
   accept a DEK directly.
4. Remove `src/keychain.rs` once nothing references it.

This also gets rid of the unsightly "let _ = keychain::store_key(...)"
fire-and-forget pattern.

### How yubikey PIN caching fits in

Once the daemon is the single source of cache truth, the PIN can ride on
the same channel — the daemon could optionally remember the yubikey PIN
in memory after the first unlock of the session. That gives the
"touch-only after first unlock" UX without involving the OS keychain.

If you don't want to do the full keychain → daemon migration first, the
short-term answer is "prompt for PIN on every yubikey unlock" (Option B
from the previous session). That's worse UX but doesn't compound the
existing inconsistency.

## How to run the diagnostic probe

```bash
nix-shell --run "cargo run -q --example yubikey_probe -- list"
nix-shell --run "cargo run -q --example yubikey_probe -- info 0"
YUBIKEY_PIN=… nix-shell --run "cargo run -q --example yubikey_probe -- enroll 0"
nix-shell --run "cargo run -q --example yubikey_probe -- assert <cred> <salt> 0"
```

`<device-index>` comes from `list`. Default is 0.

## Recommended order of operations for the next agent

1. Read this file, then `PLAN.md`, then `docs/api/99-quirks-and-open-questions.md`.
2. **Decide on the keychain → daemon migration** before adding PIN
   handling. The PIN cache design changes meaningfully depending on which
   cache it sits next to.
3. Thread PIN through `crate::yubikey::enroll` and `derive_hmac`
   (small, mechanical change — model it on `examples/yubikey_probe.rs`).
4. Add the in-form PIN prompt to the enrolment flow and the yubikey
   unlock UI.
5. Test end-to-end against a real yubikey 5. The probe is your friend
   for sanity-checking.

## What works without further code changes

- Password-only unlock and credentials at rest.
- Tenant onboarding (all three patterns).
- The probe (`examples/yubikey_probe.rs`) — already verified end-to-end on
  a Yubikey 5 NFC.

## What doesn't work yet

- Yubikey unlock in the main app (will fail with `INVALID_COMMAND` until
  the PIN plumbing lands).
- "Remembered unlock" — OS keychain stores succeed on Linux only with
  Secret Service running, which isn't the case on the user's machine.
