//! Unlock screen — re-entered every TUI launch when `keys.enc` exists and
//! the agent doesn't already hold the DEK. Owns the master-password +
//! security-key PIN fields and the background hmac-secret poll task.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use crossterm::event::{KeyCode, KeyEvent};

use crate::agent::{AgentClient, Request as AgentRequest, Response as AgentResponse};
use crate::app::event::{AppEvent, ToastKind};
use crate::app::{App, InputMode};
use crate::config::Settings;
use crate::config::crypto::Dek;

use super::auth::{self, UnlockOk};
use super::screen::{Event, Mode};
use super::security_key;

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

    /// Last non-invasive agent status poll. `Status` deliberately does not
    /// reset the daemon's idle timer, so this can notice a relock safely.
    pub last_agent_status_check: Instant,
    /// The exact editing mode to return to after dismissing or completing a
    /// relock prompt; preserving it keeps drafts owned by other features alive.
    pub relock_return_mode: Option<InputMode>,
    /// Set when the user dismisses the relock prompt, and cleared once the agent
    /// reports itself unlocked again. Without it the 5s poll would immediately
    /// re-open a modal the user just closed, since dismissing changes neither
    /// the agent's lock nor the DEK the TUI still holds.
    pub relock_dismissed: bool,
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
            last_agent_status_check: Instant::now(),
            relock_return_mode: None,
            relock_dismissed: false,
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
    if !matches!(
        app.settings.as_ref(),
        Some(Settings {
            encrypt_keys: true,
            ..
        })
    ) {
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
    let log_keys = match auth::decrypt_log_keys(&dek) {
        Ok(keys) => keys,
        Err(e) => {
            tracing::warn!("agent DEK failed to decrypt log-keys.enc: {e}");
            return;
        }
    };
    app.set_dek(Some(dek));
    app.set_jwks(jwks);
    app.set_log_keys(log_keys);
}

/// Hand the just-derived DEK to the agent so subsequent ApiCalls (this
/// session's ESV poll, plus any CLI invocation) find the agent unlocked.
/// Must be awaited before triggering any tenant-scoped HTTP — otherwise
/// the first `refresh_esvs` races the `PutDek` to the daemon and loses.
/// Failure is logged, not surfaced — the user's already past the unlock
/// screen, and the TUI can keep going (agent calls will just re-prompt).
pub async fn put_dek_to_agent(app: &App) {
    let Some(dek) = app.dek_clone() else { return };
    let result = auth::put_dek_to_agent(&dek).await;
    drop(dek);
    if let Err(e) = result {
        tracing::warn!("PutDek failed: {e}");
    }
}

/// Plain-mode equivalent of `put_dek_to_agent`. There's no DEK to send —
/// we tell the agent to load `keys.plain` itself, so subsequent
/// `ApiCall`s find it in the "unlocked, plain" vault state instead of
/// returning `Locked` and stranding the request.
pub async fn unlock_plain_agent(_app: &App) {
    if let Err(e) = auth::unlock_plain_agent().await {
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
    app.unlock
        .security_key_cancel
        .store(false, Ordering::Relaxed);
    let wraps = app.wraps.clone();
    let tx = app.events.tx.clone();
    let cancel = app.unlock.security_key_cancel.clone();
    tokio::task::spawn_blocking(move || {
        let pin_opt = if pin.is_empty() {
            None
        } else {
            Some(pin.as_str())
        };
        loop {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            if !security_key::device_present() {
                // No security key plugged in; poll again in a moment.
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
            // Single allowList call across every enrolled credential —
            // the device picks the one it has, we identify the wrap
            // from the returned credential_id.
            let result = crate::config::unlock_with_security_key(&wraps, pin_opt);
            match result {
                Ok((dek, jwks)) => {
                    let log_keys = match auth::decrypt_log_keys(&dek) {
                        Ok(keys) => keys,
                        Err(e) => {
                            let _ = tx.send(AppEvent::Vault(Event::UnlockFinished(Err(format!(
                                "log key vault: {e}"
                            )))));
                            return;
                        }
                    };
                    let _ = tx.send(AppEvent::Vault(Event::UnlockFinished(Ok(UnlockOk {
                        dek,
                        jwks,
                        log_keys,
                    }))));
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
                    let _ = tx.send(AppEvent::Vault(Event::UnlockFinished(Err(format!(
                        "security key: {msg}"
                    )))));
                    return;
                }
            }
        }
    });
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) {
    // While the unlock task is running, ignore everything except Esc-to-quit.
    if app.unlock.busy {
        if key.code == KeyCode::Esc {
            dismiss_relock(app, mode);
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
            dismiss_relock(app, mode);
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
                app.unlock.error = Some(security_key::TAP_MESSAGE.into());
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
                    let result = auth::unlock_password(password)
                        .await
                        .map_err(|e| e.to_string());
                    let _ = tx.send(AppEvent::Vault(Event::UnlockFinished(result)));
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

fn dismiss_relock(app: &mut App, mode: Mode) {
    if mode == Mode::Unlock {
        app.should_quit = true;
        return;
    }

    app.unlock
        .security_key_cancel
        .store(true, Ordering::Relaxed);
    app.unlock.security_key_armed = false;
    app.unlock.busy = false;
    app.unlock.input.clear();
    app.unlock.pin_input.clear();
    app.unlock.error = None;
    // The TUI still holds its own DEK, so `is_unlocked()` stays true and the
    // agent stays locked — the 5s poll would re-open this modal immediately and
    // then every 5s after that. Latch the dismissal instead; `^L` reopens it and
    // a status reply saying the agent is unlocked clears the latch.
    app.unlock.relock_dismissed = true;
    app.input_mode = app
        .unlock
        .relock_return_mode
        .take()
        .unwrap_or(InputMode::Normal);
}

/// Start a non-spawning `Status` probe when its five-second cadence is due.
/// The daemon explicitly excludes this request from its idle-timer bump.
pub fn poll_agent_status(app: &mut App) {
    // A TUI that holds no credentials of its own has nothing to notice a
    // divergence from — this is the startup unlock screen, where probing the
    // socket every 5s for the whole session would be pure churn.
    if !app.is_unlocked() {
        return;
    }
    if app.unlock.last_agent_status_check.elapsed() < Duration::from_secs(5) {
        return;
    }
    app.unlock.last_agent_status_check = Instant::now();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let Ok(client) = AgentClient::connect(crate::agent::socket_path()).await else {
            return;
        };
        let Ok(AgentResponse::Status(status)) = client.send(&AgentRequest::Status).await else {
            return;
        };
        let _ = tx.send(AppEvent::Vault(Event::AgentStatus(status.unlocked)));
    });
}

/// Everything the relock decision depends on, gathered from `App` at the call
/// site so the decision itself is a pure function of named values. `App::new()`
/// reads configuration from disk, so a predicate taking `&App` couldn't be
/// unit-tested at all.
#[derive(Clone, Copy, Debug)]
pub struct RelockCheck {
    /// What the agent just told us about its own vault.
    pub agent_unlocked: bool,
    /// Whether the TUI still holds credentials of its own.
    pub app_unlocked: bool,
    pub has_password: bool,
    pub has_security_key: bool,
    /// Set once the user has explicitly dismissed the prompt this lock.
    pub dismissed: bool,
    pub mode: InputMode,
}

impl RelockCheck {
    /// Whether an agent status reply should interrupt the current work with a
    /// relock prompt.
    pub fn should_open(&self) -> bool {
        // The TUI thinking it has credentials while the agent disagrees is the
        // whole signal: it means the daemon dropped its DEK under us.
        !self.agent_unlocked
            && self.app_unlocked
            && !self.dismissed
            && (self.has_password || self.has_security_key)
            && !matches!(
                self.mode,
                InputMode::Vault(_) | InputMode::Onboard(_) | InputMode::ProdConfirm
            )
    }
}

/// Consume a status reply and, if it represents an idle relock, open the
/// overlay without disturbing the underlying feature's draft mode.
pub fn handle_agent_status(app: &mut App, status_unlocked: bool) {
    // `Status` reports a plain vault as unlocked too. Plain mode has no DEK
    // or credential to re-enter, so an inconsistent/stale wraps file must not
    // turn its normal operation into an impossible relock prompt.
    if matches!(
        app.settings.as_ref(),
        Some(Settings {
            encrypt_keys: false,
            ..
        })
    ) {
        return;
    }
    // The agent came back on its own (someone ran `aic login`, say), so a
    // previous dismissal no longer applies to the next lock.
    if status_unlocked {
        app.unlock.relock_dismissed = false;
        return;
    }
    let check = RelockCheck {
        agent_unlocked: status_unlocked,
        app_unlocked: app.is_unlocked(),
        has_password: app.wraps.has_password(),
        has_security_key: app.wraps.has_security_key(),
        dismissed: app.unlock.relock_dismissed,
        mode: app.input_mode,
    };
    if !check.should_open() {
        return;
    }
    open_relock(app);
}

/// Open the relock prompt, remembering the mode to come back to. Reached both
/// from the status poll and from `^L`, for a user who dismissed it earlier.
pub fn open_relock(app: &mut App) {
    if matches!(app.input_mode, InputMode::Vault(_)) {
        return;
    }
    app.unlock.relock_dismissed = false;
    app.unlock.input.clear();
    app.unlock.pin_input.clear();
    app.unlock.error = None;
    app.unlock.busy = false;
    app.unlock.focus = if app.wraps.has_security_key() {
        Focus::SecurityKeyPin
    } else {
        Focus::Password
    };
    app.unlock
        .security_key_cancel
        .store(false, Ordering::Relaxed);
    app.unlock.security_key_armed = false;
    app.unlock.relock_return_mode = Some(app.input_mode);
    app.input_mode = InputMode::Vault(Mode::Relock);
}

pub async fn handle_result(app: &mut App, result: std::result::Result<UnlockOk, String>) {
    // A late-arriving second result (e.g. security-key unlock fired
    // after we already accepted the password) — drop it.
    let is_relock = app.input_mode == InputMode::Vault(Mode::Relock);
    if !is_relock && app.input_mode != InputMode::Vault(Mode::Unlock) {
        return;
    }
    app.unlock.busy = false;
    match result {
        Ok(UnlockOk {
            dek,
            jwks,
            log_keys,
        }) => {
            app.set_dek(Some(dek));
            app.set_jwks(jwks);
            app.set_log_keys(log_keys);
            app.unlock.error = None;
            app.unlock.pin_input.clear();
            // Tell the security-key poll task to stop (if it was running).
            app.unlock
                .security_key_cancel
                .store(true, Ordering::Relaxed);
            app.unlock.security_key_armed = false;
            // Order matters: the agent must hold the DEK *before* we fire
            // off the ESV refresh, otherwise the ApiCall lands on a
            // still-locked daemon and we surface a spurious "agent locked".
            put_dek_to_agent(app).await;
            if is_relock {
                app.input_mode = app
                    .unlock
                    .relock_return_mode
                    .take()
                    .unwrap_or(InputMode::Normal);
                app.push_toast(ToastKind::Success, "Session unlocked".to_string());
                crate::app::refresh_view(app, app.active_view, false);
            } else {
                app.input_mode = InputMode::Normal;
                crate::esv::ops::refresh(app, false);
            }
        }
        Err(e) => {
            // A security-key failure shouldn't take the screen down — let
            // the user retry the tap or fall back to the password field.
            app.unlock
                .security_key_cancel
                .store(true, Ordering::Relaxed);
            app.unlock.security_key_armed = false;
            app.unlock.error = Some(format!("Unlock failed: {e}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The case the prompt exists for: agent dropped its DEK, TUI still holds
    /// one, and a password is enrolled to re-establish presence with.
    fn diverged() -> RelockCheck {
        RelockCheck {
            agent_unlocked: false,
            app_unlocked: true,
            has_password: true,
            has_security_key: false,
            dismissed: false,
            mode: InputMode::Normal,
        }
    }

    #[test]
    fn opens_only_when_the_agent_and_the_tui_disagree() {
        assert!(diverged().should_open());
        // Agent is fine — nothing to re-enter.
        assert!(
            !RelockCheck {
                agent_unlocked: true,
                ..diverged()
            }
            .should_open()
        );
        // The TUI has no credentials either, so this is the startup unlock
        // screen doing its job, not a mid-session lock.
        assert!(
            !RelockCheck {
                app_unlocked: false,
                ..diverged()
            }
            .should_open()
        );
    }

    #[test]
    fn needs_some_enrolled_factor_to_re_enter() {
        assert!(
            !RelockCheck {
                has_password: false,
                has_security_key: false,
                ..diverged()
            }
            .should_open()
        );
        assert!(
            RelockCheck {
                has_password: false,
                has_security_key: true,
                ..diverged()
            }
            .should_open()
        );
    }

    /// Dismissing changes neither the agent's lock nor the DEK the TUI holds, so
    /// without the latch every poll would re-open the modal the user just closed.
    #[test]
    fn a_dismissed_prompt_stays_closed() {
        assert!(
            !RelockCheck {
                dismissed: true,
                ..diverged()
            }
            .should_open()
        );
    }

    #[test]
    fn relock_never_interrupts_vault_onboarding_or_prod_confirmation() {
        for mode in [
            InputMode::Vault(Mode::Unlock),
            InputMode::Onboard(crate::onboard::screen::Mode::Menu),
            InputMode::ProdConfirm,
        ] {
            assert!(
                !RelockCheck { mode, ..diverged() }.should_open(),
                "{mode:?}"
            );
        }
    }
}
