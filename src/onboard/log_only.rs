//! Provision a logs-only tenant from an existing admin browser session.
//! The session is exchanged for an admin-user bearer that can create the log
//! API key; no service account or RSA keypair is created.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::event::{AppEvent, ToastKind};
use crate::app::prod_confirm::PendingProdAction;
use crate::app::{App, InputMode};
use crate::config::tenant::{Tenant, TenantTheme};
use crate::logs::LogKeyPair;
use crate::tui::is_save_chord;
use crate::tui::widgets::text_field::{TextField, fields};

use super::common::{
    persist_tenant_overwriting, queue_overwrite_confirm, send_onboard_error, tenant_name_exists,
};
use super::screen::{Event, Mode, PendingConfirm, ProdAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogOnlyField {
    Name,
    Domain,
    Theme,
    CookieName,
    CookieValue,
    Submit,
}

impl LogOnlyField {
    pub const ORDER: [LogOnlyField; 6] = [
        LogOnlyField::Name,
        LogOnlyField::Domain,
        LogOnlyField::Theme,
        LogOnlyField::CookieName,
        LogOnlyField::CookieValue,
        LogOnlyField::Submit,
    ];

    pub fn next(self) -> Self {
        let idx = Self::ORDER
            .iter()
            .position(|field| *field == self)
            .unwrap_or(0);
        Self::ORDER[(idx + 1) % Self::ORDER.len()]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ORDER
            .iter()
            .position(|field| *field == self)
            .unwrap_or(0);
        Self::ORDER[(idx + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogOnlyIntent {
    pub tenant_name: String,
    pub base_url: String,
    pub theme: TenantTheme,
    pub cookie_name: String,
    pub cookie_value: String,
}

#[derive(Debug, Clone)]
pub struct LogOnlyForm {
    pub name: TextField,
    pub domain: TextField,
    pub theme: TenantTheme,
    pub theme_idx: usize,
    pub cookie_name: TextField,
    pub cookie_value: TextField,
    pub focused: LogOnlyField,
    pub error: Option<String>,
    pub busy: bool,
    pub status: Option<String>,
}

impl Default for LogOnlyForm {
    fn default() -> Self {
        Self {
            name: fields::tenant_name(),
            domain: fields::hostname(),
            theme: TenantTheme::Sandbox,
            theme_idx: 0,
            cookie_name: fields::cookie_name(),
            cookie_value: fields::cookie_value(),
            focused: LogOnlyField::Name,
            error: None,
            busy: false,
            status: None,
        }
    }
}

impl LogOnlyForm {
    pub fn focused_field_mut(&mut self) -> Option<&mut TextField> {
        match self.focused {
            LogOnlyField::Name => Some(&mut self.name),
            LogOnlyField::Domain => Some(&mut self.domain),
            LogOnlyField::CookieName => Some(&mut self.cookie_name),
            LogOnlyField::CookieValue => Some(&mut self.cookie_value),
            LogOnlyField::Theme | LogOnlyField::Submit => None,
        }
    }

    pub fn cycle_theme_forward(&mut self) {
        let themes = TenantTheme::all();
        self.theme_idx = (self.theme_idx + 1) % themes.len();
        self.theme = themes[self.theme_idx];
    }

    pub fn cycle_theme_backward(&mut self) {
        let themes = TenantTheme::all();
        self.theme_idx = (self.theme_idx + themes.len() - 1) % themes.len();
        self.theme = themes[self.theme_idx];
    }

    pub fn validate(&self) -> std::result::Result<LogOnlyIntent, String> {
        if self.name.is_empty() {
            return Err("Tenant name is required".into());
        }
        let domain = super::validate_domain(&self.domain.value)?;
        if self.cookie_name.is_empty() {
            return Err("Cookie name is required (the random-hex cookie from DevTools)".into());
        }
        if self.cookie_value.is_empty() {
            return Err("Cookie value is required".into());
        }

        Ok(LogOnlyIntent {
            tenant_name: self.name.trimmed().to_string(),
            base_url: super::domain_to_base_url(&domain),
            theme: self.theme,
            cookie_name: self.cookie_name.trimmed().to_string(),
            cookie_value: self.cookie_value.trimmed().to_string(),
        })
    }
}

// ---- Key handling ----

pub async fn handle_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
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

    let leaving_domain = (matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter)
        || is_save_chord(&key))
        && form.focused == LogOnlyField::Domain;
    if leaving_domain {
        let cleaned = super::normalise_domain(&form.domain.value);
        form.domain.set(cleaned);
    }

    // `Ctrl-S` submits from any field. Handled as an arm rather than an early
    // return so the domain normalisation above still runs first — and it must
    // stay ahead of the plain-`Enter` arm to win.
    let submitting =
        is_save_chord(&key) || (key.code == KeyCode::Enter && form.focused == LogOnlyField::Submit);
    match key.code {
        KeyCode::Esc => {
            app.onboard.log_only_form = None;
            app.input_mode = InputMode::Onboard(Mode::Menu);
        }
        KeyCode::Tab => form.focused = form.focused.next(),
        KeyCode::BackTab => form.focused = form.focused.prev(),
        KeyCode::Left if form.focused == LogOnlyField::Theme => form.cycle_theme_backward(),
        KeyCode::Right if form.focused == LogOnlyField::Theme => form.cycle_theme_forward(),
        _ if submitting => match form.validate() {
            Ok(intent) => {
                form.error = None;
                if tenant_name_exists(&app.tenants, &intent.tenant_name) {
                    queue_overwrite_confirm(app, PendingConfirm::LogOnly);
                } else {
                    start_bootstrap(app);
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

/// Kick off the log-only bootstrap. Public so the overwrite-confirm handler
/// can resume it after the user confirms replacing a tenant.
pub(crate) fn start_bootstrap(app: &mut App) {
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
        run_bootstrap(onboard_id, intent, tx).await;
    });
}

// ---- Completion ----

pub fn handle_created(
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

// ---- Background bootstrap ----

async fn run_bootstrap(
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
        Ok(minted) => {
            if let Some(username) = minted.admin_username.as_deref()
                && let Err(error) = crate::config::operator::set_name_if_unset(username)
            {
                tracing::warn!(%error, "could not persist operator name during onboarding");
            }
            minted.key
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_builds_normalised_log_only_intent() {
        let mut form = LogOnlyForm::default();
        form.name.set(" logs ");
        form.domain.set("https://example.forgeblocks.com/am/");
        form.cookie_name.set(" iPlanetDirectoryPro ");
        form.cookie_value.set(" session-value ");

        let intent = form.validate().unwrap();

        assert_eq!(intent.tenant_name, "logs");
        assert_eq!(intent.base_url, "https://example.forgeblocks.com");
        assert_eq!(intent.cookie_name, "iPlanetDirectoryPro");
        assert_eq!(intent.cookie_value, "session-value");
        assert_eq!(intent.theme, TenantTheme::Sandbox);
    }

    #[test]
    fn validate_requires_cookie_credentials() {
        let mut form = LogOnlyForm::default();
        form.name.set("logs");
        form.domain.set("example.forgeblocks.com");

        assert_eq!(
            form.validate().unwrap_err(),
            "Cookie name is required (the random-hex cookie from DevTools)"
        );

        form.cookie_name.set("session");
        assert_eq!(form.validate().unwrap_err(), "Cookie value is required");
    }
}
