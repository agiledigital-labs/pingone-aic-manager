//! Pattern 1 — paste session cookie.
//! The user pastes the AM session cookie name + value from their logged-in
//! browser tab; we drive the OAuth2 flow server-side and create a service
//! account.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::event::AppEvent;
use crate::app::{App, InputMode};
use crate::config::tenant::TenantTheme;
use crate::tui::is_save_chord;
use crate::tui::widgets::text_field::{TextField, fields};

use super::common::{queue_overwrite_confirm, send_onboard_error, tenant_name_exists};
use super::screen::{Event, Mode, PendingConfirm};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookieField {
    Name,
    Domain,
    Theme,
    CookieName,
    Cookie,
    Submit,
}

impl CookieField {
    pub const ORDER: [CookieField; 6] = [
        CookieField::Name,
        CookieField::Domain,
        CookieField::Theme,
        CookieField::CookieName,
        CookieField::Cookie,
        CookieField::Submit,
    ];

    pub fn next(self) -> Self {
        let idx = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        Self::ORDER[(idx + 1) % Self::ORDER.len()]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        Self::ORDER[(idx + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }
}

#[derive(Debug, Clone)]
pub struct CookieForm {
    pub name: TextField,
    pub domain: TextField,
    pub theme: TenantTheme,
    pub theme_idx: usize,
    pub cookie_name: TextField,
    pub cookie_value: TextField,
    pub focused: CookieField,
    pub error: Option<String>,
    pub busy: bool,
    pub status: Option<String>,
}

impl Default for CookieForm {
    fn default() -> Self {
        Self {
            name: fields::tenant_name(),
            domain: fields::hostname(),
            theme: TenantTheme::Sandbox,
            theme_idx: 0,
            cookie_name: fields::cookie_name(),
            cookie_value: fields::cookie_value(),
            focused: CookieField::Name,
            error: None,
            busy: false,
            status: None,
        }
    }
}

impl CookieForm {
    pub fn focused_field_mut(&mut self) -> Option<&mut TextField> {
        match self.focused {
            CookieField::Name => Some(&mut self.name),
            CookieField::Domain => Some(&mut self.domain),
            CookieField::CookieName => Some(&mut self.cookie_name),
            CookieField::Cookie => Some(&mut self.cookie_value),
            CookieField::Theme | CookieField::Submit => None,
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

    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.name.is_empty() {
            return Err("Tenant name is required".into());
        }
        super::validate_domain(&self.domain.value)?;
        if self.cookie_name.is_empty() {
            return Err("Cookie name is required (the random-hex cookie from DevTools)".into());
        }
        if self.cookie_value.is_empty() {
            return Err("Cookie value is required".into());
        }
        Ok(())
    }

    pub fn normalised_base_url(&self) -> String {
        super::domain_to_base_url(&self.domain.value)
    }
}

// ---- Key handling ----

pub async fn handle_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
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
    let leaving_domain = (matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter)
        || is_save_chord(&key))
        && form.focused == CookieField::Domain;
    if leaving_domain {
        let cleaned = super::normalise_domain(&form.domain.value);
        form.domain.set(cleaned);
    }

    // `Ctrl-S` submits from any field. Handled as an arm rather than an early
    // return so the domain normalisation above still runs first — and it must
    // stay ahead of the plain-`Enter` arm to win.
    let submitting =
        is_save_chord(&key) || (key.code == KeyCode::Enter && form.focused == CookieField::Submit);
    match key.code {
        KeyCode::Esc => {
            app.onboard.cookie_form = None;
            app.input_mode = InputMode::Onboard(Mode::Menu);
        }
        KeyCode::Tab => form.focused = form.focused.next(),
        KeyCode::BackTab => form.focused = form.focused.prev(),
        KeyCode::Left if form.focused == CookieField::Theme => form.cycle_theme_backward(),
        KeyCode::Right if form.focused == CookieField::Theme => form.cycle_theme_forward(),
        _ if submitting => {
            if let Err(e) = form.validate() {
                form.error = Some(e);
            } else {
                let name = form.name.trimmed().to_string();
                form.error = None;
                if tenant_name_exists(&app.tenants, &name) {
                    queue_overwrite_confirm(app, PendingConfirm::Cookie);
                } else {
                    start_bootstrap(app);
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

/// Kick off the cookie bootstrap. Public so the overwrite-confirm handler can
/// resume it after the user confirms replacing an existing tenant.
pub(crate) fn start_bootstrap(app: &mut App) {
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
        run_bootstrap(
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

// ---- Background bootstrap ----

async fn run_bootstrap(
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
    let minted = match mint_log_key_from_bearer(&http, &base_url, &tenant_name, &bearer, None).await
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
        &format!("Created by pingone-aic-manager for {tenant_name}"),
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
