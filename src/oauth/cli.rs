//! `aic oauth` parser and command implementation.

use std::io::ErrorKind;
use std::path::{Path, PathBuf, is_separator};

use clap::Subcommand;
use serde_json::Value;

use crate::cli::tenant_for;
use crate::config::ProjectConfig;
use crate::oauth::api;
use crate::{Error, Result};

#[derive(Subcommand, Debug)]
pub enum OauthCommand {
    /// List OAuth2 client ids in a realm.
    List {
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Pull an OAuth2 client into the workspace as JSON.
    Pull {
        /// OAuth2 client id.
        id: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Push a workspace OAuth2 client JSON file back to AIC.
    Push {
        /// OAuth2 client id.
        id: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Delete an OAuth2 client from AIC. Requires --force.
    Delete {
        /// OAuth2 client id.
        id: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
}

fn oauth_realm(realm: Option<String>) -> Result<String> {
    let realm = realm.unwrap_or_else(|| "alpha".to_string());
    if realm == "alpha" || realm == "bravo" {
        Ok(realm)
    } else {
        Err(Error::Config(format!(
            "invalid oauth realm {realm:?}; use alpha or bravo"
        )))
    }
}

fn validate_client_id(id: &str) -> Result<()> {
    if id.chars().any(is_separator) {
        return Err(Error::Config(format!(
            "oauth client id {id:?} contains a path separator"
        )));
    }
    Ok(())
}

fn export_path(tenant: &str, realm: &str, id: &str) -> Result<PathBuf> {
    validate_client_id(id)?;
    Ok(ProjectConfig::workspace_tree(tenant)
        .join("oauth")
        .join(realm)
        .join(format!("{id}.json")))
}

fn snapshot_path(tenant: &str, realm: &str, id: &str) -> Result<PathBuf> {
    validate_client_id(id)?;
    Ok(ProjectConfig::workspace_tree(tenant)
        .join("oauth")
        .join(realm)
        .join(".snapshots")
        .join(format!("{id}.json")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PushBlockReason {
    MissingSnapshot,
    RemoteDrift,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PushDecision {
    Push,
    Blocked(PushBlockReason),
    NothingToDo,
}

fn push_decision(
    local: &Value,
    remote: &Value,
    snapshot: Option<&Value>,
    force: bool,
) -> PushDecision {
    if api::content_equal(local, remote) {
        return PushDecision::NothingToDo;
    }

    let Some(snapshot) = snapshot else {
        return if force {
            PushDecision::Push
        } else {
            PushDecision::Blocked(PushBlockReason::MissingSnapshot)
        };
    };

    if api::content_equal(remote, snapshot) || force {
        PushDecision::Push
    } else {
        PushDecision::Blocked(PushBlockReason::RemoteDrift)
    }
}

fn push_block_message(id: &str, reason: &PushBlockReason) -> String {
    match reason {
        PushBlockReason::MissingSnapshot => format!(
            "no snapshot for oauth client {id:?}; run `aic oauth pull {id}` first or pass --force"
        ),
        PushBlockReason::RemoteDrift => format!(
            "remote oauth client {id} changed since you last pulled; re-pull or pass --force"
        ),
    }
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

fn write_snapshot(tenant: &str, realm: &str, id: &str, client: &Value) -> Result<()> {
    let path = snapshot_path(tenant, realm, id)?;
    write_bytes(&path, &serde_json::to_vec_pretty(client)?)
}

fn parse_client_value(value: Value, label: &str) -> Result<Value> {
    if value.is_object() {
        Ok(value)
    } else {
        Err(Error::Config(format!(
            "oauth client export {label} is not valid JSON object"
        )))
    }
}

fn read_export(path: &Path, id: &str) -> Result<Value> {
    let bytes = std::fs::read(path).map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            Error::Config(format!(
                "local oauth client export missing: {}; run `aic oauth pull {id}` first",
                path.display()
            ))
        } else {
            Error::Config(format!(
                "read oauth client export {}: {error}",
                path.display()
            ))
        }
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        Error::Config(format!(
            "parse oauth client export {}: {error}",
            path.display()
        ))
    })?;
    parse_client_value(value, &path.display().to_string())
}

fn read_snapshot(path: &Path) -> Result<Option<Value>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::Config(format!(
                "read oauth client snapshot {}: {error}",
                path.display()
            )));
        }
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        Error::Config(format!(
            "parse oauth client snapshot {}: {error}",
            path.display()
        ))
    })?;
    Ok(Some(parse_client_value(
        value,
        &path.display().to_string(),
    )?))
}

fn remove_snapshot_if_present(tenant: &str, realm: &str, id: &str) -> Result<()> {
    let path = snapshot_path(tenant, realm, id)?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Config(format!(
            "remove oauth client snapshot {}: {error}",
            path.display()
        ))),
    }
}

fn api_not_found(error: &Error) -> bool {
    matches!(error, Error::Api { status: 404, .. })
}

pub async fn run(cmd: OauthCommand) -> Result<()> {
    match cmd {
        OauthCommand::List { realm, tenant } => {
            let tenant = tenant_for(tenant)?;
            let realm = oauth_realm(realm)?;
            let clients = api::list_clients(&tenant, &realm).await?;
            for id in &clients {
                println!("{id}");
            }
            eprintln!("{} oauth clients", clients.len());
            Ok(())
        }
        OauthCommand::Pull { id, realm, tenant } => {
            let tenant = tenant_for(tenant)?;
            let realm = oauth_realm(realm)?;
            let path = export_path(&tenant, &realm, &id)?;
            let snapshot = snapshot_path(&tenant, &realm, &id)?;
            let client = api::read_client(&tenant, &realm, &id).await?;
            let bytes = serde_json::to_vec_pretty(&client)?;
            write_bytes(&path, &bytes)?;
            write_bytes(&snapshot, &bytes)?;
            println!("pulled oauth client {id} -> {}", path.display());
            Ok(())
        }
        OauthCommand::Push {
            id,
            force,
            realm,
            tenant,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = oauth_realm(realm)?;
            let path = export_path(&tenant, &realm, &id)?;
            let snapshot = snapshot_path(&tenant, &realm, &id)?;
            let local = read_export(&path, &id)?;
            let remote = match api::read_client(&tenant, &realm, &id).await {
                Ok(client) => Some(client),
                Err(error) if api_not_found(&error) => None,
                Err(error) => return Err(error),
            };

            let Some(remote) = remote else {
                api::upsert_client(&tenant, &realm, &id, local).await?;
                let refreshed = api::read_client(&tenant, &realm, &id).await?;
                write_snapshot(&tenant, &realm, &id, &refreshed)?;
                println!("created oauth client {id}");
                return Ok(());
            };

            let snapshot_value = read_snapshot(&snapshot)?;
            match push_decision(&local, &remote, snapshot_value.as_ref(), force) {
                PushDecision::NothingToDo => {
                    write_snapshot(&tenant, &realm, &id, &remote)?;
                    println!("oauth client {id} already matches remote -> {tenant}/{realm}");
                    Ok(())
                }
                PushDecision::Blocked(reason) => {
                    Err(Error::Config(push_block_message(&id, &reason)))
                }
                PushDecision::Push => {
                    api::upsert_client(&tenant, &realm, &id, local).await?;
                    let refreshed = api::read_client(&tenant, &realm, &id).await?;
                    write_snapshot(&tenant, &realm, &id, &refreshed)?;
                    println!("pushed oauth client {id}");
                    Ok(())
                }
            }
        }
        OauthCommand::Delete {
            id,
            force,
            realm,
            tenant,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = oauth_realm(realm)?;
            if !force {
                eprintln!(
                    "would delete oauth client {id} from {tenant}/{realm}; pass --force to delete it"
                );
                return Err(Error::Config("oauth client delete requires --force".into()));
            }
            api::delete_client(&tenant, &realm, &id).await?;
            remove_snapshot_if_present(&tenant, &realm, &id)?;
            println!("deleted oauth client {id}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn oauth_realm_defaults_to_alpha_and_accepts_bravo() {
        assert_eq!(oauth_realm(None).unwrap(), "alpha");
        assert_eq!(oauth_realm(Some("bravo".into())).unwrap(), "bravo");
    }

    #[test]
    fn oauth_realm_rejects_other_realms() {
        let error = oauth_realm(Some("root".into())).unwrap_err();
        assert!(error.to_string().contains("alpha or bravo"));
    }

    #[test]
    fn export_path_rejects_ids_with_path_separators() {
        let error = export_path("sandbox", "alpha", "folder/client").unwrap_err();
        assert!(error.to_string().contains("path separator"));
    }

    #[test]
    fn export_path_uses_the_oauth_workspace_tree() {
        assert_eq!(
            export_path("sandbox", "bravo", "service_C1").unwrap(),
            PathBuf::from("workspace/sandbox/oauth/bravo/service_C1.json")
        );
    }

    #[test]
    fn snapshot_path_rejects_ids_with_path_separators() {
        let error = snapshot_path("sandbox", "alpha", "folder/client").unwrap_err();
        assert!(error.to_string().contains("path separator"));
    }

    #[test]
    fn snapshot_path_uses_snapshots_sibling_directory() {
        assert_eq!(
            snapshot_path("sandbox", "bravo", "service_C1").unwrap(),
            PathBuf::from("workspace/sandbox/oauth/bravo/.snapshots/service_C1.json")
        );
    }

    fn client_with(name: &str, secret: &str, rev: &str) -> Value {
        json!({
            "_id": "client-a",
            "_rev": rev,
            "coreOAuth2ClientConfig": {
                "clientName": {"inherited": false, "value": [name]},
                "userpassword": secret
            }
        })
    }

    #[test]
    fn push_decision_returns_nothing_to_do_when_local_matches_remote() {
        let local = client_with("same", "secret", "local-rev");
        let remote = client_with("same", "secret", "remote-rev");

        assert_eq!(
            push_decision(&local, &remote, None, false),
            PushDecision::NothingToDo
        );
    }

    #[test]
    fn push_decision_pushes_when_remote_matches_snapshot_and_local_differs() {
        let snapshot = client_with("old", "secret", "snapshot-rev");
        let remote = client_with("old", "secret", "remote-rev");
        let local = client_with("new", "secret", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, Some(&snapshot), false),
            PushDecision::Push
        );
    }

    #[test]
    fn push_decision_blocks_when_remote_drifted_without_force() {
        let snapshot = client_with("old", "secret", "snapshot-rev");
        let remote = client_with("remote-change", "secret", "remote-rev");
        let local = client_with("local-change", "secret", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, Some(&snapshot), false),
            PushDecision::Blocked(PushBlockReason::RemoteDrift)
        );
    }

    #[test]
    fn push_decision_allows_remote_drift_with_force() {
        let snapshot = client_with("old", "secret", "snapshot-rev");
        let remote = client_with("remote-change", "secret", "remote-rev");
        let local = client_with("local-change", "secret", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, Some(&snapshot), true),
            PushDecision::Push
        );
    }

    #[test]
    fn push_decision_blocks_missing_snapshot_without_force() {
        let remote = client_with("old", "secret", "remote-rev");
        let local = client_with("new", "secret", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, None, false),
            PushDecision::Blocked(PushBlockReason::MissingSnapshot)
        );
    }

    #[test]
    fn push_decision_allows_missing_snapshot_with_force() {
        let remote = client_with("old", "secret", "remote-rev");
        let local = client_with("new", "secret", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, None, true),
            PushDecision::Push
        );
    }
}
