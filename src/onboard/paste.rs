//! Pattern 3 — paste an existing service-account JWK + UUID directly.
//! The user already minted an SA elsewhere (via the AIC console or another tool)
//! and just wants pingone-aic-manager to use it.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::event::ToastKind;
use crate::app::prod_confirm::PendingProdAction;
use crate::app::{App, InputMode};
use crate::config::tenant::{Tenant, TenantTheme};
use crate::tui::is_save_chord;
use crate::tui::widgets::text_field::{TextField, fields};

use super::common::{persist_tenant_overwriting, queue_overwrite_confirm, tenant_name_exists};
use super::screen::{Mode, PendingConfirm, ProdAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteField {
    Name,
    Domain,
    Theme,
    SaId,
    Jwk,
    Submit,
}

impl PasteField {
    pub const ORDER: [PasteField; 6] = [
        PasteField::Name,
        PasteField::Domain,
        PasteField::Theme,
        PasteField::SaId,
        PasteField::Jwk,
        PasteField::Submit,
    ];

    pub fn next(self) -> Self {
        let i = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        Self::ORDER[(i + 1) % Self::ORDER.len()]
    }

    pub fn prev(self) -> Self {
        let i = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        Self::ORDER[(i + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }
}

#[derive(Debug, Clone)]
pub struct PasteForm {
    pub name: TextField,
    pub domain: TextField,
    pub theme: TenantTheme,
    pub theme_idx: usize,
    pub sa_id: TextField,
    pub jwk_input: TextField,
    pub focused: PasteField,
    pub error: Option<String>,
}

impl Default for PasteForm {
    fn default() -> Self {
        Self {
            name: fields::tenant_name(),
            domain: fields::hostname(),
            theme: TenantTheme::Sandbox,
            theme_idx: 0,
            sa_id: fields::sa_uuid(),
            jwk_input: fields::jwk(),
            focused: PasteField::Name,
            error: None,
        }
    }
}

impl PasteForm {
    pub fn focused_field_mut(&mut self) -> Option<&mut TextField> {
        match self.focused {
            PasteField::Name => Some(&mut self.name),
            PasteField::Domain => Some(&mut self.domain),
            PasteField::SaId => Some(&mut self.sa_id),
            PasteField::Jwk => Some(&mut self.jwk_input),
            PasteField::Theme | PasteField::Submit => None,
        }
    }

    pub fn is_jwk_field(&self) -> bool {
        self.focused == PasteField::Jwk
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

    pub fn validate_jwk(&self) -> std::result::Result<serde_json::Value, String> {
        let v: serde_json::Value = serde_json::from_str(self.jwk_input.trimmed())
            .map_err(|e| format!("Invalid JSON: {e}"))?;
        for field in &["kty", "n", "e", "d"] {
            if v[field].is_null() {
                return Err(format!("JWK missing '{field}' field"));
            }
        }
        Ok(v)
    }

    pub fn validate(&self) -> std::result::Result<serde_json::Value, String> {
        if self.name.is_empty() {
            return Err("Tenant name is required".into());
        }
        super::validate_domain(&self.domain.value)?;
        if self.sa_id.is_empty() {
            return Err("Service account ID is required".into());
        }
        self.validate_jwk()
    }

    pub fn normalised_base_url(&self) -> String {
        super::domain_to_base_url(&self.domain.value)
    }

    pub fn into_tenant(&self) -> Tenant {
        // Derive from SA_SCOPES like every other onboarding path. A hand-copied
        // list here would silently keep paste-onboarded tenants on the old set
        // when SA_SCOPES widens, surfacing much later as a 403.
        let scopes: Vec<String> = super::bootstrap::SA_SCOPES
            .iter()
            .map(|scope| (*scope).to_string())
            .collect();
        Tenant {
            name: self.name.trimmed().to_string(),
            base_url: self.normalised_base_url(),
            theme: self.theme,
            sa_id: Some(self.sa_id.trimmed().to_string()),
            scopes,
        }
    }
}

// ---- Key handling ----

pub async fn handle_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let form = match &mut app.onboard.paste_form {
        Some(f) => f,
        None => return Ok(()),
    };

    let leaving_domain = (matches!(key.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter)
        || is_save_chord(&key))
        && form.focused == PasteField::Domain;
    if leaving_domain {
        let cleaned = super::normalise_domain(&form.domain.value);
        form.domain.set(cleaned);
    }

    // `Ctrl-S` submits from any field. Handled as an arm rather than an early
    // return so the domain normalisation above still runs first — and it must
    // stay ahead of the plain-`Enter` arms to win.
    let submitting =
        is_save_chord(&key) || (key.code == KeyCode::Enter && form.focused == PasteField::Submit);
    match key.code {
        KeyCode::Esc => {
            app.onboard.paste_form = None;
            app.input_mode = InputMode::Onboard(Mode::Menu);
        }
        KeyCode::Tab => form.focused = form.focused.next(),
        KeyCode::BackTab => form.focused = form.focused.prev(),
        KeyCode::Left if form.focused == PasteField::Theme => form.cycle_theme_backward(),
        KeyCode::Right if form.focused == PasteField::Theme => form.cycle_theme_forward(),
        _ if submitting => {
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
                persist(app, tenant, jwk);
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

/// Persist a pasted service-account tenant, routing production themes through
/// the shared prod-write guard. Public so the overwrite-confirm handler can
/// resume it after the user confirms replacing a tenant.
pub(crate) fn persist(app: &mut App, tenant: Tenant, jwk: serde_json::Value) {
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
