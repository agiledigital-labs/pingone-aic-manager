use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TenantTheme {
    Sandbox,
    Development,
    Staging,
    Production,
}

impl TenantTheme {
    pub fn label(self) -> &'static str {
        match self {
            TenantTheme::Sandbox => "sandbox",
            TenantTheme::Development => "development",
            TenantTheme::Staging => "staging",
            TenantTheme::Production => "production",
        }
    }

    /// Static content (e.g. ESV secret mappings) is only editable in the lower
    /// environments; staging/production receive it via promotion, not direct edits.
    pub fn allows_static_content(self) -> bool {
        matches!(self, TenantTheme::Sandbox | TenantTheme::Development)
    }

    pub fn all() -> &'static [TenantTheme] {
        &[
            TenantTheme::Sandbox,
            TenantTheme::Development,
            TenantTheme::Staging,
            TenantTheme::Production,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    pub name: String,
    pub base_url: String,
    pub theme: TenantTheme,
    pub sa_id: String,
    pub scopes: Vec<String>,
}

impl Tenant {
    pub fn is_prod(&self) -> bool {
        self.theme == TenantTheme::Production
    }

    pub fn allows_secret_mappings(&self) -> bool {
        self.theme.allows_static_content()
    }
}

#[cfg(test)]
mod tests {
    use super::TenantTheme;

    #[test]
    fn static_content_is_allowed_only_in_lower_environments() {
        assert!(TenantTheme::Sandbox.allows_static_content());
        assert!(TenantTheme::Development.allows_static_content());
        assert!(!TenantTheme::Staging.allows_static_content());
        assert!(!TenantTheme::Production.allows_static_content());
    }
}
