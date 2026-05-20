use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TenantTheme {
    Sandbox,
    Dev,
    Staging,
    Prod,
}

impl TenantTheme {
    pub fn label(self) -> &'static str {
        match self {
            TenantTheme::Sandbox => "sandbox",
            TenantTheme::Dev     => "dev",
            TenantTheme::Staging => "staging",
            TenantTheme::Prod    => "prod",
        }
    }

    pub fn all() -> &'static [TenantTheme] {
        &[TenantTheme::Sandbox, TenantTheme::Dev, TenantTheme::Staging, TenantTheme::Prod]
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
        self.theme == TenantTheme::Prod
    }
}
