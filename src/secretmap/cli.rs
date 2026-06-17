//! `aic secretmap` parser and command implementation.

use clap::Subcommand;
use serde::Serialize;
use serde_json::Value;

use crate::cli::{print_json, tenant_config_for};
use crate::config::Tenant;
use crate::secretmap::{api, labels};
use crate::{Error, Result};

#[derive(Subcommand, Debug)]
pub enum SecretmapCommand {
    /// List configured secret mappings in a realm.
    List {
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Read one raw secret mapping by AM secret label.
    Get {
        secret_id: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Re-point an AM secret label at an existing ESV secret.
    Set {
        secret_id: String,
        esv_secret_id: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        force: bool,
    },
    /// Remove an AM secret-label mapping.
    #[command(visible_alias = "delete")]
    Remove {
        secret_id: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        force: bool,
    },
    /// List valid AM secret labels in a realm.
    #[command(name = "list-labels", visible_alias = "labels")]
    Labels {
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Serialize)]
struct MappingOutput {
    secret_id: String,
    alias: Option<String>,
    description: String,
}

#[derive(Debug, Serialize)]
struct LabelOutput {
    secret_id: String,
    description: String,
    category: &'static str,
}

fn secretmap_realm(realm: Option<String>) -> Result<String> {
    let realm = realm.unwrap_or_else(|| "alpha".to_string());
    if realm == "alpha" || realm == "bravo" {
        Ok(realm)
    } else {
        Err(Error::Config(format!(
            "invalid secretmap realm {realm:?}; use alpha or bravo"
        )))
    }
}

fn tenant_for_secretmap(tenant: Option<String>) -> Result<Tenant> {
    let tenant = tenant_config_for(tenant)?;
    if !tenant.allows_secret_mappings() {
        return Err(Error::Config(format!(
            "secret mappings are only available on sandbox/development tenants (this tenant is '{}'); they are static content promoted up from lower environments",
            tenant.theme.label()
        )));
    }
    Ok(tenant)
}

fn mapping_output(mapping: &api::Mapping) -> MappingOutput {
    MappingOutput {
        secret_id: mapping.secret_id.clone(),
        alias: mapping.alias.clone(),
        description: labels::describe(&mapping.secret_id),
    }
}

fn label_output(secret_id: &str) -> LabelOutput {
    LabelOutput {
        secret_id: secret_id.to_string(),
        description: labels::describe(secret_id),
        category: labels::category(secret_id),
    }
}

fn dim(text: &str) -> String {
    format!("\x1b[2m{text}\x1b[0m")
}

fn print_mappings(mappings: &[api::Mapping]) {
    let width = mappings
        .iter()
        .map(|mapping| mapping.secret_id.len())
        .max()
        .unwrap_or(2);

    for mapping in mappings {
        println!(
            "{:<width$}  →  {}",
            mapping.secret_id,
            mapping.alias.as_deref().unwrap_or("(unset)"),
            width = width
        );
        println!("  {}", dim(&labels::describe(&mapping.secret_id)));
    }
}

fn print_labels(secret_ids: &[String]) {
    let width = secret_ids.iter().map(String::len).max().unwrap_or(2);
    for secret_id in secret_ids {
        println!("{:<width$}", secret_id, width = width);
        println!("  {}", dim(&labels::describe(secret_id)));
    }
}

fn api_not_found(error: &Error) -> bool {
    matches!(error, Error::Api { status: 404, .. })
}

fn esv_secret_exists(secrets: &[Value], esv_secret_id: &str) -> bool {
    secrets
        .iter()
        .any(|secret| secret.get("_id").and_then(Value::as_str) == Some(esv_secret_id))
}

fn alias_change_needed(current_alias: Option<&str>, new_alias: &str) -> bool {
    current_alias != Some(new_alias)
}

#[derive(Debug, PartialEq, Eq)]
enum RemoveDecision {
    NeedsForce,
    Delete,
}

fn decide_remove(force: bool) -> RemoveDecision {
    if force {
        RemoveDecision::Delete
    } else {
        RemoveDecision::NeedsForce
    }
}

fn near_matches<'a>(secret_id: &str, valid: &'a [String]) -> Vec<&'a str> {
    let needle = secret_id.to_lowercase();
    let leaf = secret_id
        .rsplit('.')
        .next()
        .unwrap_or(secret_id)
        .to_lowercase();
    let leaf_suffix = format!(".{leaf}");

    valid
        .iter()
        .filter(|candidate| {
            let candidate = candidate.to_lowercase();
            candidate.contains(&needle)
                || candidate.contains(&leaf)
                || candidate.ends_with(&leaf_suffix)
        })
        .take(5)
        .map(String::as_str)
        .collect()
}

fn invalid_label_message(secret_id: &str, valid: &[String]) -> String {
    let matches = near_matches(secret_id, valid);
    if matches.is_empty() {
        format!("{secret_id:?} is not a valid secret label; run `aic secretmap list-labels`")
    } else {
        format!(
            "{secret_id:?} is not a valid secret label; near matches: {}; run `aic secretmap list-labels`",
            matches.join(", ")
        )
    }
}

pub async fn run(cmd: SecretmapCommand) -> Result<()> {
    match cmd {
        SecretmapCommand::List {
            realm,
            tenant,
            json,
        } => {
            let tenant = tenant_for_secretmap(tenant)?;
            let tenant_name = tenant.name;
            let realm = secretmap_realm(realm)?;
            let mappings = api::list_mappings(&tenant_name, &realm).await?;
            let count = mappings.len();
            if json {
                let output: Vec<MappingOutput> = mappings.iter().map(mapping_output).collect();
                print_json(&output)?;
            } else {
                print_mappings(&mappings);
            }
            eprintln!("{count} secret mappings");
            Ok(())
        }
        SecretmapCommand::Get {
            secret_id,
            realm,
            tenant,
        } => {
            let tenant = tenant_for_secretmap(tenant)?;
            let tenant_name = tenant.name;
            let realm = secretmap_realm(realm)?;
            let mapping = match api::read_mapping(&tenant_name, &realm, &secret_id).await {
                Ok(mapping) => mapping,
                Err(error) if api_not_found(&error) => {
                    return Err(Error::Config(format!(
                        "secret label {secret_id:?} is unmapped in {tenant_name}/{realm}"
                    )));
                }
                Err(error) => return Err(error),
            };
            eprintln!("{}", labels::describe(&secret_id));
            print_json(&mapping)?;
            Ok(())
        }
        SecretmapCommand::Set {
            secret_id,
            esv_secret_id,
            realm,
            tenant,
            force,
        } => {
            let tenant = tenant_for_secretmap(tenant)?;
            let tenant_name = tenant.name;
            let realm = secretmap_realm(realm)?;

            let valid = api::valid_secret_ids(&tenant_name, &realm).await?;
            if !valid.iter().any(|candidate| candidate == &secret_id) {
                return Err(Error::Config(invalid_label_message(&secret_id, &valid)));
            }

            let secrets = crate::esv::api::list_secrets(&tenant_name).await?;
            if !esv_secret_exists(&secrets, &esv_secret_id) {
                return Err(Error::Config(format!(
                    "no ESV secret named '{esv_secret_id}' (this is the console footgun we prevent; pick an existing ESV secret)"
                )));
            }

            let current = match api::read_mapping(&tenant_name, &realm, &secret_id).await {
                Ok(mapping) => Some(mapping),
                Err(error) if api_not_found(&error) => None,
                Err(error) => return Err(error),
            };
            let current_alias = current
                .as_ref()
                .map(api::parse_mapping)
                .and_then(|mapping| mapping.alias);

            eprintln!(
                "previous alias: {}",
                current_alias.as_deref().unwrap_or("(unset)")
            );
            if !alias_change_needed(current_alias.as_deref(), &esv_secret_id) {
                println!(
                    "nothing to do: {secret_id} already points to {esv_secret_id} ({tenant_name}/{realm})"
                );
                return Ok(());
            }

            if !force {
                eprintln!("validated {secret_id} → {esv_secret_id}; applying mapping");
            }
            api::set_mapping(&tenant_name, &realm, &secret_id, &esv_secret_id, false).await?;
            println!("set {secret_id} → {esv_secret_id} ({tenant_name}/{realm})");
            Ok(())
        }
        SecretmapCommand::Remove {
            secret_id,
            realm,
            tenant,
            force,
        } => {
            let tenant = tenant_for_secretmap(tenant)?;
            let tenant_name = tenant.name;
            let realm = secretmap_realm(realm)?;

            let valid = api::valid_secret_ids(&tenant_name, &realm).await?;
            if !valid.iter().any(|candidate| candidate == &secret_id) {
                return Err(Error::Config(invalid_label_message(&secret_id, &valid)));
            }

            let current = match api::read_mapping(&tenant_name, &realm, &secret_id).await {
                Ok(mapping) => mapping,
                Err(error) if api_not_found(&error) => {
                    println!("already unmapped: {secret_id} ({tenant_name}/{realm})");
                    return Ok(());
                }
                Err(error) => return Err(error),
            };
            let current_alias = api::parse_mapping(&current)
                .alias
                .unwrap_or_else(|| "(unset)".to_string());

            match decide_remove(force) {
                RemoveDecision::NeedsForce => {
                    eprintln!("would remove mapping {secret_id} (currently → {current_alias})");
                    Err(Error::Config(
                        "pass --force to remove this secret mapping".into(),
                    ))
                }
                RemoveDecision::Delete => {
                    api::delete_mapping(&tenant_name, &realm, &secret_id).await?;
                    println!(
                        "removed mapping {secret_id} (was → {current_alias}) ({tenant_name}/{realm})"
                    );
                    Ok(())
                }
            }
        }
        SecretmapCommand::Labels {
            realm,
            tenant,
            json,
        } => {
            let tenant = tenant_for_secretmap(tenant)?;
            let tenant_name = tenant.name;
            let realm = secretmap_realm(realm)?;
            let secret_ids = api::valid_secret_ids(&tenant_name, &realm).await?;
            let count = secret_ids.len();
            if json {
                let output: Vec<LabelOutput> =
                    secret_ids.iter().map(|id| label_output(id)).collect();
                print_json(&output)?;
            } else {
                print_labels(&secret_ids);
            }
            eprintln!("{count} secret labels");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn secretmap_realm_defaults_to_alpha_and_accepts_bravo() {
        assert_eq!(secretmap_realm(None).unwrap(), "alpha");
        assert_eq!(secretmap_realm(Some("bravo".into())).unwrap(), "bravo");
    }

    #[test]
    fn secretmap_realm_rejects_other_realms() {
        let error = secretmap_realm(Some("root".into())).unwrap_err();
        assert!(error.to_string().contains("alpha or bravo"));
    }

    #[test]
    fn esv_secret_exists_checks_id_field() {
        let secrets = vec![
            json!({"_id": "esv-one"}),
            json!({"_id": "esv-two", "loaded": true}),
        ];

        assert!(esv_secret_exists(&secrets, "esv-two"));
        assert!(!esv_secret_exists(&secrets, "esv-missing"));
    }

    #[test]
    fn alias_change_needed_detects_nothing_to_do() {
        assert!(!alias_change_needed(Some("esv-current"), "esv-current"));
        assert!(alias_change_needed(Some("esv-current"), "esv-new"));
        assert!(alias_change_needed(None, "esv-new"));
    }

    #[test]
    fn remove_decision_requires_force_before_delete() {
        assert_eq!(decide_remove(false), RemoveDecision::NeedsForce);
        assert_ne!(decide_remove(false), RemoveDecision::Delete);
        assert_eq!(decide_remove(true), RemoveDecision::Delete);
    }

    #[test]
    fn invalid_label_message_includes_near_matches_when_available() {
        let valid = vec![
            "am.applications.oauth2.client.pega.secret".to_string(),
            "am.applications.oauth2.client.pega.jwt.public.key".to_string(),
        ];
        let message = invalid_label_message("pega.secret", &valid);

        assert!(message.contains("near matches"));
        assert!(message.contains("am.applications.oauth2.client.pega.secret"));
    }

    #[test]
    fn invalid_label_message_falls_back_to_list_labels_hint() {
        let message = invalid_label_message("nope", &[]);

        assert!(message.contains("not a valid secret label"));
        assert!(message.contains("aic secretmap list-labels"));
    }
}
