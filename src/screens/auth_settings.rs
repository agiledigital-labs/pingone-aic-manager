//! Auth Settings screen — list enrolled factors, add (→ `auth_setup`),
//! remove, rename, or disable encryption entirely. Owns the selection
//! cursor, the rename buffer, and the pending y/n action.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, InputMode};
use crate::config::wraps::{Wrap, WrapsFile};
use crate::event::ToastKind;
use crate::screens::auth_setup::AuthMethod;

/// Action waiting on a y/n confirmation overlay.
#[derive(Debug)]
pub enum PendingAuthAction {
    /// Remove the wrap at this index from `wraps.toml`. Last-factor cases
    /// transition to `DisableEncryption` before reaching this state, so
    /// the index is guaranteed not to be the only wrap.
    RemoveWrap(usize),
    /// Decrypt `keys.enc` → `keys.plain` and delete all wraps. Triggered
    /// either by `[x]` or by attempting to remove the last wrap.
    DisableEncryption,
}

#[derive(Debug, Default)]
pub struct State {
    pub idx: usize,
    pub rename_input: String,
    pub pending: Option<PendingAuthAction>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// Human-readable summary of the pending y/n action. Used by the UI
    /// to label the confirmation overlay.
    pub fn pending_action_label(&self, wraps: &WrapsFile) -> Option<String> {
        match &self.pending {
            Some(PendingAuthAction::RemoveWrap(idx)) => wraps
                .wraps
                .get(*idx)
                .map(|w| format!("Remove factor: {}?", w.label())),
            Some(PendingAuthAction::DisableEncryption) => {
                Some("Disable encryption? Credentials will be written to keys.plain.".into())
            }
            None => None,
        }
    }
}

/// Open the auth-settings screen from anywhere (Ctrl-A in Normal mode).
/// Clamps the cursor so we never index past the current factor list.
pub fn open(app: &mut App) {
    let n = app.wraps.wraps.len();
    if n == 0 {
        app.auth_settings.idx = 0;
    } else if app.auth_settings.idx >= n {
        app.auth_settings.idx = n - 1;
    }
    app.input_mode = InputMode::AuthSettings;
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let n = app.wraps.wraps.len();
    match key.code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.auth_settings.idx > 0 {
                app.auth_settings.idx -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if n > 0 && app.auth_settings.idx + 1 < n {
                app.auth_settings.idx += 1;
            }
        }
        KeyCode::Char('p') | KeyCode::Char('P') => {
            crate::screens::auth_setup::start_add_factor(app, AuthMethod::Password);
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            crate::screens::auth_setup::start_add_factor(app, AuthMethod::SecurityKey);
        }
        KeyCode::Char('d') | KeyCode::Char('D') if n > 0 => {
            // Last-factor guard — falls through to disable-encryption.
            if n == 1 {
                app.auth_settings.pending = Some(PendingAuthAction::DisableEncryption);
            } else {
                app.auth_settings.pending =
                    Some(PendingAuthAction::RemoveWrap(app.auth_settings.idx));
            }
            app.input_mode = InputMode::AuthSettingsConfirm;
        }
        KeyCode::Enter if n > 0 => edit_factor(app, app.auth_settings.idx),
        KeyCode::Char(c @ '1'..='9') => {
            // Number-key hotkey, matching the Add Tenant menu: jump the
            // cursor to the row and trigger its edit action in one step.
            let target = c.to_digit(10).unwrap() as usize - 1;
            if target < n {
                app.auth_settings.idx = target;
                edit_factor(app, target);
            }
        }
        _ => {}
    }
    Ok(())
}

/// Open the edit action for the factor at `idx`. Password rows route to
/// "change password"; security-key rows route to rename.
fn edit_factor(app: &mut App, idx: usize) {
    match app.wraps.wraps.get(idx) {
        Some(Wrap::Password { .. }) => {
            crate::screens::auth_setup::start_add_factor(app, AuthMethod::Password);
        }
        Some(Wrap::SecurityKey { .. }) => {
            app.auth_settings.idx = idx;
            start_rename(app);
        }
        None => {}
    }
}

fn start_rename(app: &mut App) {
    if let Some(Wrap::SecurityKey { label, .. }) = app.wraps.wraps.get(app.auth_settings.idx) {
        app.auth_settings.rename_input = label.clone().unwrap_or_default();
        app.input_mode = InputMode::AuthSettingsRename;
    }
}

pub fn handle_rename_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.auth_settings.rename_input.clear();
            app.input_mode = InputMode::AuthSettings;
        }
        KeyCode::Enter => {
            let new_label = app.auth_settings.rename_input.trim().to_string();
            if new_label.is_empty() {
                // Silently refuse — Esc cancels, Enter requires a label.
                return Ok(());
            }
            if let Some(Wrap::SecurityKey { label, .. }) =
                app.wraps.wraps.get_mut(app.auth_settings.idx)
            {
                *label = Some(new_label);
                app.wraps.save()?;
                app.push_toast(ToastKind::Success, "Renamed");
            }
            app.auth_settings.rename_input.clear();
            app.input_mode = InputMode::AuthSettings;
        }
        KeyCode::Backspace => {
            app.auth_settings.rename_input.pop();
        }
        KeyCode::Char(c) => {
            app.auth_settings.rename_input.push(c);
        }
        _ => {}
    }
    Ok(())
}

pub async fn handle_confirm_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let action = app.auth_settings.pending.take();
            match action {
                Some(PendingAuthAction::RemoveWrap(idx)) => {
                    if idx < app.wraps.wraps.len() {
                        app.wraps.wraps.remove(idx);
                        // If that was the last security key, drop the
                        // shared salt — the next enrolment generates a
                        // fresh one.
                        app.wraps.clear_security_key_salt_if_unused();
                        app.wraps.save()?;
                        if app.auth_settings.idx >= app.wraps.wraps.len()
                            && !app.wraps.wraps.is_empty()
                        {
                            app.auth_settings.idx = app.wraps.wraps.len() - 1;
                        }
                        app.push_toast(ToastKind::Success, "Factor removed");
                    }
                }
                Some(PendingAuthAction::DisableEncryption) => {
                    match app.dek_clone() {
                        Some(dek) => {
                            crate::config::disable_encryption(&dek)?;
                            drop(dek);
                            app.set_dek(None);
                            app.wraps = WrapsFile::default();
                            let mut s = app.settings.unwrap_or_default();
                            s.encrypt_keys = false;
                            app.settings = Some(s);
                            app.auth_settings.idx = 0;
                            // Switch the agent over to plain mode now that
                            // keys.plain exists; otherwise the next ApiCall
                            // would hit a daemon still holding the (now
                            // useless) DEK.
                            crate::screens::unlock::unlock_plain_agent(app).await;
                            app.push_toast(
                                ToastKind::Info,
                                "Encryption disabled — credentials at keys.plain",
                            );
                        }
                        _ => {
                            app.push_toast(ToastKind::Error, "Cannot disable: not unlocked");
                        }
                    }
                }
                None => {}
            }
            app.input_mode = InputMode::AuthSettings;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.auth_settings.pending = None;
            app.input_mode = InputMode::AuthSettings;
        }
        _ => {}
    }
    Ok(())
}
