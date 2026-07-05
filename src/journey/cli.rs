//! `aic journey` parser and command implementation.

use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf, is_separator};

use clap::Subcommand;
use serde_json::Value;

use crate::cli::{print_json, print_table, tenant_for};
use crate::config::ProjectConfig;
use crate::journey::api;
use crate::{Error, Result};

#[derive(Subcommand, Debug)]
pub enum JourneyCommand {
    /// List journey (auth tree) names in a realm.
    List {
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long, help = "Print journey names as JSON")]
        json: bool,
    },
    /// Pull a journey (tree + all its nodes) into the workspace as JSON.
    Pull {
        /// Journey (authentication tree) name.
        name: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Push a workspace journey export back to AIC.
    Push {
        /// Journey (authentication tree) name.
        name: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Delete a journey from AIC. Requires --force.
    Delete {
        /// Journey (authentication tree) name.
        name: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// List journeys that reference a script UUID.
    UsingScript {
        script_id: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long, help = "Print matching journey names as JSON")]
        json: bool,
    },
    /// List available journey node types.
    Nodes {
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Print a journey node type schema as JSON.
    NodeSchema {
        node_type: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Print a journey node type starter template as JSON.
    NodeTemplate {
        node_type: String,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
    },
}

fn journey_realm(realm: Option<String>) -> Result<String> {
    let realm = realm.unwrap_or_else(|| "alpha".to_string());
    if realm == "alpha" || realm == "bravo" {
        Ok(realm)
    } else {
        Err(Error::Config(format!(
            "invalid journey realm {realm:?}; use alpha or bravo"
        )))
    }
}

fn validate_journey_name(name: &str) -> Result<()> {
    if name.chars().any(is_separator) {
        return Err(Error::Config(format!(
            "journey name {name:?} contains a path separator"
        )));
    }
    Ok(())
}

fn export_path(tenant: &str, realm: &str, name: &str) -> Result<PathBuf> {
    validate_journey_name(name)?;
    Ok(ProjectConfig::workspace_tree(tenant)
        .join("journeys")
        .join(realm)
        .join(format!("{name}.json")))
}

fn snapshot_path(tenant: &str, realm: &str, name: &str) -> Result<PathBuf> {
    validate_journey_name(name)?;
    Ok(ProjectConfig::workspace_tree(tenant)
        .join("journeys")
        .join(realm)
        .join(".snapshots")
        .join(format!("{name}.json")))
}

fn node_type_matches_tag(node_type: &api::NodeType, tag: &str) -> bool {
    let needle = tag.to_lowercase();
    node_type
        .tags
        .iter()
        .any(|candidate| candidate.to_lowercase().contains(&needle))
}

fn print_node_types(node_types: &[api::NodeType]) {
    let rows = node_types
        .iter()
        .map(|node_type| {
            vec![
                node_type.id.clone(),
                node_type.name.clone(),
                node_type.tags.join(","),
                node_type.collection.to_string(),
            ]
        })
        .collect::<Vec<_>>();
    print_table(&["TYPE", "NAME", "TAGS", "COLLECTION"], &rows);
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PushBlockReason {
    MissingSnapshot,
    RemoteDrift {
        tree_changed: bool,
        nodes_changed: usize,
    },
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
        PushDecision::Blocked(remote_drift_reason(remote, snapshot))
    }
}

fn remote_drift_reason(remote: &Value, snapshot: &Value) -> PushBlockReason {
    PushBlockReason::RemoteDrift {
        tree_changed: !optional_content_equal(remote.get("tree"), snapshot.get("tree")),
        nodes_changed: changed_nodes(remote, snapshot),
    }
}

fn optional_content_equal(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => api::content_equal(a, b),
        (None, None) => true,
        _ => false,
    }
}

fn changed_nodes(remote: &Value, snapshot: &Value) -> usize {
    let remote_nodes = remote.get("nodes").and_then(Value::as_object);
    let snapshot_nodes = snapshot.get("nodes").and_then(Value::as_object);
    let mut ids = BTreeSet::new();
    if let Some(nodes) = remote_nodes {
        ids.extend(nodes.keys().map(String::as_str));
    }
    if let Some(nodes) = snapshot_nodes {
        ids.extend(nodes.keys().map(String::as_str));
    }

    ids.into_iter()
        .filter(|id| {
            !optional_content_equal(
                remote_nodes.and_then(|nodes| nodes.get(*id)),
                snapshot_nodes.and_then(|nodes| nodes.get(*id)),
            )
        })
        .count()
}

fn push_block_message(name: &str, reason: &PushBlockReason) -> String {
    match reason {
        PushBlockReason::MissingSnapshot => format!(
            "no snapshot for journey {name:?}; run `aic journey pull {name}` first or pass --force"
        ),
        PushBlockReason::RemoteDrift {
            tree_changed,
            nodes_changed,
        } => {
            let mut changed = Vec::new();
            if *tree_changed {
                changed.push("tree".to_string());
            }
            if *nodes_changed > 0 {
                changed.push(format!("{nodes_changed} nodes"));
            }
            let changed = if changed.is_empty() {
                "tree and/or nodes differ".to_string()
            } else {
                changed.join(" and ")
            };
            format!(
                "remote journey {name} changed since you last pulled ({changed}); re-pull or pass --force"
            )
        }
    }
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

fn write_snapshot(
    tenant: &str,
    realm: &str,
    name: &str,
    export: &api::JourneyExport,
) -> Result<()> {
    let path = snapshot_path(tenant, realm, name)?;
    write_bytes(&path, &serde_json::to_vec_pretty(export)?)
}

fn export_value(export: &api::JourneyExport) -> Result<Value> {
    Ok(serde_json::to_value(export)?)
}

fn parse_export_value(value: Value, label: &str) -> Result<api::JourneyExport> {
    let object = value.as_object().ok_or_else(|| {
        Error::Config(format!(
            "journey export {label} is not valid {{tree:object,nodes:object}}: top-level value is not an object"
        ))
    })?;
    let tree = object.get("tree").ok_or_else(|| {
        Error::Config(format!(
            "journey export {label} is not valid {{tree:object,nodes:object}}: missing `tree`"
        ))
    })?;
    if !tree.is_object() {
        return Err(Error::Config(format!(
            "journey export {label} is not valid {{tree:object,nodes:object}}: `tree` is not an object"
        )));
    }
    let nodes = object
        .get("nodes")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            Error::Config(format!(
                "journey export {label} is not valid {{tree:object,nodes:object}}: `nodes` is not an object"
            ))
        })?;

    Ok(api::JourneyExport {
        tree: tree.clone(),
        nodes: nodes.clone(),
    })
}

fn read_export(path: &Path, name: &str) -> Result<(api::JourneyExport, Value)> {
    let bytes = std::fs::read(path).map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            Error::Config(format!(
                "local journey export missing: {}; run `aic journey pull {name}` first",
                path.display()
            ))
        } else {
            Error::Config(format!("read journey export {}: {error}", path.display()))
        }
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        Error::Config(format!("parse journey export {}: {error}", path.display()))
    })?;
    let export = parse_export_value(value, &path.display().to_string())?;
    let value = export_value(&export)?;
    Ok((export, value))
}

fn read_snapshot(path: &Path) -> Result<Option<Value>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::Config(format!(
                "read journey snapshot {}: {error}",
                path.display()
            )));
        }
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        Error::Config(format!(
            "parse journey snapshot {}: {error}",
            path.display()
        ))
    })?;
    let export = parse_export_value(value, &path.display().to_string())?;
    Ok(Some(export_value(&export)?))
}

fn remove_snapshot_if_present(tenant: &str, realm: &str, name: &str) -> Result<()> {
    let path = snapshot_path(tenant, realm, name)?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Config(format!(
            "remove journey snapshot {}: {error}",
            path.display()
        ))),
    }
}

fn api_not_found(error: &Error) -> bool {
    matches!(error, Error::Api { status: 404, .. })
}

fn script_node_refs(tree: &Value) -> Vec<(String, String)> {
    tree.get("nodes")
        .and_then(Value::as_object)
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|(id, meta)| {
                    let node_type = meta.get("nodeType").and_then(Value::as_str)?;
                    node_type
                        .contains("Script")
                        .then(|| (id.clone(), node_type.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn node_config_references_script(node_type: &str, config: &Value, script_id: &str) -> bool {
    node_type.contains("Script")
        && ["script", "scriptId"].iter().any(|field| {
            config
                .get(*field)
                .and_then(Value::as_str)
                .is_some_and(|candidate| candidate == script_id)
        })
}

pub async fn run(cmd: JourneyCommand) -> Result<()> {
    match cmd {
        JourneyCommand::List {
            realm,
            tenant,
            json,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = journey_realm(realm)?;
            let names = api::list_trees(&tenant, &realm).await?;
            if json {
                print_json(&names)?;
            } else {
                let rows = names
                    .iter()
                    .map(|name| vec![name.clone()])
                    .collect::<Vec<_>>();
                print_table(&["JOURNEY"], &rows);
            }
            Ok(())
        }
        JourneyCommand::Pull {
            name,
            realm,
            tenant,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = journey_realm(realm)?;
            let path = export_path(&tenant, &realm, &name)?;
            let snapshot = snapshot_path(&tenant, &realm, &name)?;
            let export = api::pull(&tenant, &realm, &name).await?;
            let node_count = export.nodes.len();
            let bytes = serde_json::to_vec_pretty(&export)?;
            write_bytes(&path, &bytes)?;
            write_bytes(&snapshot, &bytes)?;
            println!(
                "pulled journey {name:?} ({node_count} nodes) -> {}",
                path.display()
            );
            Ok(())
        }
        JourneyCommand::Push {
            name,
            force,
            realm,
            tenant,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = journey_realm(realm)?;
            let path = export_path(&tenant, &realm, &name)?;
            let snapshot = snapshot_path(&tenant, &realm, &name)?;
            let (local_export, local_value) = read_export(&path, &name)?;
            let remote_export = match api::pull(&tenant, &realm, &name).await {
                Ok(export) => Some(export),
                Err(error) if api_not_found(&error) => None,
                Err(error) => return Err(error),
            };
            let remote_value = remote_export
                .as_ref()
                .map(export_value)
                .transpose()?
                .unwrap_or(Value::Null);
            let snapshot_value = read_snapshot(&snapshot)?;

            match push_decision(&local_value, &remote_value, snapshot_value.as_ref(), force) {
                PushDecision::NothingToDo => {
                    if let Some(remote_export) = remote_export.as_ref() {
                        write_snapshot(&tenant, &realm, &name, remote_export)?;
                    }
                    println!(
                        "journey {name} already matches remote ({} nodes) -> {tenant}/{realm}",
                        local_export.nodes.len()
                    );
                    Ok(())
                }
                PushDecision::Blocked(reason) => {
                    Err(Error::Config(push_block_message(&name, &reason)))
                }
                PushDecision::Push => {
                    let pushed = api::push(&tenant, &realm, &name, &local_export).await?;
                    let refreshed = api::pull(&tenant, &realm, &name).await?;
                    write_snapshot(&tenant, &realm, &name, &refreshed)?;
                    println!("pushed journey {name} ({pushed} nodes) -> {tenant}/{realm}");
                    Ok(())
                }
            }
        }
        JourneyCommand::Delete {
            name,
            force,
            realm,
            tenant,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = journey_realm(realm)?;
            if !force {
                eprintln!(
                    "would delete journey {name} from {tenant}/{realm}; pass --force to delete it"
                );
                return Err(Error::Config("journey delete requires --force".into()));
            }
            api::delete_tree(&tenant, &realm, &name).await?;
            remove_snapshot_if_present(&tenant, &realm, &name)?;
            println!("deleted journey {name}");
            Ok(())
        }
        JourneyCommand::UsingScript {
            script_id,
            realm,
            tenant,
            json,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = journey_realm(realm)?;
            let mut matches = Vec::new();
            for name in api::list_trees(&tenant, &realm).await? {
                let tree = match api::read_tree(&tenant, &realm, &name).await {
                    Ok(tree) => tree,
                    Err(error) => {
                        eprintln!("warning: could not read journey {name}: {error}");
                        continue;
                    }
                };
                let mut found = false;
                for (node_id, node_type) in script_node_refs(&tree) {
                    match api::read_node(&tenant, &realm, &node_type, &node_id).await {
                        Ok(config) => {
                            if node_config_references_script(&node_type, &config, &script_id) {
                                found = true;
                                break;
                            }
                        }
                        Err(error) => {
                            eprintln!(
                                "warning: could not read journey node {node_type}/{node_id}: {error}"
                            );
                        }
                    }
                }
                if found {
                    matches.push(name);
                }
            }
            if json {
                print_json(&matches)?;
            } else {
                let rows = matches
                    .iter()
                    .map(|name| vec![name.clone()])
                    .collect::<Vec<_>>();
                print_table(&["JOURNEY"], &rows);
            }
            eprintln!("{} journeys reference script {script_id}", matches.len());
            Ok(())
        }
        JourneyCommand::Nodes {
            tag,
            json,
            realm,
            tenant,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = journey_realm(realm)?;
            let mut node_types = api::list_node_types(&tenant, &realm).await?;
            if let Some(tag) = tag.as_deref() {
                node_types.retain(|node_type| node_type_matches_tag(node_type, tag));
            }
            let count = node_types.len();
            if json {
                print_json(&node_types)?;
            } else {
                print_node_types(&node_types);
            }
            eprintln!("{count} node types");
            Ok(())
        }
        JourneyCommand::NodeSchema {
            node_type,
            realm,
            tenant,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = journey_realm(realm)?;
            let schema = api::node_schema(&tenant, &realm, &node_type).await?;
            println!("{}", serde_json::to_string_pretty(&schema)?);
            Ok(())
        }
        JourneyCommand::NodeTemplate {
            node_type,
            realm,
            tenant,
        } => {
            let tenant = tenant_for(tenant)?;
            let realm = journey_realm(realm)?;
            let template = api::node_template(&tenant, &realm, &node_type).await?;
            println!("{}", serde_json::to_string_pretty(&template)?);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn journey_realm_defaults_to_alpha_and_accepts_bravo() {
        assert_eq!(journey_realm(None).unwrap(), "alpha");
        assert_eq!(journey_realm(Some("bravo".into())).unwrap(), "bravo");
    }

    #[test]
    fn journey_realm_rejects_other_realms() {
        let error = journey_realm(Some("root".into())).unwrap_err();
        assert!(error.to_string().contains("alpha or bravo"));
    }

    #[test]
    fn export_path_rejects_names_with_path_separators() {
        let error = export_path("sandbox", "alpha", "folder/GetIP").unwrap_err();
        assert!(error.to_string().contains("path separator"));
    }

    #[test]
    fn export_path_uses_the_journey_workspace_tree() {
        assert_eq!(
            export_path("sandbox", "bravo", "GetIP").unwrap(),
            PathBuf::from("workspace/sandbox/journeys/bravo/GetIP.json")
        );
    }

    #[test]
    fn snapshot_path_rejects_names_with_path_separators() {
        let error = snapshot_path("sandbox", "alpha", "folder/GetIP").unwrap_err();
        assert!(error.to_string().contains("path separator"));
    }

    #[test]
    fn snapshot_path_uses_snapshots_sibling_directory() {
        assert_eq!(
            snapshot_path("sandbox", "bravo", "GetIP").unwrap(),
            PathBuf::from("workspace/sandbox/journeys/bravo/.snapshots/GetIP.json")
        );
    }

    #[test]
    fn node_type_tag_filter_matches_case_insensitively() {
        let node_type = api::NodeType {
            id: "ScriptedDecisionNode".into(),
            name: "Scripted Decision".into(),
            tags: vec!["Basic Authn".into(), "Marketplace".into()],
            help: String::new(),
            collection: false,
        };

        assert!(node_type_matches_tag(&node_type, "basic"));
        assert!(node_type_matches_tag(&node_type, "AUTHN"));
        assert!(node_type_matches_tag(&node_type, "market"));
        assert!(!node_type_matches_tag(&node_type, "mfa"));
    }

    fn export_with(tree_label: &str, script_id: &str, rev: &str) -> Value {
        json!({
            "tree": {
                "_rev": rev,
                "nodes": {
                    "node-a": {
                        "displayName": tree_label,
                        "nodeType": "ScriptedDecisionNode"
                    }
                }
            },
            "nodes": {
                "node-a": {
                    "_rev": rev,
                    "script": script_id
                }
            }
        })
    }

    #[test]
    fn push_decision_returns_nothing_to_do_when_local_matches_remote() {
        let local = export_with("same", "script-a", "local-rev");
        let remote = export_with("same", "script-a", "remote-rev");

        assert_eq!(
            push_decision(&local, &remote, None, false),
            PushDecision::NothingToDo
        );
    }

    #[test]
    fn push_decision_pushes_when_remote_matches_snapshot_and_local_differs() {
        let snapshot = export_with("old", "script-a", "snapshot-rev");
        let remote = export_with("old", "script-a", "remote-rev");
        let local = export_with("new", "script-a", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, Some(&snapshot), false),
            PushDecision::Push
        );
    }

    #[test]
    fn push_decision_blocks_when_remote_drifted_without_force() {
        let snapshot = export_with("old", "script-a", "snapshot-rev");
        let remote = export_with("remote-change", "script-b", "remote-rev");
        let local = export_with("local-change", "script-a", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, Some(&snapshot), false),
            PushDecision::Blocked(PushBlockReason::RemoteDrift {
                tree_changed: true,
                nodes_changed: 1,
            })
        );
    }

    #[test]
    fn push_decision_allows_remote_drift_with_force() {
        let snapshot = export_with("old", "script-a", "snapshot-rev");
        let remote = export_with("remote-change", "script-b", "remote-rev");
        let local = export_with("local-change", "script-a", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, Some(&snapshot), true),
            PushDecision::Push
        );
    }

    #[test]
    fn push_decision_blocks_missing_snapshot_without_force() {
        let remote = export_with("old", "script-a", "remote-rev");
        let local = export_with("new", "script-a", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, None, false),
            PushDecision::Blocked(PushBlockReason::MissingSnapshot)
        );
    }

    #[test]
    fn push_decision_allows_missing_snapshot_with_force() {
        let remote = export_with("old", "script-a", "remote-rev");
        let local = export_with("new", "script-a", "local-rev");

        assert_eq!(
            push_decision(&local, &remote, None, true),
            PushDecision::Push
        );
    }

    #[test]
    fn scripted_node_matcher_checks_script_fields() {
        assert!(node_config_references_script(
            "ScriptedDecisionNode",
            &json!({"script": "script-a"}),
            "script-a"
        ));
        assert!(node_config_references_script(
            "ScriptedDecisionNode",
            &json!({"scriptId": "script-a"}),
            "script-a"
        ));
        assert!(!node_config_references_script(
            "ScriptedDecisionNode",
            &json!({"script": "script-b"}),
            "script-a"
        ));
        assert!(!node_config_references_script(
            "PageNode",
            &json!({"script": "script-a"}),
            "script-a"
        ));
    }

    #[test]
    fn script_node_refs_only_returns_script_node_types() {
        let refs = script_node_refs(&json!({
            "nodes": {
                "script": {"nodeType": "ScriptedDecisionNode"},
                "page": {"nodeType": "PageNode"}
            }
        }));

        assert_eq!(
            refs,
            vec![("script".to_string(), "ScriptedDecisionNode".to_string())]
        );
    }
}
