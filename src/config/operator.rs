//! Operator identity resolution and persistence.
//!
//! This module is surface-agnostic: CLI prompting lives in [`crate::cli`],
//! while future TUI code can use the same resolver and setters without pulling
//! terminal ownership into config. The optional service-account lookup crosses
//! through [`crate::aic::api`] so it shares the agent's bearer and HTTP path.

use super::{Operator, Settings, Tenant};
use crate::{Error, Result};

const PRINCIPAL_API_VERSION: &str = "protocol=2.1,resource=3.0";

/// Where a resolved operator name came from. `Settings` and `ServiceAccount`
/// are names; `Placeholder` is display filler only and must not be persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameSource {
    /// Explicitly persisted in `settings.toml`.
    Settings,
    /// Guessed from the configured tenant's service-account name.
    ServiceAccount,
    /// No operator name is established; this local value is display filler.
    Placeholder,
}

/// The effective operator identity for this run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedOperator {
    /// Effective human name for this run.
    pub name: String,
    /// Effective machine name for this run.
    pub host: String,
    /// How `name` was obtained.
    pub source: NameSource,
}

/// Whether operator resolution may ask the unlocked agent to read the tenant's
/// service-account principal. `Skip` guarantees no network or agent call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkAccess {
    /// The unlocked agent may make the best-effort principal read.
    Allow,
    /// Resolve exclusively from settings and local operating-system values.
    Skip,
}

impl Operator {
    /// Trim and validate a value before storing it as `operator.name`.
    pub fn validated_name(value: &str) -> Result<String> {
        validate_component("operator.name", value)
    }

    /// Trim and validate a value before storing it as `operator.host`.
    pub fn validated_host(value: &str) -> Result<String> {
        validate_component("operator.host", value)
    }
}

fn validate_component(key: &str, value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(Error::Config(format!("{key} cannot be empty")));
    }
    Ok(value.to_string())
}

/// Resolve the effective operator without ever failing because a tenant or its
/// agent is unavailable. Explicit settings always win and avoid the network.
pub async fn resolve(
    settings: &Settings,
    tenant: Option<&Tenant>,
    network: NetworkAccess,
) -> ResolvedOperator {
    let service_account_cn = if settings.operator.name.is_none() && network == NetworkAccess::Allow
    {
        resolve_service_account_cn(tenant).await
    } else {
        None
    };

    resolve_from_candidates(
        settings,
        service_account_cn.as_deref(),
        &placeholder_name(),
        system_hostname().as_deref(),
    )
}

/// Persist an explicit operator name, replacing any previous value. Empty or
/// whitespace-only names are rejected so `Some` always means established.
pub fn set_name(name: String) -> Result<()> {
    let name = Operator::validated_name(&name)?;
    let mut settings = Settings::load()?.unwrap_or_default();
    settings.operator.name = Some(name);
    settings.save()
}

/// Persist a high-quality name only when the operator has not already chosen
/// one. Returns whether this call established the setting.
pub fn set_name_if_unset(name: &str) -> Result<bool> {
    set_name_if_unset_with(name, Settings::load, Settings::save)
}

fn set_name_if_unset_with(
    name: &str,
    load: impl FnOnce() -> Result<Option<Settings>>,
    save: impl FnOnce(&Settings) -> Result<()>,
) -> Result<bool> {
    let name = Operator::validated_name(name)?;
    let mut settings = load()?.unwrap_or_default();
    if settings.operator.name.is_some() {
        return Ok(false);
    }
    settings.operator.name = Some(name);
    save(&settings)?;
    Ok(true)
}

/// Whether a service-account `cn` is likely to identify a person. Email-like
/// names and dotted letter pairs are accepted. An internal lower-to-uppercase
/// transition in an upper-camel-case owner prefix also accepts names such as
/// `DaveBalmain-fr-config-manager` without accepting timestamped tool accounts.
pub fn looks_like_person(name: &str) -> bool {
    if name.contains('@') {
        return true;
    }

    let chars = name.chars().collect::<Vec<_>>();
    let dotted_letters = chars
        .windows(3)
        .any(|window| window[0].is_alphabetic() && window[1] == '.' && window[2].is_alphabetic());
    if dotted_letters {
        return true;
    }

    name.split('-').next().is_some_and(|prefix| {
        let prefix = prefix.chars().collect::<Vec<_>>();
        prefix.first().is_some_and(|first| first.is_uppercase())
            && prefix
                .windows(2)
                .any(|pair| pair[0].is_lowercase() && pair[1].is_uppercase())
    })
}

async fn resolve_service_account_cn(tenant: Option<&Tenant>) -> Option<String> {
    let tenant = tenant?;
    let sa_id = tenant.sa_id.as_deref()?.trim();
    if sa_id.is_empty() {
        return None;
    }
    let path = format!("/am/json/realms/root/users/{sa_id}");
    let principal =
        match crate::aic::api::get_versioned(&tenant.name, &path, PRINCIPAL_API_VERSION).await {
            Ok(principal) => principal,
            Err(error) => {
                tracing::debug!(%error, tenant = %tenant.name, "operator SA lookup failed");
                return None;
            }
        };
    principal
        .get("cn")
        .and_then(serde_json::Value::as_array)
        .and_then(|values| values.first())
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

fn resolve_from_candidates(
    settings: &Settings,
    service_account_cn: Option<&str>,
    placeholder_name: &str,
    system_hostname: Option<&str>,
) -> ResolvedOperator {
    let (name, source) = if let Some(name) = settings.operator.name.as_ref() {
        (name.clone(), NameSource::Settings)
    } else if let Some(name) = service_account_cn.filter(|name| looks_like_person(name)) {
        (name.to_string(), NameSource::ServiceAccount)
    } else {
        (placeholder_name.to_string(), NameSource::Placeholder)
    };
    let host = settings
        .operator
        .host
        .clone()
        .or_else(|| system_hostname.map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_string());
    ResolvedOperator { name, host, source }
}

fn placeholder_name() -> String {
    std::env::var("USER")
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn system_hostname() -> Option<String> {
    let mut buffer = [0_u8; 256];
    // SAFETY: `buffer` is valid writable storage for the supplied length, and
    // we inspect only bytes within that initialized array after the call.
    if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } != 0 {
        return None;
    }
    let end = buffer.iter().position(|byte| *byte == 0)?;
    let hostname = String::from_utf8_lossy(&buffer[..end]).trim().to_string();
    (!hostname.is_empty()).then_some(hostname)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn person_predicate_covers_real_service_account_names() {
        assert!(!looks_like_person("Frodo-SA-1735012367301"));
        assert!(!looks_like_person("myTool-SA-1735012367301"));
        assert!(looks_like_person("DaveBalmain-fr-config-manager"));
        assert!(looks_like_person("dsbalmain@agiledigital.com.au"));
        assert!(looks_like_person("dave.balmain-service"));
    }

    #[test]
    fn operator_resolution_follows_candidate_precedence() {
        let cases = [
            (
                Some("chosen"),
                Some("service.account"),
                "local-user",
                Some("system-host"),
                ("chosen", "system-host", NameSource::Settings),
            ),
            (
                None,
                Some("service.account"),
                "local-user",
                Some("system-host"),
                ("service.account", "system-host", NameSource::ServiceAccount),
            ),
            (
                None,
                Some("Frodo-SA-1735012367301"),
                "local-user",
                None,
                ("local-user", "unknown", NameSource::Placeholder),
            ),
        ];

        for (settings_name, sa_cn, placeholder, hostname, expected) in cases {
            let mut settings = Settings::default();
            settings.operator.name = settings_name.map(str::to_owned);

            let resolved = resolve_from_candidates(&settings, sa_cn, placeholder, hostname);

            assert_eq!(
                (
                    resolved.name.as_str(),
                    resolved.host.as_str(),
                    resolved.source
                ),
                expected
            );
        }
    }

    #[test]
    fn configured_host_wins_over_system_hostname() {
        let mut settings = Settings::default();
        settings.operator.host = Some("configured-host".into());

        let resolved = resolve_from_candidates(&settings, None, "placeholder", Some("system-host"));

        assert_eq!(resolved.host, "configured-host");
    }

    #[test]
    fn set_name_if_unset_refuses_to_overwrite_an_established_name() {
        let dir = crate::config::TestDir::new();
        let path = dir.path("settings.toml");
        let mut settings = Settings::default();
        settings.operator.name = Some("chosen".into());
        settings.save_to(&path).unwrap();

        let changed = set_name_if_unset_with(
            "replacement",
            || Settings::load_from(&path).map(Some),
            |settings| settings.save_to(&path),
        )
        .unwrap();

        assert!(!changed);
        assert_eq!(
            Settings::load_from(&path).unwrap().operator.name.as_deref(),
            Some("chosen")
        );
    }

    #[test]
    fn set_name_if_unset_establishes_and_trims_a_missing_name() {
        let dir = crate::config::TestDir::new();
        let path = dir.path("settings.toml");
        Settings::default().save_to(&path).unwrap();

        let changed = set_name_if_unset_with(
            "  established  ",
            || Settings::load_from(&path).map(Some),
            |settings| settings.save_to(&path),
        )
        .unwrap();

        assert!(changed);
        assert_eq!(
            Settings::load_from(&path).unwrap().operator.name.as_deref(),
            Some("established")
        );
    }
}
