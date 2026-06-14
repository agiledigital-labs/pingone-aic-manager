//! Verified HTTP wrappers for AM journeys (authentication trees).
//! See `docs/api/09-journeys.md`.

use std::future::Future;
use std::path::is_separator;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{Error, Result};

const API_VERSION: &str = "protocol=2.0,resource=1.0";

fn realm_path(realm: &str) -> String {
    format!("/am/json/realms/root/realms/{realm}")
}

fn trees_path(realm: &str) -> String {
    format!(
        "{}/realm-config/authentication/authenticationtrees",
        realm_path(realm)
    )
}

fn nodes_path(realm: &str) -> String {
    format!("{}/nodes", trees_path(realm))
}

fn validate_node_type(node_type: &str) -> Result<()> {
    if node_type.chars().any(is_separator) {
        return Err(Error::Config(format!(
            "journey node type {node_type:?} contains a path separator"
        )));
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JourneyExport {
    pub tree: Value,
    pub nodes: Map<String, Value>,
}

#[derive(Debug, Serialize, Clone)]
pub struct NodeType {
    pub id: String,
    pub name: String,
    pub tags: Vec<String>,
    pub help: String,
    pub collection: bool,
}

fn parse_node_type(entry: &Value) -> NodeType {
    let tags = entry
        .get("tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    NodeType {
        id: entry
            .get("_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        name: entry
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        tags,
        help: entry
            .get("help")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        collection: entry
            .get("collection")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn parse_node_types(result: &[Value]) -> Vec<NodeType> {
    let mut node_types: Vec<NodeType> = result.iter().map(parse_node_type).collect();
    node_types
        .sort_by_cached_key(|node_type| (node_type.name.to_lowercase(), node_type.id.clone()));
    node_types
}

pub async fn list_trees(tenant: &str, realm: &str) -> Result<Vec<String>> {
    let path = format!(
        "{}/trees?_queryFilter=true&_pageSize=1000",
        trees_path(realm)
    );
    let body = crate::aic::api::get_versioned(tenant, &path, API_VERSION).await?;
    let result = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected journey list shape: {body}"),
        })?;

    let mut names: Vec<String> = result
        .iter()
        .filter_map(|tree| tree.get("_id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    names.sort();
    Ok(names)
}

pub async fn read_tree(tenant: &str, realm: &str, name: &str) -> Result<Value> {
    let path = format!("{}/trees/{name}", trees_path(realm));
    crate::aic::api::get_versioned(tenant, &path, API_VERSION).await
}

pub async fn upsert_tree(tenant: &str, realm: &str, name: &str, body: Value) -> Result<Value> {
    let path = format!("{}/trees/{name}", trees_path(realm));
    let body = strip_server_fields(&body);
    crate::aic::api::put_versioned(tenant, &path, body, false, API_VERSION).await
}

pub async fn read_node(tenant: &str, realm: &str, node_type: &str, node_id: &str) -> Result<Value> {
    validate_node_type(node_type)?;
    let path = format!("{}/{node_type}/{node_id}", nodes_path(realm));
    crate::aic::api::get_versioned(tenant, &path, API_VERSION).await
}

pub async fn upsert_node(
    tenant: &str,
    realm: &str,
    node_type: &str,
    node_id: &str,
    body: Value,
) -> Result<Value> {
    validate_node_type(node_type)?;
    let path = format!("{}/{node_type}/{node_id}", nodes_path(realm));
    let body = strip_server_fields(&body);
    crate::aic::api::put_versioned(tenant, &path, body, false, API_VERSION).await
}

pub async fn delete_tree(tenant: &str, realm: &str, name: &str) -> Result<()> {
    let path = format!("{}/trees/{name}", trees_path(realm));
    crate::aic::api::delete_versioned(tenant, &path, false, API_VERSION).await?;
    Ok(())
}

pub async fn list_node_types(tenant: &str, realm: &str) -> Result<Vec<NodeType>> {
    let path = format!("{}?_action=getAllTypes", nodes_path(realm));
    let body =
        crate::aic::api::post_versioned(tenant, &path, json!({}), false, API_VERSION).await?;
    let result = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected journey node type list shape: {body}"),
        })?;
    Ok(parse_node_types(result))
}

pub async fn node_schema(tenant: &str, realm: &str, node_type: &str) -> Result<Value> {
    validate_node_type(node_type)?;
    let path = format!("{}/{node_type}?_action=schema", nodes_path(realm));
    crate::aic::api::post_versioned(tenant, &path, json!({}), false, API_VERSION).await
}

pub async fn node_template(tenant: &str, realm: &str, node_type: &str) -> Result<Value> {
    validate_node_type(node_type)?;
    let path = format!("{}/{node_type}?_action=template", nodes_path(realm));
    crate::aic::api::post_versioned(tenant, &path, json!({}), false, API_VERSION).await
}

pub async fn list_custom_node_types(tenant: &str) -> Result<Vec<Value>> {
    let path = "/am/json/node-designer/node-type?_queryFilter=true";
    let body = crate::aic::api::get_versioned(tenant, path, API_VERSION).await?;
    let result = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected custom journey node type list shape: {body}"),
        })?;
    Ok(result.clone())
}

fn node_refs(tree: &Value) -> Result<Vec<(String, String)>> {
    let nodes = tree
        .get("nodes")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("journey tree has no `nodes` object: {tree}"),
        })?;

    nodes
        .iter()
        .map(|(id, meta)| {
            let node_type = meta
                .get("nodeType")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Api {
                    status: 0,
                    body: format!("journey node {id:?} has no string `nodeType`: {meta}"),
                })?;
            Ok((id.clone(), node_type.to_owned()))
        })
        .collect()
}

async fn assemble_export_with<F, Fut>(tree: Value, mut fetch_node: F) -> Result<JourneyExport>
where
    F: FnMut(String, String) -> Fut,
    Fut: Future<Output = Result<Value>>,
{
    let refs = node_refs(&tree)?;
    let mut nodes = Map::new();
    for (id, node_type) in refs {
        match fetch_node(node_type.clone(), id.clone()).await {
            Ok(node) => {
                nodes.insert(id, node);
            }
            Err(error) => {
                eprintln!("warning: could not read journey node {node_type}/{id}: {error}");
            }
        }
    }
    Ok(JourneyExport { tree, nodes })
}

pub async fn pull(tenant: &str, realm: &str, name: &str) -> Result<JourneyExport> {
    let tree = read_tree(tenant, realm, name).await?;
    assemble_export_with(tree, |node_type, node_id| async move {
        read_node(tenant, realm, &node_type, &node_id).await
    })
    .await
}

pub async fn push(tenant: &str, realm: &str, name: &str, export: &JourneyExport) -> Result<usize> {
    let refs = node_refs(&export.tree)?;
    let mut pushed = 0;
    for (node_id, node_type) in refs {
        let Some(node) = export.nodes.get(&node_id) else {
            eprintln!(
                "warning: journey tree references node {node_type}/{node_id}, but export has no node config; skipping"
            );
            continue;
        };
        upsert_node(tenant, realm, &node_type, &node_id, node.clone()).await?;
        pushed += 1;
    }
    upsert_tree(tenant, realm, name, export.tree.clone()).await?;
    Ok(pushed)
}

fn strip_server_fields(value: &Value) -> Value {
    let Value::Object(map) = value else {
        return value.clone();
    };

    Value::Object(
        map.iter()
            .filter(|(key, _)| !matches!(key.as_str(), "_id" | "_rev"))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

fn strip_revs(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(strip_revs).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| key.as_str() != "_rev")
                .map(|(key, value)| (key.clone(), strip_revs(value)))
                .collect(),
        ),
        value => value.clone(),
    }
}

pub(crate) fn content_equal(a: &Value, b: &Value) -> bool {
    strip_revs(a) == strip_revs(b)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn parses_full_node_type_entry() {
        let parsed = parse_node_types(&[json!({
            "_id": "ScriptedDecisionNode",
            "name": "Scripted Decision",
            "tags": ["basic authn", "marketplace"],
            "help": "Runs a script to make a decision.",
            "collection": true,
            "metadata": {
                "tags": ["ignored here"]
            }
        })]);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "ScriptedDecisionNode");
        assert_eq!(parsed[0].name, "Scripted Decision");
        assert_eq!(parsed[0].tags, vec!["basic authn", "marketplace"]);
        assert_eq!(parsed[0].help, "Runs a script to make a decision.");
        assert!(parsed[0].collection);
    }

    #[tokio::test]
    async fn node_type_missing_optional_fields_uses_defaults() {
        let parsed = parse_node_types(&[json!({
            "_id": "PageNode",
            "name": "Page"
        })]);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "PageNode");
        assert_eq!(parsed[0].name, "Page");
        assert!(parsed[0].tags.is_empty());
        assert_eq!(parsed[0].help, "");
        assert!(!parsed[0].collection);
    }

    #[tokio::test]
    async fn node_type_list_is_sorted_by_name_then_id() {
        let parsed = parse_node_types(&[
            json!({"_id": "BNode", "name": "Beta"}),
            json!({"_id": "ZNode", "name": "alpha"}),
            json!({"_id": "ANode", "name": "Alpha"}),
        ]);
        let ids: Vec<&str> = parsed
            .iter()
            .map(|node_type| node_type.id.as_str())
            .collect();

        assert_eq!(ids, vec!["ANode", "ZNode", "BNode"]);
    }

    #[tokio::test]
    async fn assembles_tree_and_fetched_nodes_but_ignores_static_nodes() {
        let tree = json!({
            "_id": "Example",
            "nodes": {
                "node-a": {"nodeType": "PageNode"},
                "node-b": {"nodeType": "ScriptedDecisionNode"}
            },
            "staticNodes": {
                "failure": {"x": 1, "y": 2}
            }
        });
        let mut stub = HashMap::from([
            (
                ("PageNode".to_string(), "node-a".to_string()),
                json!({"_id": "node-a"}),
            ),
            (
                ("ScriptedDecisionNode".to_string(), "node-b".to_string()),
                json!({"_id": "node-b", "script": "script-id"}),
            ),
        ]);

        let export = assemble_export_with(tree.clone(), move |node_type, node_id| {
            let result = stub
                .remove(&(node_type, node_id))
                .ok_or_else(|| Error::Config("unexpected node request".into()));
            async move { result }
        })
        .await
        .unwrap();

        assert_eq!(export.tree, tree);
        assert_eq!(export.nodes.len(), 2);
        assert_eq!(export.nodes["node-a"]["_id"], "node-a");
        assert_eq!(export.nodes["node-b"]["script"], "script-id");
        assert!(!export.nodes.contains_key("failure"));
    }

    #[tokio::test]
    async fn a_failed_node_fetch_does_not_abort_the_export() {
        let tree = json!({
            "nodes": {
                "good": {"nodeType": "PageNode"},
                "missing": {"nodeType": "OddBuiltInNode"}
            }
        });

        let export = assemble_export_with(tree, |_, node_id| async move {
            if node_id == "missing" {
                Err(Error::Api {
                    status: 404,
                    body: "not found".into(),
                })
            } else {
                Ok(json!({"_id": node_id}))
            }
        })
        .await
        .unwrap();

        assert_eq!(export.nodes.len(), 1);
        assert_eq!(export.nodes["good"]["_id"], "good");
    }

    #[test]
    fn strip_server_fields_removes_only_top_level_id_and_rev() {
        let value = json!({
            "_id": "tree-name",
            "_rev": "top-level-rev",
            "entryNodeId": "node-a",
            "nodes": {
                "node-a": {
                    "_id": "nested-node-id",
                    "_rev": "nested-node-rev",
                    "connections": {
                        "_id": "connection-id",
                        "_rev": "connection-rev",
                        "true": "node-b"
                    },
                    "nodeType": "ScriptedDecisionNode"
                }
            },
            "staticNodes": {},
            "uiConfig": {}
        });

        let stripped = strip_server_fields(&value);

        assert!(stripped.get("_id").is_none());
        assert!(stripped.get("_rev").is_none());
        assert_eq!(stripped["entryNodeId"], "node-a");
        assert_eq!(stripped["staticNodes"], json!({}));
        assert_eq!(stripped["uiConfig"], json!({}));
        assert_eq!(stripped["nodes"]["node-a"]["_id"], "nested-node-id");
        assert_eq!(stripped["nodes"]["node-a"]["_rev"], "nested-node-rev");
        assert_eq!(
            stripped["nodes"]["node-a"]["connections"]["_id"],
            "connection-id"
        );
        assert_eq!(
            stripped["nodes"]["node-a"]["connections"]["_rev"],
            "connection-rev"
        );
        assert_eq!(stripped["nodes"]["node-a"]["connections"]["true"], "node-b");
    }

    #[test]
    fn content_equal_ignores_rev_fields_recursively() {
        let a = json!({
            "_rev": "one",
            "tree": {
                "_rev": "two",
                "nodes": {
                    "node-a": {
                        "_rev": "three",
                        "nodeType": "PageNode"
                    }
                }
            },
            "nodes": {
                "node-a": {
                    "_rev": "four",
                    "inner": {
                        "_rev": "five",
                        "value": true
                    }
                }
            }
        });
        let b = json!({
            "_rev": "changed",
            "tree": {
                "_rev": "changed",
                "nodes": {
                    "node-a": {
                        "_rev": "changed",
                        "nodeType": "PageNode"
                    }
                }
            },
            "nodes": {
                "node-a": {
                    "_rev": "changed",
                    "inner": {
                        "_rev": "changed",
                        "value": true
                    }
                }
            }
        });

        assert!(content_equal(&a, &b));
    }

    #[test]
    fn content_equal_catches_real_differences() {
        let a = json!({
            "_rev": "one",
            "nodes": {
                "node-a": {
                    "script": "script-a"
                }
            }
        });
        let b = json!({
            "_rev": "two",
            "nodes": {
                "node-a": {
                    "script": "script-b"
                }
            }
        });

        assert!(!content_equal(&a, &b));
    }
}
