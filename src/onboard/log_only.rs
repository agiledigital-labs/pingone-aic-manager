//! Provision a logs-only tenant from an existing admin browser session.
//! The session is exchanged for an admin-user bearer that can create the log
//! API key; no service account or RSA keypair is created.

use crate::config::tenant::TenantTheme;
use crate::tui::widgets::text_field::{TextField, fields};

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
