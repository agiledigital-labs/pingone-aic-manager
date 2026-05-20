//! Pattern 1 — paste session cookie.
//! The user pastes the AM session cookie name + value from their logged-in
//! browser tab; we drive the OAuth2 flow server-side and create a service
//! account.

use crate::config::tenant::TenantTheme;
use crate::ui::widgets::text_field::{fields, TextField};

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
