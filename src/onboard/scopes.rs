//! `aic ctx scopes`: show the scopes this install's service account holds on
//! a tenant, and make its tokens request ones it already holds.
//!
//! Two lists have to agree for a scope to work. The tenant's — the SA record at
//! `managed/svcacct/{id}`, which says what its tokens *may* carry — and the
//! local config's, which says what the agent's token requests *ask for*
//! (`aic::auth`). Onboarding writes both from `bootstrap::SA_SCOPES`, so a scope
//! added there later reaches neither for a tenant onboarded before.
//!
//! `--add` changes only the local half. The SA cannot grant itself anything:
//! a `PATCH` of its own `managed/svcacct` record is `403 Access denied`
//! (measured 2026-10-26 on two tenants), so the tenant half is an admin's —
//! the console, or re-onboarding — and `--add` refuses a scope the tenant has
//! not granted rather than record a request that would fail every token.
//!
//! API: `docs/api/00-auth.md` ("Service-account scopes").

use serde_json::Value;

use crate::{Error, Result};

/// A scope token as AIC spells them: `fr:<family>:<…>`, no whitespace (the
/// token request joins scopes with spaces, so one with a space would become
/// two).
pub fn validate(scope: &str) -> Result<()> {
    let ok = scope.starts_with("fr:")
        && scope.len() > 3
        && !scope.chars().any(|c| c.is_whitespace() || c.is_control());
    if ok {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "{scope:?} is not a scope aic grants — expected `fr:<family>:<…>`, e.g. fr:idc:promotion:*"
        )))
    }
}

/// The SA record's `scopes`, refusing a shape we do not understand rather than
/// reading it as "no scopes".
pub fn remote_scopes(record: &Value) -> Result<Vec<String>> {
    record
        .get("scopes")
        .and_then(Value::as_array)
        .and_then(|scopes| {
            scopes
                .iter()
                .map(|scope| scope.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()
        })
        .ok_or_else(|| {
            Error::Config(format!(
                "the service-account record has no readable `scopes` array: {record}"
            ))
        })
}

/// The local config's scope list after `--add`, or a refusal naming every
/// scope the tenant has not granted — requesting one fails the whole token.
pub fn plan(
    tenant: &str,
    remote: &[String],
    local: &[String],
    add: &[String],
) -> Result<Vec<String>> {
    let ungranted: Vec<&str> = add
        .iter()
        .filter(|scope| !remote.contains(scope))
        .map(String::as_str)
        .collect();
    if !ungranted.is_empty() {
        return Err(Error::Config(format!(
            "{tenant}'s service account is not granted {} — aic cannot grant it (the SA may not \
             edit its own record), so add it in the admin console under the service account's \
             scopes, then re-run this",
            ungranted.join(", ")
        )));
    }
    let mut after = local.to_vec();
    for scope in add {
        if !after.contains(scope) {
            after.push(scope.clone());
        }
    }
    Ok(after)
}

/// The scope list to save, computed against the entry as it is **now** on
/// disk rather than the one read before the tenant call: the grant check was
/// made against `checked_sa`, so an entry whose account changed meanwhile is
/// refused (the check says nothing about the new account), and scopes another
/// process added in the gap are kept rather than overwritten.
pub fn rebase(
    tenant: &str,
    checked_sa: &str,
    current_sa: Option<&str>,
    remote: &[String],
    current: &[String],
    add: &[String],
) -> Result<Vec<String>> {
    if current_sa != Some(checked_sa) {
        return Err(Error::Config(format!(
            "{tenant}'s service account changed while its grants were being read \
             ({checked_sa} -> {}); nothing saved — run this again",
            current_sa.unwrap_or("none")
        )));
    }
    plan(tenant, remote, current, add)
}

fn record_path(sa_id: &str) -> String {
    format!("/openidm/managed/svcacct/{sa_id}?_fields=scopes")
}

pub async fn run(tenant_arg: Option<String>, add: Vec<String>) -> Result<()> {
    for scope in &add {
        validate(scope)?;
    }
    let tenant = crate::cli::tenant_config_for(tenant_arg)?;
    let sa_id = tenant.sa_id.clone().ok_or_else(|| {
        Error::Config(format!(
            "tenant {} has no sa_id in .aic/config.toml, so there is no service account to read",
            tenant.name
        ))
    })?;
    let remote = remote_scopes(&crate::aic::api::get(&tenant.name, &record_path(&sa_id)).await?)?;
    if add.is_empty() {
        let rows = remote
            .iter()
            .chain(tenant.scopes.iter().filter(|s| !remote.contains(s)))
            .map(|scope| {
                vec![
                    scope.clone(),
                    yes_no(remote.contains(scope)),
                    yes_no(tenant.scopes.contains(scope)),
                ]
            })
            .collect::<Vec<_>>();
        crate::cli::print_table(&["SCOPE", "GRANTED", "REQUESTED"], &rows);
        return Ok(());
    }
    // Fail early on an ungranted scope, before touching the file.
    plan(&tenant.name, &remote, &tenant.scopes, &add)?;
    // Reload and rebase with no await between this read and the save, so the
    // window for a concurrent writer is the synchronous load→save alone.
    let mut cfg = crate::config::ProjectConfig::load()?
        .ok_or_else(|| Error::Config("no .aic/config.toml here".into()))?;
    let entry = cfg
        .tenants
        .iter_mut()
        .find(|t| t.name == tenant.name)
        .ok_or_else(|| Error::Config(format!("tenant {} vanished from config", tenant.name)))?;
    let after = rebase(
        &tenant.name,
        &sa_id,
        entry.sa_id.as_deref(),
        &remote,
        &entry.scopes,
        &add,
    )?;
    if after == entry.scopes {
        println!("nothing to do: {} already requested", add.join(", "));
        return Ok(());
    }
    entry.scopes = after.clone();
    cfg.save()?;
    println!("{}'s tokens now request: {}", tenant.name, after.join(" "));
    Ok(())
}

fn yes_no(on: bool) -> String {
    if on { "yes" } else { "no" }.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn add_records_a_granted_scope_once_and_keeps_the_rest() {
        let remote = v(&["fr:idm:*", "fr:am:*", "fr:idc:promotion:*"]);
        let local = v(&["fr:idm:*", "fr:extra:*"]);
        let add = v(&["fr:idc:promotion:*", "fr:idc:promotion:*", "fr:idm:*"]);
        assert_eq!(
            plan("t", &remote, &local, &add).unwrap(),
            v(&["fr:idm:*", "fr:extra:*", "fr:idc:promotion:*"])
        );
    }

    /// The discriminating case: one granted and one not. Recording the
    /// granted half would leave a request the tenant refuses for every token.
    #[test]
    fn add_refuses_the_whole_list_when_any_scope_is_ungranted() {
        let remote = v(&["fr:idm:*"]);
        let error = plan("dev", &remote, &[], &v(&["fr:idm:*", "fr:idc:promotion:*"]))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("fr:idc:promotion:*") && error.contains("console"),
            "{error}"
        );
        assert!(!error.contains("fr:idm:*,"), "{error}");
    }

    /// The grant check was made against `sa-1`; an entry now naming `sa-2`
    /// is refused, and a scope added concurrently to the current list is kept.
    #[test]
    fn rebase_refuses_a_changed_account_and_keeps_concurrent_additions() {
        let remote = v(&["fr:idm:*", "fr:idc:promotion:*"]);
        let add = v(&["fr:idc:promotion:*"]);
        let error = rebase("dev", "sa-1", Some("sa-2"), &remote, &[], &add)
            .unwrap_err()
            .to_string();
        assert!(error.contains("sa-1 -> sa-2"), "{error}");
        assert!(rebase("dev", "sa-1", None, &remote, &[], &add).is_err());
        let current = v(&["fr:idm:*", "fr:added:meanwhile"]);
        assert_eq!(
            rebase("dev", "sa-1", Some("sa-1"), &remote, &current, &add).unwrap(),
            v(&["fr:idm:*", "fr:added:meanwhile", "fr:idc:promotion:*"])
        );
    }

    #[test]
    fn a_record_without_a_readable_scopes_array_is_an_error_not_empty() {
        assert!(remote_scopes(&json!({"_id": "x"})).is_err());
        assert!(remote_scopes(&json!({"scopes": "fr:idm:*"})).is_err());
        assert!(remote_scopes(&json!({"scopes": ["fr:idm:*", 3]})).is_err());
        assert_eq!(
            remote_scopes(&json!({"scopes": ["fr:idm:*"]})).unwrap(),
            v(&["fr:idm:*"])
        );
    }

    #[test]
    fn validate_rejects_what_the_space_joined_request_would_split() {
        assert!(validate("fr:idc:promotion:*").is_ok());
        assert!(validate("fr:idc:promotion:* fr:am:*").is_err());
        assert!(validate("openid").is_err());
        assert!(validate("fr:").is_err());
    }
}
