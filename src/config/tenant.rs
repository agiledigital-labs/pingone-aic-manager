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
}
