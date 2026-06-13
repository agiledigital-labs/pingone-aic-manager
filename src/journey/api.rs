//! Verified HTTP wrappers for AM journeys (authentication trees).
//! See `docs/api/09-journeys.md`.

use std::future::Future;

use serde::Serialize;
use serde_json::{Map, Value};

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

#[derive(Debug, Serialize)]
pub struct JourneyExport {
    pub tree: Value,
    pub nodes: Map<String, Value>,
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

pub async fn read_node(tenant: &str, realm: &str, node_type: &str, node_id: &str) -> Result<Value> {
    let path = format!("{}/nodes/{node_type}/{node_id}", trees_path(realm));
    crate::aic::api::get_versioned(tenant, &path, API_VERSION).await
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::*;

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
}
