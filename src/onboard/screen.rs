//! Onboarding state, nested modes/events, and the top-level dispatch that
//! routes each menu choice to its flow. Per-flow key handling, bootstrap
//! tasks, and completion handlers live in the flow files (`cookie`,
//! `userpass`, `paste`, `log_only`); cross-flow glue and persistence live in
//! [`super::common`].

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::event::ToastKind;
use crate::app::{App, InputMode};
use crate::config::tenant::{Tenant, TenantTheme};
use crate::logs::LogKeyPair;

use super::common::{
    clear_onboard_forms, handle_sa_created, menu_option_count, path_for_index,
    persist_tenant_overwriting,
};
use super::cookie::CookieForm;
use super::log_only::LogOnlyForm;
use super::paste::PasteForm;
use super::userpass::UpForm;
use super::{OnboardPath, cookie, log_only, paste, userpass};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Menu,
    Cookie,
    UserPass,
    Paste,
    LogOnly,
    OverwriteConfirm,
}

#[derive(Debug)]
pub enum PendingConfirm {
    Cookie,
    UserPass,
    LogOnly,
    Paste {
        tenant: Tenant,
        jwk: serde_json::Value,
    },
}

#[derive(Debug)]
pub enum ProdAction {
    SaveTenant {
        tenant: Tenant,
        jwk: Option<serde_json::Value>,
        log_key: Option<LogKeyPair>,
    },
}

pub fn execute_prod_action(app: &mut App, action: ProdAction) {
    match action {
        ProdAction::SaveTenant {
            tenant,
            jwk,
            log_key,
        } => match persist_tenant_overwriting(app, tenant, jwk, log_key) {
            Ok(()) => app.push_toast(ToastKind::Success, "Tenant saved"),
            Err(e) => app.push_toast(ToastKind::Error, format!("Save failed: {e}")),
        },
    }
}

pub fn resume_mode(_app: &App, _action: &ProdAction) -> InputMode {
    InputMode::Normal
}

#[derive(Debug)]
pub enum Event {
    /// Pattern 2: the AM authentication journey returned a callback we need
    /// extra user input to satisfy (TOTP). `body` is the JSON to POST back
    /// once the user supplies the missing value.
    AuthProgress {
        onboard_id: uuid::Uuid,
        body: serde_json::Value,
        prompt: String,
    },
    /// Onboarding failed somewhere in the background task.
    Error {
        onboard_id: uuid::Uuid,
        message: String,
    },
    /// Service account creation completed. `onboard_id` matches the
    /// `pending_id` stamped on the App when the bootstrap was kicked off;
    /// the handler must drop the event when the id does not match because the
    /// user cancelled or a stale completion arrived after another bootstrap.
    /// The task carries `base_url` + `theme` so the handler does not have to
    /// inspect a form that may already have been cleared.
    ServiceAccountReady {
        onboard_id: uuid::Uuid,
        tenant_name: String,
        base_url: String,
        theme: TenantTheme,
        sa_id: String,
        jwk: serde_json::Value,
        log_key: Option<LogKeyPair>,
    },
    /// A log API key was created from an admin-user session without creating
    /// a service account.
    LogOnlyReady {
        onboard_id: uuid::Uuid,
        tenant_name: String,
        base_url: String,
        theme: TenantTheme,
        log_key: LogKeyPair,
    },
}

pub fn apply_event(app: &mut App, event: Event) -> crate::Result<()> {
    match event {
        Event::AuthProgress {
            onboard_id,
            body,
            prompt,
        } => {
            handle_auth_progress(app, onboard_id, body, prompt);
            Ok(())
        }
        Event::Error {
            onboard_id,
            message,
        } => {
            tracing::error!(error = %message, "onboard error");
            handle_onboard_error(app, onboard_id, message);
            Ok(())
        }
        Event::ServiceAccountReady {
            onboard_id,
            tenant_name,
            base_url,
            theme,
            sa_id,
            jwk,
            log_key,
        } => handle_sa_created(
            app,
            onboard_id,
            tenant_name,
            base_url,
            theme,
            sa_id,
            jwk,
            log_key,
        ),
        Event::LogOnlyReady {
            onboard_id,
            tenant_name,
            base_url,
            theme,
            log_key,
        } => log_only::handle_created(app, onboard_id, tenant_name, base_url, theme, log_key),
    }
}

pub async fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) -> crate::Result<()> {
    match mode {
        Mode::Menu => handle_menu_key(app, key).await?,
        Mode::Cookie => cookie::handle_key(app, key).await?,
        Mode::UserPass => userpass::handle_key(app, key).await?,
        Mode::Paste => paste::handle_key(app, key).await?,
        Mode::LogOnly => log_only::handle_key(app, key).await?,
        Mode::OverwriteConfirm => handle_overwrite_key(app, key)?,
    }
    Ok(())
}

#[derive(Debug, Default)]
pub struct State {
    pub menu_idx: usize,
    pub cookie_form: Option<CookieForm>,
    pub up_form: Option<UpForm>,
    pub paste_form: Option<PasteForm>,
    pub log_only_form: Option<LogOnlyForm>,

    /// UUID stamped on the in-flight bootstrap task. Set when the user
    /// kicks off cookie, userpass, or log-only bootstrap, cleared on
    /// Esc-cancel. Completion handlers drop non-matching ids instead of
    /// persisting a tenant the user no longer wants.
    pub pending_id: Option<uuid::Uuid>,

    /// Pattern 2: the in-flight callback JSON we POST'd that needs an
    /// extra prompt (TOTP).
    pub pending_callback_body: Option<serde_json::Value>,

    /// Pending pre-mint overwrite confirmation. The original form stays in
    /// place so confirming can resume the selected bootstrap without losing
    /// user input.
    pub pending_confirm: Option<PendingConfirm>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }
}

// ---- Menu ----

pub async fn handle_menu_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let option_count = menu_option_count(app.has_env_creds);
    let max_idx = option_count - 1;
    match key.code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char('j') | KeyCode::Down if app.onboard.menu_idx < max_idx => {
            app.onboard.menu_idx += 1;
        }
        KeyCode::Char('k') | KeyCode::Up if app.onboard.menu_idx > 0 => {
            app.onboard.menu_idx -= 1;
        }
        KeyCode::Enter => enter_choice(app, app.onboard.menu_idx).await?,
        KeyCode::Char(number @ '1'..='5') => {
            let idx = number as usize - '1' as usize;
            if idx < option_count {
                enter_choice(app, idx).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn number_range(count: usize) -> Option<String> {
    let max = count.min(9);
    match max {
        0 => None,
        1 => Some("1".to_string()),
        _ => Some(format!("1-{max}")),
    }
}

pub fn help_lines(mode: Mode, has_env_creds: bool) -> Option<Vec<(&'static str, &'static str)>> {
    match mode {
        Mode::Menu => {
            let mut out = vec![
                ("Enter", "choose selected method"),
                ("Esc", "cancel"),
                ("↑/↓", "move selection"),
            ];
            let count = menu_option_count(has_env_creds);
            if let Some(range) = number_range(count) {
                out.push((Box::leak(range.into_boxed_str()), "choose numbered method"));
            }
            out.push(("F1/?", "show keybinds"));
            Some(out)
        }
        Mode::Cookie | Mode::Paste | Mode::LogOnly => Some(vec![
            ("Enter", "advance or submit"),
            ("Esc", "go back"),
            ("Tab/Shift-Tab", "move between fields"),
            ("←/→", "change theme when the Theme selector is focused"),
            ("Arrows/Home/End", "move cursor"),
            ("Backspace/Delete", "delete text"),
            ("F1", "show keybinds"),
        ]),
        Mode::UserPass => Some(vec![
            ("Enter", "advance or submit"),
            ("Esc", "quit"),
            ("Tab/Shift-Tab", "move between fields"),
            ("←/→ or Space", "change authentication method"),
            ("Type", "enter password"),
            ("Backspace", "delete character"),
            ("F1", "show keybinds"),
        ]),
        Mode::OverwriteConfirm => Some(vec![("y", "overwrite"), ("n/Esc", "cancel")]),
    }
}

async fn enter_choice(app: &mut App, idx: usize) -> crate::Result<()> {
    match path_for_index(idx, app.has_env_creds) {
        Some(OnboardPath::Cookie) => {
            app.onboard.pending_confirm = None;
            app.onboard.cookie_form = Some(CookieForm::default());
            app.input_mode = InputMode::Onboard(Mode::Cookie);
        }
        Some(OnboardPath::UserPass) => {
            app.onboard.pending_confirm = None;
            app.onboard.up_form = Some(UpForm::default());
            app.input_mode = InputMode::Onboard(Mode::UserPass);
        }
        Some(OnboardPath::Paste) => {
            app.onboard.pending_confirm = None;
            app.onboard.paste_form = Some(PasteForm::default());
            app.input_mode = InputMode::Onboard(Mode::Paste);
        }
        Some(OnboardPath::Envrc) => {
            import_env_creds(app).await?;
        }
        Some(OnboardPath::LogOnly) => {
            app.onboard.pending_confirm = None;
            app.onboard.log_only_form = Some(LogOnlyForm::default());
            app.input_mode = InputMode::Onboard(Mode::LogOnly);
        }
        None => {}
    }
    Ok(())
}

pub(crate) fn pending_confirm_name(app: &App) -> Option<&str> {
    match app.onboard.pending_confirm.as_ref()? {
        PendingConfirm::Cookie => app
            .onboard
            .cookie_form
            .as_ref()
            .map(|form| form.name.trimmed()),
        PendingConfirm::UserPass => app.onboard.up_form.as_ref().map(|form| form.name.trimmed()),
        PendingConfirm::LogOnly => app
            .onboard
            .log_only_form
            .as_ref()
            .map(|form| form.name.trimmed()),
        PendingConfirm::Paste { tenant, .. } => Some(tenant.name.as_str()),
    }
}

// ---- Overwrite confirmation ----

pub fn handle_overwrite_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Some(pending) = app.onboard.pending_confirm.take() {
                match pending {
                    PendingConfirm::Cookie => {
                        app.input_mode = InputMode::Onboard(Mode::Cookie);
                        cookie::start_bootstrap(app);
                    }
                    PendingConfirm::UserPass => {
                        app.input_mode = InputMode::Onboard(Mode::UserPass);
                        userpass::start_bootstrap(app);
                    }
                    PendingConfirm::LogOnly => {
                        app.input_mode = InputMode::Onboard(Mode::LogOnly);
                        log_only::start_bootstrap(app);
                    }
                    PendingConfirm::Paste { tenant, jwk } => {
                        paste::persist(app, tenant, jwk);
                    }
                }
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.onboard.pending_confirm = None;
            clear_onboard_forms(app);
            app.input_mode = InputMode::Normal;
            app.push_toast(ToastKind::Info, "Onboarding cancelled");
        }
        _ => {}
    }
    Ok(())
}

// ---- Cross-flow background-task event handlers ----

pub fn handle_auth_progress(
    app: &mut App,
    onboard_id: uuid::Uuid,
    body: serde_json::Value,
    prompt: String,
) {
    if app.onboard.pending_id != Some(onboard_id) {
        tracing::debug!(
            event_id = %onboard_id,
            pending = ?app.onboard.pending_id,
            "dropping stale onboarding auth progress"
        );
        return;
    }
    if let Some(form) = &mut app.onboard.up_form {
        form.pending_prompt = Some(prompt);
        form.status = None;
    }
    app.onboard.pending_callback_body = Some(body);
}

pub fn handle_onboard_error(app: &mut App, onboard_id: uuid::Uuid, msg: String) {
    if app.onboard.pending_id != Some(onboard_id) {
        tracing::debug!(
            event_id = %onboard_id,
            pending = ?app.onboard.pending_id,
            "dropping stale onboarding error"
        );
        return;
    }
    app.onboard.pending_id = None;
    app.onboard.pending_callback_body = None;
    if let Some(form) = &mut app.onboard.cookie_form {
        form.busy = false;
        form.error = Some(msg.clone());
        form.status = None;
    }
    if let Some(form) = &mut app.onboard.up_form {
        form.busy = false;
        form.error = Some(msg.clone());
        form.status = None;
        form.pending_prompt = None;
    }
    if let Some(form) = &mut app.onboard.log_only_form {
        form.busy = false;
        form.error = Some(msg.clone());
        form.status = None;
    }
    app.push_toast(ToastKind::Error, msg);
}

// ---- Envrc: direct sandbox import from the development environment ----

async fn import_env_creds(app: &mut App) -> crate::Result<()> {
    let base_url = std::env::var("TENANT_BASE_URL")
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_string();
    let sa_id = std::env::var("SERVICE_ACCOUNT_ID").unwrap_or_default();
    let jwk_str = std::env::var("SERVICE_ACCOUNT_KEY").unwrap_or_default();

    if base_url.is_empty() || sa_id.is_empty() || jwk_str.is_empty() {
        app.push_toast(
            ToastKind::Error,
            "Missing env vars — need TENANT_BASE_URL, SERVICE_ACCOUNT_ID, SERVICE_ACCOUNT_KEY",
        );
        app.input_mode = InputMode::Normal;
        return Ok(());
    }

    let jwk: serde_json::Value = match serde_json::from_str(&jwk_str) {
        Ok(v) => v,
        Err(e) => {
            app.push_toast(ToastKind::Error, format!("JWK parse error: {e}"));
            app.input_mode = InputMode::Normal;
            return Ok(());
        }
    };

    let scopes: Vec<String> = super::bootstrap::SA_SCOPES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let tenant = Tenant {
        name: "sandbox".into(),
        base_url,
        theme: TenantTheme::Sandbox,
        sa_id: Some(sa_id),
        scopes,
    };

    match persist_tenant_overwriting(app, tenant, Some(jwk), None) {
        Ok(()) => {
            app.push_toast(
                ToastKind::Success,
                "Imported sandbox tenant from environment",
            );
            app.input_mode = InputMode::Normal;
        }
        Err(e) => {
            app.push_toast(ToastKind::Error, format!("Import failed: {e}"));
            app.input_mode = InputMode::Normal;
        }
    }
    Ok(())
}
