//! Pattern 3 — paste an existing service-account JWK + UUID directly.
//! The user already minted an SA elsewhere (via the AIC console or another tool)
//! and just wants aic-edit to use it.

use crate::config::tenant::{Tenant, TenantTheme};
use crate::tui::widgets::text_field::{TextField, fields};

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
        let scopes = vec![
            "fr:idm:*".into(),
            "fr:am:*".into(),
            "fr:idc:esv:*".into(),
            "fr:idc:cookie-domain:*".into(),
        ];
        Tenant {
            name: self.name.trimmed().to_string(),
            base_url: self.normalised_base_url(),
            theme: self.theme,
            sa_id: self.sa_id.trimmed().to_string(),
            scopes,
        }
    }
}
