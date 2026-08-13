use serde::{Deserialize, Serialize};

/// Origin of one stored credential.
///
/// `Created` means this install minted it; `External` means the user supplied
/// it. Offboarding defaults a `Created` or unknown credential on and an
/// `External` one off — but never deletes a credential another surviving
/// tenant still holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CredentialSource {
    Created,
    External,
}

/// Per-credential origin recorded at onboarding.
///
/// Each field is independent: a tenant can have a Created service account and
/// an External log key (or the reverse). A missing field is unknown, which is
/// the state of every tenant onboarded before this block existed and must stay
/// distinct from either answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_account: Option<CredentialSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_key: Option<CredentialSource>,
}

impl Provenance {
    /// Both credentials unknown — the legacy / omitted-block state.
    pub fn is_unknown(&self) -> bool {
        self.service_account.is_none() && self.log_key.is_none()
    }
}

/// Map a tenant name to the file-name used for per-tenant stores.
///
/// Not injective: any character outside `[A-Za-z0-9._-]` becomes `_`, so
/// `a b` and `a_b` share a path. Offboarding must refuse a path target when a
/// survivor collides.
pub fn tenant_file_name(tenant: &str) -> String {
    tenant
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tenant {
    pub name: String,
    pub base_url: String,
    pub theme: TenantTheme,
    #[serde(default)]
    pub sa_id: Option<String>,
    pub scopes: Vec<String>,
    /// Must stay last: TOML emits a nested table after every scalar in the
    /// parent, and a field above `scopes` would drop or misplace the array.
    #[serde(default, skip_serializing_if = "Provenance::is_unknown")]
    pub provenance: Provenance,
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
    use super::{TenantTheme, tenant_file_name};

    #[test]
    fn static_content_is_allowed_only_in_lower_environments() {
        assert!(TenantTheme::Sandbox.allows_static_content());
        assert!(TenantTheme::Development.allows_static_content());
        assert!(!TenantTheme::Staging.allows_static_content());
        assert!(!TenantTheme::Production.allows_static_content());
    }

    #[test]
    fn tenant_file_name_collides_space_with_underscore() {
        // Treating sanitisation as injective, or comparing raw names in the
        // offboard path guard, lets `a b` delete `a_b`'s store.
        assert_eq!(tenant_file_name("a b"), "a_b");
        assert_eq!(tenant_file_name("a b"), tenant_file_name("a_b"));
        assert_ne!(tenant_file_name("uat"), tenant_file_name("UAT"));
    }
}
