//! Tenant onboarding screens: cookie / userpass / paste / sandbox-import.
//! Owns the four form structs (well, three — paste is synchronous), the
//! pending-action book-keeping (prod confirm, overwrite confirm, in-flight
//! bootstrap id, OTP callback body), and the background bootstrap tasks
//! that hit AIC pre-tenant.

use crossterm::event::{KeyCode, KeyEvent};

use crate::aic::onboard::cookie::{CookieField, CookieForm};
use crate::aic::onboard::paste::{PasteField, PasteForm};
use crate::aic::onboard::userpass::{CallbackOutcome, UpField, UpForm};
use crate::app::{App, InputMode};
use crate::config::tenant::{Tenant, TenantTheme};
use crate::config::ProjectConfig;
use crate::event::{AppEvent, ToastKind};

/// Pending write that requires the prod-confirm overlay first.
#[derive(Debug)]
pub enum PendingProdAction {
    SaveTenant {
        tenant: Tenant,
        jwk: serde_json::Value,
    },
}

#[derive(Debug, Default)]
pub struct State {
    pub menu_idx: usize,
    pub cookie_form: Option<CookieForm>,
    pub up_form: Option<UpForm>,
    pub paste_form: Option<PasteForm>,

    /// UUID stamped on the in-flight bootstrap task. Set when the user
    /// kicks off Pattern 1/2 (cookie / userpass), cleared on Esc-cancel.
    /// When a `ServiceAccountCreated` event arrives with a non-matching
    /// id, the handler drops it instead of persisting a tenant the user
    /// no longer wants.
    pub pending_id: Option<uuid::Uuid>,

    /// Pattern 2: the in-flight callback JSON we POST'd that needs an
    /// extra prompt (TOTP).
    pub pending_callback_body: Option<serde_json::Value>,

    /// Pending write awaiting the prod-confirm overlay.
    pub pending_prod_action: Option<PendingProdAction>,

    /// Pending tenant whose name collides with an existing one. Set when
    /// the overwrite-confirm modal is up.
    pub pending_overwrite: Option<(Tenant, serde_json::Value)>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pending_overwrite_name(&self) -> Option<&str> {
        self.pending_overwrite.as_ref().map(|(t, _)| t.name.as_str())
    }
}

pub async fn handle_menu_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let max_idx = if app.has_env_creds { 3 } else { 2 };
    match key.code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if app.onboard.menu_idx < max_idx {
                app.onboard.menu_idx += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.onboard.menu_idx > 0 {
                app.onboard.menu_idx -= 1;
            }
        }
        KeyCode::Enter => enter_choice(app, app.onboard.menu_idx).await?,
        KeyCode::Char('1') => enter_choice(app, 0).await?,
        KeyCode::Char('2') => enter_choice(app, 1).await?,
        KeyCode::Char('3') => enter_choice(app, 2).await?,
        KeyCode::Char('4') if app.has_env_creds => enter_choice(app, 3).await?,
        _ => {}
    }
    Ok(())
}

async fn enter_choice(app: &mut App, idx: usize) -> crate::Result<()> {
    match idx {
        0 => {
            app.onboard.cookie_form = Some(CookieForm::default());
            app.input_mode = InputMode::OnboardCookie;
        }
        1 => {
            app.onboard.up_form = Some(UpForm::default());
            app.input_mode = InputMode::OnboardUserPass;
        }
        2 => {
            app.onboard.paste_form = Some(PasteForm::default());
            app.input_mode = InputMode::OnboardPaste;
        }
        3 if app.has_env_creds => {
            import_env_creds(app).await?;
        }
        _ => {}
    }
    Ok(())
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
            // ServiceAccountCreated event (if it still arrives) is
            // recognised as stale and ignored.
            app.onboard.pending_id = None;
            app.input_mode = InputMode::OnboardMenu;
        }
        return Ok(());
    }

    // Normalise the domain field whenever focus leaves it.
    let leaving_domain = matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter)
        && form.focused == CookieField::Domain;
    if leaving_domain {
        let cleaned = crate::aic::onboard::normalise_domain(&form.domain.value);
        form.domain.set(cleaned);
    }

    match key.code {
        KeyCode::Esc => {
            app.onboard.cookie_form = None;
            app.input_mode = InputMode::OnboardMenu;
        }
        KeyCode::Tab => form.focused = form.focused.next(),
        KeyCode::BackTab => form.focused = form.focused.prev(),
        KeyCode::Left if form.focused == CookieField::Theme => form.cycle_theme_backward(),
        KeyCode::Right if form.focused == CookieField::Theme => form.cycle_theme_forward(),
        KeyCode::Enter if form.focused == CookieField::Submit => {
            if let Err(e) = form.validate() {
                form.error = Some(e);
            } else {
                form.error = None;
                start_cookie_bootstrap(app);
            }
        }
        KeyCode::Enter => form.focused = form.focused.next(),
        KeyCode::Backspace => {
            if let Some(f) = form.focused_field_mut() {
                f.backspace();
            }
        }
        KeyCode::Char(c) => {
            if let Some(f) = form.focused_field_mut() {
                f.push_char(c);
            }
        }
        _ => {}
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
        run_bootstrap_from_cookie(onboard_id, name, base_url, theme, cookie_name, cookie_value, tx)
            .await;
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
            KeyCode::Enter => {
                if !form.prompt_input.is_empty() {
                    let extra = form.prompt_input.clone();
                    form.prompt_input.clear();
                    form.pending_prompt = None;
                    form.status = Some("Continuing authentication…".into());
                    continue_up_with_extra(app, extra);
                }
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
            app.input_mode = InputMode::OnboardMenu;
        }
        return Ok(());
    }

    let leaving_domain = matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter)
        && form.focused == UpField::Domain;
    if leaving_domain {
        let cleaned = crate::aic::onboard::normalise_domain(&form.domain.value);
        form.domain.set(cleaned);
    }

    match key.code {
        KeyCode::Esc => {
            app.onboard.up_form = None;
            app.input_mode = InputMode::OnboardMenu;
        }
        KeyCode::Tab => form.focused = form.focused.next(),
        KeyCode::BackTab => form.focused = form.focused.prev(),
        KeyCode::Left if form.focused == UpField::Theme => form.cycle_theme_backward(),
        KeyCode::Right if form.focused == UpField::Theme => form.cycle_theme_forward(),
        KeyCode::Enter if form.focused == UpField::Submit => {
            if let Err(e) = form.validate() {
                form.error = Some(e);
            } else {
                form.error = None;
                start_up_bootstrap(app);
            }
        }
        KeyCode::Enter => form.focused = form.focused.next(),
        KeyCode::Backspace => {
            if let Some(f) = form.focused_field_mut() {
                f.backspace();
            }
        }
        KeyCode::Char(c) => {
            if let Some(f) = form.focused_field_mut() {
                f.push_char(c);
            }
        }
        _ => {}
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
    let scopes: Vec<String> = crate::aic::onboard::bootstrap::SA_SCOPES
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
    let scopes: Vec<String> = crate::aic::onboard::bootstrap::SA_SCOPES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let tx = app.events.tx.clone();
    // Re-use the existing onboard id — this is a continuation of the same
    // user-initiated bootstrap. If the user cancelled and the id is gone,
    // there's nothing to continue.
    let Some(onboard_id) = app.onboard.pending_id else { return };
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

pub fn handle_auth_progress(app: &mut App, body: serde_json::Value, prompt: String) {
    if let Some(form) = &mut app.onboard.up_form {
        form.pending_prompt = Some(prompt);
        form.status = None;
    }
    app.onboard.pending_callback_body = Some(body);
}

pub fn handle_onboard_error(app: &mut App, msg: String) {
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
        let cleaned = crate::aic::onboard::normalise_domain(&form.domain.value);
        form.domain.set(cleaned);
    }

    match key.code {
        KeyCode::Esc => {
            app.onboard.paste_form = None;
            app.input_mode = InputMode::OnboardMenu;
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
            let prod = tenant.theme == TenantTheme::Production;
            app.onboard.paste_form = None;
            if prod {
                app.onboard.pending_prod_action = Some(PendingProdAction::SaveTenant { tenant, jwk });
                app.input_mode = InputMode::ProdConfirm;
            } else {
                match persist_new_tenant(app, tenant, jwk) {
                    Ok(()) => app.push_toast(ToastKind::Success, "Tenant added!"),
                    Err(e) => app.push_toast(ToastKind::Error, format!("Save failed: {e}")),
                }
                app.input_mode = InputMode::Normal;
            }
        }
        KeyCode::Enter if form.is_jwk_field() => {
            form.jwk_input.push_newline();
        }
        KeyCode::Enter => form.focused = form.focused.next(),
        KeyCode::Backspace => {
            if let Some(f) = form.focused_field_mut() {
                f.backspace();
            }
        }
        KeyCode::Char(c) => {
            if let Some(f) = form.focused_field_mut() {
                f.push_char(c);
            }
        }
        _ => {}
    }
    Ok(())
}

// ---- Overwrite / prod confirm / SA-created ----

pub fn handle_overwrite_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Some((tenant, jwk)) = app.onboard.pending_overwrite.take() {
                app.input_mode = InputMode::Normal;
                match persist_tenant_overwriting(app, tenant, jwk) {
                    Ok(()) => app.push_toast(ToastKind::Success, "Tenant overwritten"),
                    Err(e) => {
                        app.push_toast(ToastKind::Error, format!("Save failed: {e}"));
                    }
                }
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.onboard.pending_overwrite = None;
            app.input_mode = InputMode::Normal;
            app.push_toast(ToastKind::Info, "Overwrite cancelled");
        }
        _ => {}
    }
    Ok(())
}

pub async fn handle_prod_confirm_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let action = app.onboard.pending_prod_action.take();
            app.input_mode = InputMode::Normal;
            if let Some(action) = action {
                match action {
                    PendingProdAction::SaveTenant { tenant, jwk } => {
                        match persist_new_tenant(app, tenant, jwk) {
                            Ok(()) => app.push_toast(ToastKind::Success, "Tenant added!"),
                            Err(e) => {
                                app.push_toast(ToastKind::Error, format!("Save failed: {e}"))
                            }
                        }
                    }
                }
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.onboard.pending_prod_action = None;
            app.input_mode = InputMode::Normal;
            app.push_toast(ToastKind::Info, "Prod write cancelled");
        }
        _ => {}
    }
    Ok(())
}

pub fn handle_sa_created(
    app: &mut App,
    onboard_id: uuid::Uuid,
    tenant_name: String,
    base_url: String,
    theme: TenantTheme,
    sa_id: String,
    jwk: serde_json::Value,
) -> crate::Result<()> {
    // Drop the event if it doesn't match the bootstrap we're waiting on.
    if app.onboard.pending_id != Some(onboard_id) {
        tracing::debug!(
            event_id = %onboard_id,
            pending = ?app.onboard.pending_id,
            "dropping stale ServiceAccountCreated"
        );
        return Ok(());
    }
    app.onboard.pending_id = None;

    let scopes: Vec<String> = crate::aic::onboard::bootstrap::SA_SCOPES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let tenant = Tenant {
        name: tenant_name,
        base_url,
        theme,
        sa_id,
        scopes,
    };

    // Clear in-flight forms.
    app.onboard.cookie_form = None;
    app.onboard.up_form = None;
    app.onboard.pending_callback_body = None;

    if tenant.theme == TenantTheme::Production {
        app.onboard.pending_prod_action = Some(PendingProdAction::SaveTenant { tenant, jwk });
        app.input_mode = InputMode::ProdConfirm;
        return Ok(());
    }

    match persist_new_tenant(app, tenant, jwk) {
        Ok(()) => {
            app.push_toast(ToastKind::Success, "Tenant added!");
            app.input_mode = InputMode::Normal;
        }
        Err(e) => {
            app.push_toast(ToastKind::Error, format!("Save failed: {e}"));
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

    let scopes: Vec<String> = crate::aic::onboard::bootstrap::SA_SCOPES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let tenant = Tenant {
        name: "sandbox".into(),
        base_url,
        theme: TenantTheme::Sandbox,
        sa_id,
        scopes,
    };

    match persist_new_tenant(app, tenant, jwk) {
        Ok(()) => {
            app.push_toast(ToastKind::Success, "Imported sandbox tenant from environment");
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

/// Persist a new tenant. If a tenant with the same name already exists,
/// switch to the OverwriteConfirm modal and bail out — the caller's flow
/// is paused until the user answers.
fn persist_new_tenant(
    app: &mut App,
    tenant: Tenant,
    jwk: serde_json::Value,
) -> crate::Result<()> {
    if app.tenants.iter().any(|t| t.name == tenant.name) {
        app.onboard.pending_overwrite = Some((tenant, jwk));
        app.input_mode = InputMode::OverwriteConfirm;
        return Ok(());
    }
    persist_tenant_overwriting(app, tenant, jwk)
}

/// Save a tenant outright — replacing any existing entry with the same
/// name. Caller is responsible for confirming the overwrite before calling.
fn persist_tenant_overwriting(
    app: &mut App,
    tenant: Tenant,
    jwk: serde_json::Value,
) -> crate::Result<()> {
    app.save_jwk(&tenant.name, jwk)?;

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
    Ok(())
}

// ---- Background bootstrap tasks ----

async fn run_bootstrap_from_cookie(
    onboard_id: uuid::Uuid,
    tenant_name: String,
    base_url: String,
    theme: TenantTheme,
    cookie_name: String,
    session_value: String,
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
) {
    use crate::aic::onboard::bootstrap::*;
    let http = match no_redirect_client() {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(AppEvent::OnboardError(format!("HTTP client init: {e}")));
            return;
        }
    };
    let bearer = match session_to_bearer(&http, &base_url, &cookie_name, &session_value).await {
        Ok(b) => b,
        Err(e) => {
            let _ = tx.send(AppEvent::OnboardError(format!("authorize/token: {e}")));
            return;
        }
    };
    let kid = uuid::Uuid::new_v4().to_string();
    let priv_jwk = match generate_rsa_jwk(&kid) {
        Ok(j) => j,
        Err(e) => {
            let _ = tx.send(AppEvent::OnboardError(format!("RSA keygen: {e}")));
            return;
        }
    };
    let pub_jwk = crate::aic::auth::public_jwk(&priv_jwk);
    let sa_name = format!("aic-edit-{tenant_name}");
    let sa_id = match create_service_account(
        &http,
        &base_url,
        &bearer,
        &sa_name,
        &format!("Created by aic-edit for {tenant_name}"),
        &pub_jwk,
    )
    .await
    {
        Ok(id) => id,
        Err(e) => {
            let _ = tx.send(AppEvent::OnboardError(format!("SA create: {e}")));
            return;
        }
    };
    let _ = tx.send(AppEvent::ServiceAccountCreated {
        onboard_id,
        tenant_name,
        base_url,
        theme,
        sa_id,
        jwk: priv_jwk,
    });
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
    use crate::aic::onboard::bootstrap::*;
    let http = match no_redirect_client() {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(AppEvent::OnboardError(format!("HTTP client init: {e}")));
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
                    let _ = tx.send(AppEvent::OnboardError(format!("authenticate: {e}")));
                    return;
                }
            };
            if !resp.status().is_success() {
                let _ = tx.send(AppEvent::OnboardError(format!(
                    "authenticate: HTTP {}",
                    resp.status()
                )));
                return;
            }
            match resp.json::<serde_json::Value>().await {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx.send(AppEvent::OnboardError(format!("authenticate body: {e}")));
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
                    let _ = tx.send(AppEvent::OnboardError(format!("serverinfo: {e}")));
                    return;
                }
            };
            let bearer = match session_to_bearer(&http, &base_url, &cookie_name, &token_id).await {
                Ok(b) => b,
                Err(e) => {
                    let _ = tx.send(AppEvent::OnboardError(format!("authorize/token: {e}")));
                    return;
                }
            };
            let kid = uuid::Uuid::new_v4().to_string();
            let priv_jwk = match generate_rsa_jwk(&kid) {
                Ok(j) => j,
                Err(e) => {
                    let _ = tx.send(AppEvent::OnboardError(format!("RSA keygen: {e}")));
                    return;
                }
            };
            let pub_jwk = crate::aic::auth::public_jwk(&priv_jwk);
            let sa_name = format!("aic-edit-{tenant_name}");
            let sa_id = match create_service_account(
                &http,
                &base_url,
                &bearer,
                &sa_name,
                &format!("Created by aic-edit for {tenant_name}"),
                &pub_jwk,
            )
            .await
            {
                Ok(id) => id,
                Err(e) => {
                    let _ = tx.send(AppEvent::OnboardError(format!("SA create: {e}")));
                    return;
                }
            };
            let _ = tx.send(AppEvent::ServiceAccountCreated {
                onboard_id,
                tenant_name,
                base_url,
                theme,
                sa_id,
                jwk: priv_jwk,
            });
            return;
        }

        let outcome = crate::aic::onboard::userpass::walk_with_extra(
            &body,
            &username,
            &password,
            current_extra.as_deref(),
        );
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
                        let _ = tx.send(AppEvent::OnboardError(format!("authenticate POST: {e}")));
                        return;
                    }
                };
                if !resp.status().is_success() {
                    let status = resp.status();
                    let txt = resp.text().await.unwrap_or_default();
                    let _ = tx.send(AppEvent::OnboardError(format!(
                        "authentication failed ({status}): {txt}"
                    )));
                    return;
                }
                body = match resp.json::<serde_json::Value>().await {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = tx.send(AppEvent::OnboardError(format!("authenticate body: {e}")));
                        return;
                    }
                };
            }
            CallbackOutcome::PromptRequired {
                prompt,
                body: pending,
            } => {
                let _ = tx.send(AppEvent::AuthCallbackProgress {
                    body: pending,
                    prompt,
                });
                return;
            }
            CallbackOutcome::Unsupported(msg) => {
                let _ = tx.send(AppEvent::OnboardError(msg));
                return;
            }
        }
    }

    let _ = tx.send(AppEvent::OnboardError(
        "too many authentication rounds — aborting".into(),
    ));
}
