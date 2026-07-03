//! Tenant onboarding state, nested modes/events, key handling, persistence,
//! and the background bootstrap tasks that hit AIC before a tenant exists.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::event::{AppEvent, ToastKind};
use crate::app::prod_confirm::PendingProdAction;
use crate::app::{App, InputMode};
use crate::config::ProjectConfig;
use crate::config::tenant::{Tenant, TenantTheme};
use crate::logs::LogKeyPair;

use super::OnboardPath;
use super::cookie::{CookieField, CookieForm};
use super::log_only::{LogOnlyField, LogOnlyForm, LogOnlyIntent};
use super::paste::{PasteField, PasteForm};
use super::userpass::{CallbackOutcome, UpField, UpForm};

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
        } => handle_log_only_created(app, onboard_id, tenant_name, base_url, theme, log_key),
    }
}

pub async fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) -> crate::Result<()> {
    match mode {
        Mode::Menu => handle_menu_key(app, key).await?,
        Mode::Cookie => handle_cookie_key(app, key).await?,
        Mode::UserPass => handle_up_key(app, key).await?,
        Mode::Paste => handle_paste_key(app, key).await?,
        Mode::LogOnly => handle_log_only_key(app, key).await?,
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

pub(crate) fn menu_option_count(has_env_creds: bool) -> usize {
    if has_env_creds { 5 } else { 4 }
}

pub(crate) fn log_only_menu_number(has_env_creds: bool) -> usize {
    menu_option_count(has_env_creds)
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

fn path_for_index(idx: usize, has_env_creds: bool) -> Option<OnboardPath> {
    match (idx, has_env_creds) {
        (0, _) => Some(OnboardPath::Cookie),
        (1, _) => Some(OnboardPath::UserPass),
        (2, _) => Some(OnboardPath::Paste),
        (3, true) => Some(OnboardPath::Envrc),
        (3, false) | (4, true) => Some(OnboardPath::LogOnly),
        _ => None,
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

fn tenant_name_exists(tenants: &[Tenant], name: &str) -> bool {
    tenants.iter().any(|tenant| tenant.name == name)
}

fn queue_overwrite_confirm(app: &mut App, pending: PendingConfirm) {
    app.onboard.pending_confirm = Some(pending);
    app.input_mode = InputMode::Onboard(Mode::OverwriteConfirm);
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

// ---- Pattern 1: cookie ----

pub async fn handle_cookie_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let form = match &mut app.onboard.cookie_form {
        Some(f) => f,
        None => return Ok(()),
    };
    if form.busy {
        // Allow Esc to cancel while busy
        if key.code == KeyCode::Esc {
            form.busy = false;
            app.onboard.cookie_form = None;
            // Drop the in-flight bootstrap's id so its
            // Any bootstrap completion that still arrives is
            // recognised as stale and ignored.
            app.onboard.pending_id = None;
            app.input_mode = InputMode::Onboard(Mode::Menu);
        }
        return Ok(());
    }

    // Normalise the domain field whenever focus leaves it.
    let leaving_domain = matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter)
        && form.focused == CookieField::Domain;
    if leaving_domain {
        let cleaned = super::normalise_domain(&form.domain.value);
        form.domain.set(cleaned);
    }

    match key.code {
        KeyCode::Esc => {
            app.onboard.cookie_form = None;
            app.input_mode = InputMode::Onboard(Mode::Menu);
        }
        KeyCode::Tab => form.focused = form.focused.next(),
        KeyCode::BackTab => form.focused = form.focused.prev(),
        KeyCode::Left if form.focused == CookieField::Theme => form.cycle_theme_backward(),
        KeyCode::Right if form.focused == CookieField::Theme => form.cycle_theme_forward(),
        KeyCode::Enter if form.focused == CookieField::Submit => {
            if let Err(e) = form.validate() {
                form.error = Some(e);
            } else {
                let name = form.name.trimmed().to_string();
                form.error = None;
                if tenant_name_exists(&app.tenants, &name) {
                    queue_overwrite_confirm(app, PendingConfirm::Cookie);
                } else {
                    start_cookie_bootstrap(app);
                }
            }
        }
        KeyCode::Enter => form.focused = form.focused.next(),
        _ => {
            if let Some(f) = form.focused_field_mut() {
                f.handle_key(&key);
            }
        }
    }
    Ok(())
}

fn start_cookie_bootstrap(app: &mut App) {
    let form = match &mut app.onboard.cookie_form {
        Some(f) => f,
        None => return,
    };
    form.busy = true;
    form.status = Some("Authenticating…".into());
    let name = form.name.trimmed().to_string();
    let base_url = form.normalised_base_url();
    let theme = form.theme;
    let cookie_name = form.cookie_name.trimmed().to_string();
    let cookie_value = form.cookie_value.trimmed().to_string();
    let tx = app.events.tx.clone();
    let onboard_id = uuid::Uuid::new_v4();
    app.onboard.pending_id = Some(onboard_id);

    tokio::spawn(async move {
        run_bootstrap_from_cookie(
            onboard_id,
            name,
            base_url,
            theme,
            cookie_name,
            cookie_value,
            tx,
        )
        .await;
    });
}

// ---- Log-only environment ----

pub async fn handle_log_only_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let form = match &mut app.onboard.log_only_form {
        Some(form) => form,
        None => return Ok(()),
    };
    if form.busy {
        if key.code == KeyCode::Esc {
            form.busy = false;
            app.onboard.log_only_form = None;
            app.onboard.pending_id = None;
            app.input_mode = InputMode::Onboard(Mode::Menu);
        }
        return Ok(());
    }

    let leaving_domain = matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter)
        && form.focused == LogOnlyField::Domain;
    if leaving_domain {
        let cleaned = super::normalise_domain(&form.domain.value);
        form.domain.set(cleaned);
    }

    match key.code {
        KeyCode::Esc => {
            app.onboard.log_only_form = None;
            app.input_mode = InputMode::Onboard(Mode::Menu);
        }
        KeyCode::Tab => form.focused = form.focused.next(),
        KeyCode::BackTab => form.focused = form.focused.prev(),
        KeyCode::Left if form.focused == LogOnlyField::Theme => form.cycle_theme_backward(),
        KeyCode::Right if form.focused == LogOnlyField::Theme => form.cycle_theme_forward(),
        KeyCode::Enter if form.focused == LogOnlyField::Submit => match form.validate() {
            Ok(intent) => {
                form.error = None;
                if tenant_name_exists(&app.tenants, &intent.tenant_name) {
                    queue_overwrite_confirm(app, PendingConfirm::LogOnly);
                } else {
                    start_log_only_bootstrap(app);
                }
            }
            Err(error) => form.error = Some(error),
        },
        KeyCode::Enter => form.focused = form.focused.next(),
        _ => {
            if let Some(field) = form.focused_field_mut() {
                field.handle_key(&key);
            }
        }
    }
    Ok(())
}

fn start_log_only_bootstrap(app: &mut App) {
    let form = match &mut app.onboard.log_only_form {
        Some(form) => form,
        None => return,
    };
    let intent = match form.validate() {
        Ok(intent) => intent,
        Err(error) => {
            form.error = Some(error);
            return;
        }
    };
    form.busy = true;
    form.status = Some("Authenticating and creating log API key…".into());

    let tx = app.events.tx.clone();
    let onboard_id = uuid::Uuid::new_v4();
    app.onboard.pending_id = Some(onboard_id);
    tokio::spawn(async move {
        run_bootstrap_log_only(onboard_id, intent, tx).await;
    });
}

// ---- Pattern 2: userpass ----

pub async fn handle_up_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let form = match &mut app.onboard.up_form {
        Some(f) => f,
        None => return Ok(()),
    };

    // OTP / extra prompt is in flight — only the prompt input listens.
    if form.pending_prompt.is_some() {
        match key.code {
            KeyCode::Esc => {
                form.pending_prompt = None;
                form.prompt_input.clear();
                form.busy = false;
                form.status = None;
                app.onboard.pending_callback_body = None;
            }
            KeyCode::Enter if !form.prompt_input.is_empty() => {
                let extra = form.prompt_input.clone();
                form.prompt_input.clear();
                form.pending_prompt = None;
                form.status = Some("Continuing authentication…".into());
                continue_up_with_extra(app, extra);
            }
            KeyCode::Backspace => {
                form.prompt_input.pop();
            }
            KeyCode::Char(c) => {
                form.prompt_input.push(c);
            }
            _ => {}
        }
        return Ok(());
    }

    if form.busy {
        if key.code == KeyCode::Esc {
            form.busy = false;
            form.status = None;
            app.onboard.up_form = None;
            app.onboard.pending_id = None;
            app.input_mode = InputMode::Onboard(Mode::Menu);
        }
        return Ok(());
    }

    let leaving_domain = matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter)
        && form.focused == UpField::Domain;
    if leaving_domain {
        let cleaned = super::normalise_domain(&form.domain.value);
        form.domain.set(cleaned);
    }

    match key.code {
        KeyCode::Esc => {
            app.onboard.up_form = None;
            app.input_mode = InputMode::Onboard(Mode::Menu);
        }
        KeyCode::Tab => form.focused = form.focused.next(),
        KeyCode::BackTab => form.focused = form.focused.prev(),
        KeyCode::Left if form.focused == UpField::Theme => form.cycle_theme_backward(),
        KeyCode::Right if form.focused == UpField::Theme => form.cycle_theme_forward(),
        KeyCode::Enter if form.focused == UpField::Submit => {
            if let Err(e) = form.validate() {
                form.error = Some(e);
            } else {
                let name = form.name.trimmed().to_string();
                form.error = None;
                if tenant_name_exists(&app.tenants, &name) {
                    queue_overwrite_confirm(app, PendingConfirm::UserPass);
                } else {
                    start_up_bootstrap(app);
                }
            }
        }
        KeyCode::Enter => form.focused = form.focused.next(),
        _ => {
            if let Some(f) = form.focused_field_mut() {
                f.handle_key(&key);
            }
        }
    }
    Ok(())
}

fn start_up_bootstrap(app: &mut App) {
    let form = match &mut app.onboard.up_form {
        Some(f) => f,
        None => return,
    };
    form.busy = true;
    form.status = Some("Starting authentication journey…".into());
    let name = form.name.trimmed().to_string();
    let base_url = form.normalised_base_url();
    let theme = form.theme;
    let username = form.username.trimmed().to_string();
    let password = form.password.value.clone();
    let realm_path = form.realm_path();
    let tx = app.events.tx.clone();
    let scopes: Vec<String> = super::bootstrap::SA_SCOPES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let onboard_id = uuid::Uuid::new_v4();
    app.onboard.pending_id = Some(onboard_id);

    tokio::spawn(async move {
        run_bootstrap_from_userpass(
            onboard_id, name, base_url, theme, realm_path, username, password, None, None, scopes,
            tx,
        )
        .await;
    });
}

fn continue_up_with_extra(app: &mut App, extra: String) {
    let body = match app.onboard.pending_callback_body.take() {
        Some(b) => b,
        None => return,
    };
    let form = match &mut app.onboard.up_form {
        Some(f) => f,
        None => return,
    };
    let name = form.name.trimmed().to_string();
    let base_url = form.normalised_base_url();
    let theme = form.theme;
    let username = form.username.trimmed().to_string();
    let password = form.password.value.clone();
    let realm_path = form.realm_path();
    let scopes: Vec<String> = super::bootstrap::SA_SCOPES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let tx = app.events.tx.clone();
    // Re-use the existing onboard id — this is a continuation of the same
    // user-initiated bootstrap. If the user cancelled and the id is gone,
    // there's nothing to continue.
    let Some(onboard_id) = app.onboard.pending_id else {
        return;
    };
    tokio::spawn(async move {
        run_bootstrap_from_userpass(
            onboard_id,
            name,
            base_url,
            theme,
            realm_path,
            username,
            password,
            Some(body),
            Some(extra),
            scopes,
            tx,
        )
        .await;
    });
}

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

// ---- Pattern 3: paste ----

pub async fn handle_paste_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let form = match &mut app.onboard.paste_form {
        Some(f) => f,
        None => return Ok(()),
    };

    let leaving_domain = matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter)
        && form.focused == PasteField::Domain;
    if leaving_domain {
        let cleaned = super::normalise_domain(&form.domain.value);
        form.domain.set(cleaned);
    }

    match key.code {
        KeyCode::Esc => {
            app.onboard.paste_form = None;
            app.input_mode = InputMode::Onboard(Mode::Menu);
        }
        KeyCode::Tab => form.focused = form.focused.next(),
        KeyCode::BackTab => form.focused = form.focused.prev(),
        KeyCode::Left if form.focused == PasteField::Theme => form.cycle_theme_backward(),
        KeyCode::Right if form.focused == PasteField::Theme => form.cycle_theme_forward(),
        KeyCode::Enter if form.focused == PasteField::Submit => {
            let jwk = match form.validate() {
                Ok(v) => v,
                Err(e) => {
                    form.error = Some(e);
                    return Ok(());
                }
            };
            let tenant = form.into_tenant();
            if tenant_name_exists(&app.tenants, &tenant.name) {
                form.error = None;
                queue_overwrite_confirm(app, PendingConfirm::Paste { tenant, jwk });
            } else {
                persist_pasted_tenant(app, tenant, jwk);
            }
        }
        KeyCode::Enter if form.is_jwk_field() => {
            form.jwk_input.push_newline();
        }
        KeyCode::Enter => form.focused = form.focused.next(),
        _ => {
            if let Some(f) = form.focused_field_mut() {
                f.handle_key(&key);
            }
        }
    }
    Ok(())
}

fn persist_pasted_tenant(app: &mut App, tenant: Tenant, jwk: serde_json::Value) {
    app.onboard.paste_form = None;
    if tenant.theme == TenantTheme::Production {
        app.prod_confirm.pending = Some(PendingProdAction::Onboard(ProdAction::SaveTenant {
            tenant,
            jwk: Some(jwk),
            log_key: None,
        }));
        app.input_mode = InputMode::ProdConfirm;
        return;
    }

    match persist_tenant_overwriting(app, tenant, Some(jwk), None) {
        Ok(()) => app.push_toast(ToastKind::Success, "Tenant saved"),
        Err(e) => app.push_toast(ToastKind::Error, format!("Save failed: {e}")),
    }
    app.input_mode = InputMode::Normal;
}

// ---- Overwrite / SA-created ----

pub fn handle_overwrite_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Some(pending) = app.onboard.pending_confirm.take() {
                match pending {
                    PendingConfirm::Cookie => {
                        app.input_mode = InputMode::Onboard(Mode::Cookie);
                        start_cookie_bootstrap(app);
                    }
                    PendingConfirm::UserPass => {
                        app.input_mode = InputMode::Onboard(Mode::UserPass);
                        start_up_bootstrap(app);
                    }
                    PendingConfirm::LogOnly => {
                        app.input_mode = InputMode::Onboard(Mode::LogOnly);
                        start_log_only_bootstrap(app);
                    }
                    PendingConfirm::Paste { tenant, jwk } => {
                        persist_pasted_tenant(app, tenant, jwk);
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

fn clear_onboard_forms(app: &mut App) {
    app.onboard.cookie_form = None;
    app.onboard.up_form = None;
    app.onboard.paste_form = None;
    app.onboard.log_only_form = None;
    app.onboard.pending_id = None;
    app.onboard.pending_callback_body = None;
}

#[allow(clippy::too_many_arguments)]
pub fn handle_sa_created(
    app: &mut App,
    onboard_id: uuid::Uuid,
    tenant_name: String,
    base_url: String,
    theme: TenantTheme,
    sa_id: String,
    jwk: serde_json::Value,
    log_key: Option<LogKeyPair>,
) -> crate::Result<()> {
    // Drop the event if it doesn't match the bootstrap we're waiting on.
    if app.onboard.pending_id != Some(onboard_id) {
        tracing::debug!(
            event_id = %onboard_id,
            pending = ?app.onboard.pending_id,
            "dropping stale service-account completion"
        );
        return Ok(());
    }
    app.onboard.pending_id = None;

    let scopes: Vec<String> = super::bootstrap::SA_SCOPES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let tenant = Tenant {
        name: tenant_name,
        base_url,
        theme,
        sa_id: Some(sa_id),
        scopes,
    };

    // Clear in-flight forms.
    app.onboard.cookie_form = None;
    app.onboard.up_form = None;
    app.onboard.pending_callback_body = None;

    if tenant.theme == TenantTheme::Production {
        app.prod_confirm.pending = Some(PendingProdAction::Onboard(ProdAction::SaveTenant {
            tenant,
            jwk: Some(jwk),
            log_key,
        }));
        app.input_mode = InputMode::ProdConfirm;
        return Ok(());
    }

    match persist_tenant_overwriting(app, tenant, Some(jwk), log_key) {
        Ok(()) => {
            app.push_toast(ToastKind::Success, "Tenant saved");
            app.input_mode = InputMode::Normal;
        }
        Err(e) => {
            app.push_toast(ToastKind::Error, format!("Save failed: {e}"));
            app.input_mode = InputMode::Normal;
        }
    }
    Ok(())
}

pub fn handle_log_only_created(
    app: &mut App,
    onboard_id: uuid::Uuid,
    tenant_name: String,
    base_url: String,
    theme: TenantTheme,
    log_key: LogKeyPair,
) -> crate::Result<()> {
    if app.onboard.pending_id != Some(onboard_id) {
        tracing::debug!(
            event_id = %onboard_id,
            pending = ?app.onboard.pending_id,
            "dropping stale log-only completion"
        );
        return Ok(());
    }
    app.onboard.pending_id = None;
    app.onboard.log_only_form = None;

    let tenant = Tenant {
        name: tenant_name,
        base_url,
        theme,
        sa_id: None,
        scopes: Vec::new(),
    };

    if tenant.theme == TenantTheme::Production {
        app.prod_confirm.pending = Some(PendingProdAction::Onboard(ProdAction::SaveTenant {
            tenant,
            jwk: None,
            log_key: Some(log_key),
        }));
        app.input_mode = InputMode::ProdConfirm;
        return Ok(());
    }

    match persist_tenant_overwriting(app, tenant, None, Some(log_key)) {
        Ok(()) => {
            app.push_toast(ToastKind::Success, "Log-only environment saved");
            app.input_mode = InputMode::Normal;
        }
        Err(error) => {
            app.push_toast(ToastKind::Error, format!("Save failed: {error}"));
            app.input_mode = InputMode::Normal;
        }
    }
    Ok(())
}

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

// ---- Persist helpers (jwks + tenants config) ----

/// Save a tenant outright — replacing any existing entry with the same
/// name. Caller is responsible for confirming the overwrite before calling.
pub(crate) fn persist_tenant_overwriting(
    app: &mut App,
    tenant: Tenant,
    jwk: Option<serde_json::Value>,
    log_key: Option<LogKeyPair>,
) -> crate::Result<()> {
    let tenant_name = tenant.name.clone();
    if let Some(jwk) = jwk {
        app.save_jwk(&tenant.name, jwk)?;
    }

    // Replace any existing entry with the same name, or append.
    if let Some(idx) = app.tenants.iter().position(|t| t.name == tenant.name) {
        app.tenants[idx] = tenant.clone();
        app.set_active_tenant(idx);
    } else {
        app.tenants.push(tenant.clone());
        app.set_active_tenant(app.tenants.len() - 1);
    }

    let project = app
        .config
        .as_ref()
        .map(|c| c.project.clone())
        .unwrap_or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "aic-project".into())
        });

    let default_tenant = app
        .tenants
        .first()
        .map(|t| t.name.clone())
        .unwrap_or_default();
    let config = ProjectConfig {
        project,
        default_tenant,
        tenants: app.tenants.clone(),
    };
    config.save()?;
    app.config = Some(config);

    if let Some(log_key) = log_key {
        app.save_log_key(&tenant_name, log_key)?;
    }

    // Kick the ESVs tab to fetch for the just-added tenant. Without this
    // the user lands on the dashboard after a fresh import and has to
    // wait up to 30s for the next poll tick — feels broken even though
    // it isn't.
    crate::esv::ops::refresh(app, false);
    Ok(())
}

// ---- Background bootstrap tasks ----

fn send_onboard_error(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    onboard_id: uuid::Uuid,
    message: impl Into<String>,
) {
    let _ = tx.send(AppEvent::Onboard(Event::Error {
        onboard_id,
        message: message.into(),
    }));
}

async fn run_bootstrap_from_cookie(
    onboard_id: uuid::Uuid,
    tenant_name: String,
    base_url: String,
    theme: TenantTheme,
    cookie_name: String,
    session_value: String,
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
) {
    use super::bootstrap::*;
    let http = match no_redirect_client() {
        Ok(c) => c,
        Err(e) => {
            send_onboard_error(&tx, onboard_id, format!("HTTP client init: {e}"));
            return;
        }
    };
    let kid = uuid::Uuid::new_v4().to_string();
    let priv_jwk = match generate_rsa_jwk(&kid) {
        Ok(j) => j,
        Err(e) => {
            send_onboard_error(&tx, onboard_id, format!("RSA keygen: {e}"));
            return;
        }
    };
    let pub_jwk = crate::aic::auth::public_jwk(&priv_jwk);
    let bearer = match session_to_bearer(&http, &base_url, &cookie_name, &session_value).await {
        Ok(b) => b,
        Err(e) => {
            send_onboard_error(&tx, onboard_id, format!("authorize/token: {e}"));
            return;
        }
    };
    let minted = match super::bootstrap::mint_log_key_from_bearer(
        &http,
        &base_url,
        &tenant_name,
        &bearer,
        None,
    )
    .await
    {
        Ok(minted) => minted,
        Err(e) => {
            send_onboard_error(&tx, onboard_id, format!("log API key create: {e}"));
            return;
        }
    };
    let sa_id = match create_service_account(
        &http,
        &base_url,
        &bearer,
        &minted.credential_name,
        &format!("Created by aic-edit for {tenant_name}"),
        &pub_jwk,
    )
    .await
    {
        Ok(id) => id,
        Err(e) => {
            send_onboard_error(&tx, onboard_id, format!("SA create: {e}"));
            return;
        }
    };
    let log_key = Some(minted.key);
    let _ = tx.send(AppEvent::Onboard(Event::ServiceAccountReady {
        onboard_id,
        tenant_name,
        base_url,
        theme,
        sa_id,
        jwk: priv_jwk,
        log_key,
    }));
}

async fn run_bootstrap_log_only(
    onboard_id: uuid::Uuid,
    intent: LogOnlyIntent,
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
) {
    let http = match super::bootstrap::no_redirect_client() {
        Ok(client) => client,
        Err(error) => {
            send_onboard_error(&tx, onboard_id, format!("HTTP client init: {error}"));
            return;
        }
    };
    let log_key = match super::bootstrap::mint_log_key_via_session(
        &http,
        &intent.base_url,
        Some(&intent.cookie_name),
        &intent.cookie_value,
        &intent.tenant_name,
        None,
    )
    .await
    {
        Ok(minted) => minted.key,
        Err(error) => {
            send_onboard_error(&tx, onboard_id, format!("log API key create: {error}"));
            return;
        }
    };
    tracing::info!(
        tenant = intent.tenant_name,
        api_key_id = %log_key.api_key_id,
        "log-only API key provisioned during onboarding"
    );
    let _ = tx.send(AppEvent::Onboard(Event::LogOnlyReady {
        onboard_id,
        tenant_name: intent.tenant_name,
        base_url: intent.base_url,
        theme: intent.theme,
        log_key,
    }));
}

#[allow(clippy::too_many_arguments)]
async fn run_bootstrap_from_userpass(
    onboard_id: uuid::Uuid,
    tenant_name: String,
    base_url: String,
    theme: TenantTheme,
    realm_path: String,
    username: String,
    password: String,
    resume_body: Option<serde_json::Value>,
    extra: Option<String>,
    _scopes: Vec<String>,
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
) {
    use super::bootstrap::*;
    let http = match no_redirect_client() {
        Ok(c) => c,
        Err(e) => {
            send_onboard_error(&tx, onboard_id, format!("HTTP client init: {e}"));
            return;
        }
    };
    let auth_url = format!("{base_url}/am/json{realm_path}/authenticate");

    let mut body = match resume_body {
        Some(b) => b,
        None => {
            // AIC's load balancer (ALB) rejects POSTs with no
            // `Content-Length` header → HTTP 411. `curl -X POST` adds
            // `Content-Length: 0` automatically; reqwest+hyper does not,
            // even with `.body("")`. Send `{}` instead — AM ignores body
            // content on the first round, and we get a deterministic
            // `Content-Length: 2`.
            let resp = match http
                .post(&auth_url)
                .header("Accept-API-Version", "resource=2.0, protocol=1.0")
                .header("Content-Type", "application/json")
                .body("{}")
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    send_onboard_error(&tx, onboard_id, format!("authenticate: {e}"));
                    return;
                }
            };
            if !resp.status().is_success() {
                send_onboard_error(
                    &tx,
                    onboard_id,
                    format!("authenticate: HTTP {}", resp.status()),
                );
                return;
            }
            match resp.json::<serde_json::Value>().await {
                Ok(v) => v,
                Err(e) => {
                    send_onboard_error(&tx, onboard_id, format!("authenticate body: {e}"));
                    return;
                }
            }
        }
    };

    let mut current_extra = extra;
    for _round in 0..6 {
        if let Some(token_id) = body.get("tokenId").and_then(|v| v.as_str()) {
            let token_id = token_id.to_string();
            let cookie_name = match discover_cookie_name(&http, &base_url).await {
                Ok(n) => n,
                Err(e) => {
                    send_onboard_error(&tx, onboard_id, format!("serverinfo: {e}"));
                    return;
                }
            };
            let bearer = match session_to_bearer(&http, &base_url, &cookie_name, &token_id).await {
                Ok(bearer) => bearer,
                Err(e) => {
                    send_onboard_error(&tx, onboard_id, format!("authorize/token: {e}"));
                    return;
                }
            };
            let minted = match super::bootstrap::mint_log_key_from_bearer(
                &http,
                &base_url,
                &tenant_name,
                &bearer,
                Some(username.as_str()),
            )
            .await
            {
                Ok(minted) => minted,
                Err(e) => {
                    send_onboard_error(&tx, onboard_id, format!("log API key create: {e}"));
                    return;
                }
            };
            let kid = uuid::Uuid::new_v4().to_string();
            let priv_jwk = match generate_rsa_jwk(&kid) {
                Ok(j) => j,
                Err(e) => {
                    send_onboard_error(&tx, onboard_id, format!("RSA keygen: {e}"));
                    return;
                }
            };
            let pub_jwk = crate::aic::auth::public_jwk(&priv_jwk);
            let sa_id = match create_service_account(
                &http,
                &base_url,
                &bearer,
                &minted.credential_name,
                &format!("Created by aic-edit for {tenant_name}"),
                &pub_jwk,
            )
            .await
            {
                Ok(id) => id,
                Err(e) => {
                    send_onboard_error(&tx, onboard_id, format!("SA create: {e}"));
                    return;
                }
            };
            let log_key = Some(minted.key);
            let _ = tx.send(AppEvent::Onboard(Event::ServiceAccountReady {
                onboard_id,
                tenant_name,
                base_url,
                theme,
                sa_id,
                jwk: priv_jwk,
                log_key,
            }));
            return;
        }

        let outcome =
            super::userpass::walk_with_extra(&body, &username, &password, current_extra.as_deref());
        current_extra = None;
        match outcome {
            CallbackOutcome::Ready(filled) => {
                let resp = match http
                    .post(&auth_url)
                    .header("Accept-API-Version", "resource=2.0, protocol=1.0")
                    .header("Content-Type", "application/json")
                    .json(&filled)
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        send_onboard_error(&tx, onboard_id, format!("authenticate POST: {e}"));
                        return;
                    }
                };
                if !resp.status().is_success() {
                    let status = resp.status();
                    let txt = resp.text().await.unwrap_or_default();
                    send_onboard_error(
                        &tx,
                        onboard_id,
                        format!("authentication failed ({status}): {txt}"),
                    );
                    return;
                }
                body = match resp.json::<serde_json::Value>().await {
                    Ok(v) => v,
                    Err(e) => {
                        send_onboard_error(&tx, onboard_id, format!("authenticate body: {e}"));
                        return;
                    }
                };
            }
            CallbackOutcome::PromptRequired {
                prompt,
                body: pending,
            } => {
                let _ = tx.send(AppEvent::Onboard(Event::AuthProgress {
                    onboard_id,
                    body: pending,
                    prompt,
                }));
                return;
            }
            CallbackOutcome::Unsupported(msg) => {
                send_onboard_error(&tx, onboard_id, msg);
                return;
            }
        }
    }

    send_onboard_error(&tx, onboard_id, "too many authentication rounds — aborting");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_only_is_last_with_or_without_env_import() {
        assert_eq!(path_for_index(3, false), Some(OnboardPath::LogOnly));
        assert_eq!(path_for_index(4, false), None);

        assert_eq!(path_for_index(3, true), Some(OnboardPath::Envrc));
        assert_eq!(path_for_index(4, true), Some(OnboardPath::LogOnly));
        assert_eq!(path_for_index(5, true), None);
    }

    #[test]
    fn tenant_name_exists_matches_exact_existing_name() {
        let tenants = vec![Tenant {
            name: "sandbox".into(),
            base_url: "https://example.forgeblocks.com".into(),
            theme: TenantTheme::Sandbox,
            sa_id: Some("service-account-id".into()),
            scopes: vec!["fr:am:*".into()],
        }];

        assert!(tenant_name_exists(&tenants, "sandbox"));
        assert!(!tenant_name_exists(&tenants, "Sandbox"));
        assert!(!tenant_name_exists(&tenants, "development"));
    }
}
