//! First-run + add-factor auth setup screen.
//!
//! Owns the picker / password / security-key form (`State.form`), the
//! context (first-run vs add-factor), the input handler, the submit path,
//! and the post-enrolment finaliser. Rendering still lives in
//! `src/ui/auth_setup.rs`.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, InputMode};
use crate::config::crypto::{self, Dek};
use crate::config::wraps::{self, Wrap};
use crate::config::Settings;
use crate::config::ProjectConfig;
use crate::event::{AppEvent, ToastKind};

/// Auth methods offered on the first-run picker (and on the in-app
/// "add factor" flow). "None" = `keys.plain`, no DEK; the other two share
/// the same DEK wrapped two different ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    None,
    Password,
    SecurityKey,
}

impl AuthMethod {
    pub const ORDER: [AuthMethod; 3] = [
        AuthMethod::None,
        AuthMethod::Password,
        AuthMethod::SecurityKey,
    ];

    pub fn next(self) -> AuthMethod {
        let i = Self::ORDER.iter().position(|m| *m == self).unwrap_or(0);
        Self::ORDER[(i + 1) % Self::ORDER.len()]
    }

    pub fn prev(self) -> AuthMethod {
        let i = Self::ORDER.iter().position(|m| *m == self).unwrap_or(0);
        Self::ORDER[(i + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            AuthMethod::None => "None",
            AuthMethod::Password => "Master password",
            AuthMethod::SecurityKey => "Security key",
        }
    }
}

/// Fields on the first-run / add-factor setup form. Only some of these are
/// visible per `AuthMethod`; navigation skips the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSetupField {
    Method,
    Password,
    Confirm,
    Pin,
    Label,
    Submit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupContext {
    /// Initial install: no settings.toml on disk. Picker is full 3-way
    /// (None / Password / security key) and submit returns to Normal.
    FirstRun,
    /// "Add factor" from Auth Settings: picker hides None (you can't
    /// downgrade to no-encryption by adding a factor), and submit returns
    /// to AuthSettings.
    AddFactor,
}

#[derive(Debug, Clone)]
pub struct AuthSetupForm {
    pub method: AuthMethod,
    pub password: String,
    pub confirm: String,
    pub pin: String,
    pub label: String,
    pub focused: AuthSetupField,
    pub error: Option<String>,
    /// True while a blocking enrol task is in flight. Locks out input and
    /// shows a "Tap your security key…" hint.
    pub busy: bool,
}

impl Default for AuthSetupForm {
    fn default() -> Self {
        Self {
            method: AuthMethod::Password,
            password: String::new(),
            confirm: String::new(),
            pin: String::new(),
            label: "Security key 1".into(),
            focused: AuthSetupField::Method,
            error: None,
            busy: false,
        }
    }
}

impl AuthSetupForm {
    /// Field order for Tab/BackTab. `Method` is included only on first-run;
    /// `AddFactor` callers pre-pick the method on the previous screen
    /// (auth_settings `p`/`s`), so the picker isn't rendered or focusable.
    fn order(&self, context: SetupContext) -> &'static [AuthSetupField] {
        match (context, self.method) {
            (SetupContext::FirstRun, AuthMethod::None) => {
                &[AuthSetupField::Method, AuthSetupField::Submit]
            }
            (SetupContext::FirstRun, AuthMethod::Password) => &[
                AuthSetupField::Method,
                AuthSetupField::Password,
                AuthSetupField::Confirm,
                AuthSetupField::Submit,
            ],
            (SetupContext::FirstRun, AuthMethod::SecurityKey) => &[
                AuthSetupField::Method,
                AuthSetupField::Pin,
                AuthSetupField::Label,
                AuthSetupField::Submit,
            ],
            (SetupContext::AddFactor, AuthMethod::None) => &[AuthSetupField::Submit],
            (SetupContext::AddFactor, AuthMethod::Password) => &[
                AuthSetupField::Password,
                AuthSetupField::Confirm,
                AuthSetupField::Submit,
            ],
            (SetupContext::AddFactor, AuthMethod::SecurityKey) => &[
                AuthSetupField::Pin,
                AuthSetupField::Label,
                AuthSetupField::Submit,
            ],
        }
    }

    pub fn next(&mut self, context: SetupContext) {
        let order = self.order(context);
        let i = order.iter().position(|f| *f == self.focused).unwrap_or(0);
        self.focused = order[(i + 1) % order.len()];
    }

    pub fn prev(&mut self, context: SetupContext) {
        let order = self.order(context);
        let i = order.iter().position(|f| *f == self.focused).unwrap_or(0);
        self.focused = order[(i + order.len() - 1) % order.len()];
    }

    /// After switching the radio, keep focus on Method. The user can ←/→
    /// through the options to compare them, then Tab into the body when
    /// they're ready.
    pub fn settle_focus_after_method_change(&mut self) {
        self.focused = AuthSetupField::Method;
    }
}

/// State owned by `App.auth_setup`.
#[derive(Debug)]
pub struct State {
    pub form: AuthSetupForm,
    pub context: SetupContext,
}

impl Default for State {
    fn default() -> Self {
        Self {
            form: AuthSetupForm::default(),
            context: SetupContext::FirstRun,
        }
    }
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Skip None in the AddFactor flow — you can't downgrade encryption by
/// adding a factor.
fn step_method_next(current: AuthMethod, context: SetupContext) -> AuthMethod {
    let next = current.next();
    if context == SetupContext::AddFactor && next == AuthMethod::None {
        next.next()
    } else {
        next
    }
}

fn step_method_prev(current: AuthMethod, context: SetupContext) -> AuthMethod {
    let prev = current.prev();
    if context == SetupContext::AddFactor && prev == AuthMethod::None {
        prev.prev()
    } else {
        prev
    }
}

/// Enrol a security key and produce a wrap entry. Blocks until the user
/// taps the device. `hmac_salt` is the file-level salt shared across
/// every security-key wrap so unlock can be done in a single allowList
/// call.
fn enroll_security_key_blocking(
    dek: Dek,
    label: String,
    pin: Option<String>,
    hmac_salt: [u8; crate::security_key::HMAC_SALT_LEN],
) -> std::result::Result<Wrap, String> {
    let enrolment =
        crate::security_key::enroll(pin.as_deref(), &hmac_salt).map_err(|e| e.to_string())?;
    let (nonce, ct) =
        crypto::wrap_dek_with_kek(&dek, &enrolment.hmac).map_err(|e| e.to_string())?;
    Ok(Wrap::SecurityKey {
        label: Some(label),
        credential_id: wraps::b64_encode(&enrolment.credential_id),
        rp_id: crate::security_key::RP_ID.to_string(),
        nonce: wraps::b64_encode(&nonce),
        ciphertext: wraps::b64_encode(&ct),
    })
}

/// Open the SetupAuth form pre-set for "add factor" mode (entered from
/// auth_settings via `p` or `s`).
pub fn start_add_factor(app: &mut App, method: AuthMethod) {
    app.auth_setup.form = AuthSetupForm::default();
    app.auth_setup.form.method = method;
    app.auth_setup.form.focused = match method {
        AuthMethod::Password => AuthSetupField::Password,
        AuthMethod::SecurityKey => AuthSetupField::Pin,
        AuthMethod::None => AuthSetupField::Method,
    };
    if method == AuthMethod::SecurityKey {
        app.auth_setup.form.label = format!(
            "Security key {}",
            app.wraps.security_key_wraps().count() + 1
        );
    }
    app.auth_setup.context = SetupContext::AddFactor;
    app.input_mode = InputMode::SetupAuth;
}

pub async fn handle_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    // While a security-key enrol task is running the form is read-only
    // except for Esc-to-quit (the blocking task can't be cancelled
    // mid-touch).
    if app.auth_setup.form.busy {
        if key.code == KeyCode::Esc {
            // Can't cancel an in-flight enrol cleanly — just refuse.
        }
        return Ok(());
    }

    match key.code {
        KeyCode::Esc => match app.auth_setup.context {
            SetupContext::FirstRun => app.should_quit = true,
            SetupContext::AddFactor => {
                app.auth_setup.form = AuthSetupForm::default();
                app.auth_setup.context = SetupContext::FirstRun;
                app.input_mode = InputMode::AuthSettings;
            }
        },
        KeyCode::Tab => app.auth_setup.form.next(app.auth_setup.context),
        KeyCode::BackTab => app.auth_setup.form.prev(app.auth_setup.context),
        KeyCode::Left if app.auth_setup.form.focused == AuthSetupField::Method => {
            app.auth_setup.form.method =
                step_method_prev(app.auth_setup.form.method, app.auth_setup.context);
            app.auth_setup.form.error = None;
            app.auth_setup.form.settle_focus_after_method_change();
        }
        KeyCode::Right if app.auth_setup.form.focused == AuthSetupField::Method => {
            app.auth_setup.form.method =
                step_method_next(app.auth_setup.form.method, app.auth_setup.context);
            app.auth_setup.form.error = None;
            app.auth_setup.form.settle_focus_after_method_change();
        }
        KeyCode::Char(' ') if app.auth_setup.form.focused == AuthSetupField::Method => {
            app.auth_setup.form.method =
                step_method_next(app.auth_setup.form.method, app.auth_setup.context);
            app.auth_setup.form.error = None;
            app.auth_setup.form.settle_focus_after_method_change();
        }
        KeyCode::Enter if app.auth_setup.form.focused == AuthSetupField::Submit => {
            commit(app).await?;
        }
        KeyCode::Enter => app.auth_setup.form.next(app.auth_setup.context),
        KeyCode::Backspace => match app.auth_setup.form.focused {
            AuthSetupField::Password => {
                app.auth_setup.form.password.pop();
            }
            AuthSetupField::Confirm => {
                app.auth_setup.form.confirm.pop();
            }
            AuthSetupField::Pin => {
                app.auth_setup.form.pin.pop();
            }
            AuthSetupField::Label => {
                app.auth_setup.form.label.pop();
            }
            _ => {}
        },
        KeyCode::Char(c) => match app.auth_setup.form.focused {
            AuthSetupField::Password => app.auth_setup.form.password.push(c),
            AuthSetupField::Confirm => app.auth_setup.form.confirm.push(c),
            AuthSetupField::Pin => app.auth_setup.form.pin.push(c),
            AuthSetupField::Label => app.auth_setup.form.label.push(c),
            _ => {}
        },
        _ => {}
    }
    Ok(())
}

pub async fn commit(app: &mut App) -> crate::Result<()> {
    let context = app.auth_setup.context;
    match app.auth_setup.form.method {
        AuthMethod::None => {
            // None is only valid from first-run. Defensive guard.
            if context != SetupContext::FirstRun {
                app.auth_setup.form.error =
                    Some("Use [x] disable encryption from Auth Settings instead".into());
                return Ok(());
            }
            // No encryption: write an empty keys.plain at mode 600 and
            // record settings. No DEK; no wraps.toml.
            app.set_dek(None);
            let mut s = app.settings.unwrap_or_default();
            s.encrypt_keys = false;
            app.settings = Some(s);
            s.save()?;
            let bytes = serde_json::to_vec(app.jwks())?;
            ProjectConfig::save_keys_plain(&bytes)?;
            app.auth_setup.form = AuthSetupForm::default();
            app.auth_setup.context = SetupContext::FirstRun;
            app.input_mode = InputMode::Normal;
            crate::screens::unlock::unlock_plain_agent(app).await;
        }
        AuthMethod::Password => {
            if app.auth_setup.form.password.is_empty() {
                app.auth_setup.form.error = Some("Password cannot be empty".into());
                app.auth_setup.form.focused = AuthSetupField::Password;
                return Ok(());
            }
            if app.auth_setup.form.password != app.auth_setup.form.confirm {
                app.auth_setup.form.error = Some("Passwords do not match".into());
                app.auth_setup.form.focused = AuthSetupField::Confirm;
                return Ok(());
            }

            // Mint a DEK iff there isn't one yet. Re-using an existing DEK
            // is what makes "change password" work without touching keys.enc.
            let freshly_minted = app.dek_clone().is_none();
            let dek = app.dek_clone().unwrap_or_else(Dek::random);
            let (salt, nonce, ct) =
                crypto::wrap_dek_with_password(&dek, &app.auth_setup.form.password)?;
            app.wraps.upsert_password(Wrap::Password {
                salt: wraps::b64_encode(&salt),
                nonce: wraps::b64_encode(&nonce),
                ciphertext: wraps::b64_encode(&ct),
            });
            app.wraps.save()?;
            app.set_dek(Some(dek));
            finalize_factor_addition(app, freshly_minted, context, "Password set").await?;
        }
        AuthMethod::SecurityKey => {
            if app.auth_setup.form.pin.is_empty() {
                app.auth_setup.form.error = Some("FIDO2 PIN cannot be empty".into());
                app.auth_setup.form.focused = AuthSetupField::Pin;
                return Ok(());
            }
            if app.auth_setup.form.label.trim().is_empty() {
                app.auth_setup.form.error = Some("Label cannot be empty".into());
                app.auth_setup.form.focused = AuthSetupField::Label;
                return Ok(());
            }
            let dek = app.dek_clone().unwrap_or_else(Dek::random);
            app.set_dek(Some(dek.clone()));
            app.auth_setup.form.busy = true;
            app.auth_setup.form.error = None;
            // Pull (or generate) the file-level hmac-secret salt. The
            // in-memory mutation lands on disk inside handle_enroll_result
            // after the enrolment succeeds.
            let hmac_salt = app.wraps.get_or_create_security_key_salt();
            let pin = std::mem::take(&mut app.auth_setup.form.pin);
            let label = app.auth_setup.form.label.trim().to_string();
            let tx = app.events.tx.clone();
            tokio::task::spawn_blocking(move || {
                let result = enroll_security_key_blocking(dek, label, Some(pin), hmac_salt);
                let _ = tx.send(AppEvent::SecurityKeyEnrollResult(result));
            });
        }
    }
    Ok(())
}

/// Shared "after a wrap was just persisted" tail. Handles the encryption
/// transition (keys.plain → keys.enc) when this is the first factor, routes
/// back to whichever screen launched SetupAuth, and hands the freshly-
/// derived DEK to the agent so subsequent `ApiCall`s — both in this TUI
/// session and in any concurrent CLI — find it unlocked.
async fn finalize_factor_addition(
    app: &mut App,
    freshly_minted_dek: bool,
    context: SetupContext,
    success_toast: &str,
) -> crate::Result<()> {
    let already_encrypted = matches!(app.settings, Some(Settings { encrypt_keys: true, .. }));

    if !already_encrypted {
        // We just added the first factor while encryption was disabled
        // (or this is first-run with no prior settings.toml). Promote
        // keys.plain → keys.enc and flip the flag.
        let dek = app
            .dek_clone()
            .ok_or_else(|| crate::Error::Crypto("DEK missing".into()))?;
        crate::config::enable_encryption(&dek)?;
        let mut s = app.settings.unwrap_or_default();
        s.encrypt_keys = true;
        app.settings = Some(s);
    } else if freshly_minted_dek {
        // Defensive: should not happen — if encryption was already on,
        // we have a DEK and didn't mint a fresh one. But if state ever
        // drifts, persist the new DEK so keys.enc is readable.
        app.persist_keys()?;
    }

    app.auth_setup.form = AuthSetupForm::default();
    let next_mode = match context {
        SetupContext::FirstRun => InputMode::Normal,
        SetupContext::AddFactor => InputMode::AuthSettings,
    };
    app.auth_setup.context = SetupContext::FirstRun;
    app.input_mode = next_mode;
    // Spawn / wake the agent and seed it with the DEK. Without this the
    // first-run user lands on the dashboard, the ESVs tab fires its
    // initial fetch, and the agent rejects it as Locked — even though the
    // TUI itself has the DEK in memory. Awaited (not spawned) so any
    // refresh that runs straight after sees an unlocked agent.
    crate::screens::unlock::put_dek_to_agent(app).await;
    app.push_toast(ToastKind::Success, success_toast);
    Ok(())
}

/// Called from the dispatcher when the background security-key enrol task
/// reports success/failure. Mirrors the old `handle_security_key_enroll_result`
/// on `App`.
pub async fn handle_enroll_result(app: &mut App, result: std::result::Result<Wrap, String>) {
    let context = app.auth_setup.context;
    let was_dek_minted_for_this_op =
        !matches!(app.settings, Some(Settings { encrypt_keys: true, .. }));

    match result {
        Ok(wrap) => {
            app.wraps.push_security_key(wrap);
            if let Err(e) = app.wraps.save() {
                tracing::error!(error = %e, "wraps.toml save failed after enrol");
                app.push_toast(ToastKind::Error, format!("wraps.toml: {e}"));
                app.wraps.wraps.pop();
                if was_dek_minted_for_this_op {
                    app.set_dek(None);
                }
                app.auth_setup.form.busy = false;
                app.auth_setup.form.error = Some(format!("Save failed: {e}"));
                return;
            }

            app.auth_setup.form.busy = false;
            if let Err(e) = finalize_factor_addition(
                app,
                was_dek_minted_for_this_op,
                context,
                "Security key enrolled",
            )
            .await
            {
                tracing::error!(error = %e, "finalize_factor_addition failed");
                app.auth_setup.form.error = Some(format!("Finalise: {e}"));
            }
        }
        Err(msg) => {
            tracing::error!(error = %msg, "security key enrolment failed");
            if was_dek_minted_for_this_op {
                app.set_dek(None);
            }
            app.auth_setup.form.busy = false;
            app.auth_setup.form.error = Some(format!("Enrol failed: {msg}"));
        }
    }
}
