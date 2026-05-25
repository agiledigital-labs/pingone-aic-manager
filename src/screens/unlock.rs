//! Unlock screen — re-entered every TUI launch when `keys.enc` exists and
//! the agent doesn't already hold the DEK. Owns the master-password +
//! security-key PIN fields and the background hmac-secret poll task.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use crossterm::event::{KeyCode, KeyEvent};

use crate::agent::{AgentClient, Request as AgentRequest, Response as AgentResponse};
use crate::app::{App, InputMode};
use crate::auth::UnlockOk;
use crate::config::crypto::Dek;
use crate::config::Settings;
use crate::event::AppEvent;

/// Which input on the Unlock screen has focus. Only meaningful when a
/// security-key wrap is enrolled; otherwise the Master password is the
/// only field and `focus` is irrelevant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    SecurityKeyPin,
    Password,
}

#[derive(Debug)]
pub struct State {
    /// Typed master password (cleared on Enter / on switch to the PIN
    /// field).
    pub input: String,
    /// Last error to surface above the input.
    pub error: Option<String>,
    /// True while a background unlock task is running. Locks the UI to
    /// Esc-only.
    pub busy: bool,
    /// Typed security-key PIN. Consumed by the background poll the
    /// moment Enter is pressed on the PIN field — we don't keep it
    /// around.
    pub pin_input: String,
    pub focus: Focus,

    /// Set by the background security-key poll to stop itself once unlock
    /// has happened by any method. Shared with the spawned task.
    pub security_key_cancel: Arc<AtomicBool>,
    /// True while a security-key poll task is running — guards against
    /// spawning more than one.
    pub security_key_armed: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            input: String::new(),
            error: None,
            busy: false,
            pin_input: String::new(),
            focus: Focus::SecurityKeyPin,
            security_key_cancel: Arc::new(AtomicBool::new(false)),
            security_key_armed: false,
        }
    }
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }
}

/// If the agent is already holding the DEK from a previous TUI session,
/// fetch it and hydrate `app.dek` + `app.jwks` so `decide_initial_mode`
/// can skip the Unlock screen. Best-effort: any failure (agent missing,
/// stale socket, locked, decrypt-error) silently falls through to the
/// normal unlock path.
pub async fn try_agent_unlock(app: &mut App) {
    // Only meaningful when there's an encrypted blob to unlock.
    if !matches!(app.settings, Some(Settings { encrypt_keys: true, .. })) {
        return;
    }

    let client = match AgentClient::connect_or_spawn().await {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("agent unavailable on startup: {e}");
            return;
        }
    };
    let dek_b64 = match client.send(&AgentRequest::GetDek).await {
        Ok(AgentResponse::Dek { dek_b64 }) => dek_b64,
        Ok(AgentResponse::Locked) => return,
        Ok(other) => {
            tracing::debug!("unexpected GetDek reply: {other:?}");
            return;
        }
        Err(e) => {
            tracing::debug!("GetDek failed: {e}");
            return;
        }
    };
    let bytes = match B64.decode(&dek_b64) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("agent returned non-base64 DEK: {e}");
            return;
        }
    };
    let arr: [u8; 32] = match bytes.as_slice().try_into() {
        Ok(a) => a,
        Err(_) => {
            tracing::warn!("agent returned DEK of wrong length");
            return;
        }
    };
    let dek = Dek::from_bytes(arr);
    let jwks = match crate::config::decrypt_keys_file(&dek) {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!("agent DEK failed to decrypt keys.enc: {e}");
            return;
        }
    };
    app.set_dek(Some(dek));
    app.set_jwks(jwks);
}

/// Hand the just-derived DEK to the agent so subsequent ApiCalls (this
/// session's ESV poll, plus any CLI invocation) find the agent unlocked.
/// Must be awaited before triggering any tenant-scoped HTTP — otherwise
/// the first `refresh_esvs` races the `PutDek` to the daemon and loses.
/// Failure is logged, not surfaced — the user's already past the unlock
/// screen, and the TUI can keep going (agent calls will just re-prompt).
pub async fn put_dek_to_agent(app: &App) {
    let Some(dek) = app.dek_clone() else { return };
    if let Err(e) = crate::auth::put_dek_to_agent(&dek).await {
        tracing::warn!("PutDek failed: {e}");
    }
}

/// Plain-mode equivalent of `put_dek_to_agent`. There's no DEK to send —
/// we tell the agent to load `keys.plain` itself, so subsequent
/// `ApiCall`s find it in the "unlocked, plain" vault state instead of
/// returning `Locked` and stranding the request.
pub async fn unlock_plain_agent(_app: &App) {
    if let Err(e) = crate::auth::unlock_plain_agent().await {
        tracing::warn!("UnlockPlain failed: {e}");
    }
}

/// Lock the agent (drop its cached DEK) and quit the TUI. The next launch
/// goes back through the Unlock screen.
pub async fn lock_and_quit(app: &mut App) {
    app.set_dek(None);
    // Best-effort. If the agent isn't running there's nothing to lock;
    // we deliberately don't spawn one just to immediately lock it.
    if let Ok(c) = AgentClient::connect(crate::agent::socket_path()).await {
        let _ = c.send(&AgentRequest::Lock).await;
    }
    app.should_quit = true;
}

/// Spawn the security-key background poll for the Unlock screen. `pin`
/// is the FIDO2 PIN the user just typed; it's required for every
/// assertion.
fn spawn_security_key_poll(app: &mut App, pin: String) {
    if app.unlock.security_key_armed {
        return;
    }
    app.unlock.security_key_armed = true;
    app.unlock.security_key_cancel.store(false, Ordering::Relaxed);
    let wraps = app.wraps.clone();
    let tx = app.events.tx.clone();
    let cancel = app.unlock.security_key_cancel.clone();
    tokio::task::spawn_blocking(move || {
        let pin_opt = if pin.is_empty() { None } else { Some(pin.as_str()) };
        loop {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            if !crate::security_key::device_present() {
                // No security key plugged in; poll again in a moment.
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
            // Single allowList call across every enrolled credential —
            // the device picks the one it has, we identify the wrap
            // from the returned credential_id.
            match crate::config::unlock_with_security_key(&wraps, pin_opt) {
                Ok((dek, jwks)) => {
                    let _ = tx.send(AppEvent::UnlockResult(Ok(UnlockOk { dek, jwks })));
                    return;
                }
                Err(e) => {
                    let msg = e.to_string();
                    // Whitelist: only "device doesn't hold any matching
                    // credential" lets us retry silently. Everything
                    // else — wrong PIN, blocked, bad device — is
                    // surfaced so the user can react before we hammer
                    // the PIN counter further.
                    if msg.contains("NO_CREDENTIALS") || msg.contains("0x2E") {
                        tracing::debug!("device has no enrolled credential: {msg}");
                        std::thread::sleep(Duration::from_secs(2));
                        continue;
                    }
                    let _ = tx.send(AppEvent::UnlockResult(Err(format!(
                        "security key: {msg}"
                    ))));
                    return;
                }
            }
        }
    });
}

pub fn handle_key(app: &mut App, key: KeyEvent) {
    // While the unlock task is running, ignore everything except Esc-to-quit.
    if app.unlock.busy {
        if key.code == KeyCode::Esc {
            app.should_quit = true;
        }
        return;
    }

    // Which methods are actually enrolled. Tab toggles only when both
    // are present; otherwise focus is pinned to whichever exists.
    let yk = app.wraps.has_security_key();
    let pw = app.wraps.has_password();
    let both = yk && pw;
    let on_pin = (yk && !pw) || (both && app.unlock.focus == Focus::SecurityKeyPin);

    match key.code {
        KeyCode::Esc => {
            app.should_quit = true;
        }
        KeyCode::Tab | KeyCode::BackTab if both => {
            app.unlock.focus = match app.unlock.focus {
                Focus::SecurityKeyPin => Focus::Password,
                Focus::Password => Focus::SecurityKeyPin,
            };
        }
        KeyCode::Enter => {
            if on_pin {
                if app.unlock.pin_input.is_empty() {
                    app.unlock.error = Some("Security key PIN cannot be empty".into());
                    return;
                }
                app.unlock.error = Some(crate::ui::unlock::TAP_MESSAGE.into());
                let pin = std::mem::take(&mut app.unlock.pin_input);
                spawn_security_key_poll(app, pin);
            } else {
                if app.unlock.input.is_empty() {
                    app.unlock.error = Some("Password cannot be empty".into());
                    return;
                }
                let password = std::mem::take(&mut app.unlock.input);
                app.unlock.error = None;
                app.unlock.busy = true;
                let tx = app.events.tx.clone();
                tokio::spawn(async move {
                    let result = crate::auth::unlock_password(password)
                        .await
                        .map_err(|e| e.to_string());
                    let _ = tx.send(AppEvent::UnlockResult(result));
                });
            }
        }
        KeyCode::Backspace => {
            if on_pin {
                app.unlock.pin_input.pop();
            } else {
                app.unlock.input.pop();
            }
        }
        KeyCode::Char(c) => {
            if on_pin {
                app.unlock.pin_input.push(c);
            } else {
                app.unlock.input.push(c);
            }
        }
        _ => {}
    }
}

pub async fn handle_result(
    app: &mut App,
    result: std::result::Result<UnlockOk, String>,
) {
    // A late-arriving second result (e.g. security-key unlock fired
    // after we already accepted the password) — drop it.
    if app.input_mode != InputMode::Unlock {
        return;
    }
    app.unlock.busy = false;
    match result {
        Ok(UnlockOk { dek, jwks }) => {
            app.set_dek(Some(dek));
            app.set_jwks(jwks);
            app.unlock.error = None;
            app.unlock.pin_input.clear();
            app.input_mode = InputMode::Normal;
            // Tell the security-key poll task to stop (if it was running).
            app.unlock.security_key_cancel.store(true, Ordering::Relaxed);
            app.unlock.security_key_armed = false;
            // Order matters: the agent must hold the DEK *before* we fire
            // off the ESV refresh, otherwise the ApiCall lands on a
            // still-locked daemon and we surface a spurious "agent locked".
            put_dek_to_agent(app).await;
            crate::screens::esv::refresh(app, false);
        }
        Err(e) => {
            // A security-key failure shouldn't take the screen down — let
            // the user retry the tap or fall back to the password field.
            app.unlock.security_key_cancel.store(true, Ordering::Relaxed);
            app.unlock.security_key_armed = false;
            app.unlock.error = Some(format!("Unlock failed: {e}"));
        }
    }
}
