//! Verified HTTP wrappers for AM journeys (authentication trees).
//! See `docs/api/09-journeys.md`.

use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::path::is_separator;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::aic::api::ApiCall;
use crate::{Error, Result};

const API_VERSION: &str = "protocol=2.0,resource=1.0";

fn versioned_call<'a>(
    tenant: &'a str,
    method: &'a str,
    path: &'a str,
    body: Option<Value>,
    confirmed_prod: bool,
) -> ApiCall<'a> {
    let mut call = ApiCall::new(tenant, method, path)
        .confirmed_prod(confirmed_prod)
        .api_version(API_VERSION);
    if let Some(body) = body {
        call = call.body(body);
    }
    call
}

fn read_action_call<'a>(tenant: &'a str, path: &'a str) -> ApiCall<'a> {
    ApiCall::read_only_action(tenant, path)
        .api_version(API_VERSION)
        .body(json!({}))
}

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

pub async fn upsert_tree(
    tenant: &str,
    realm: &str,
    name: &str,
    body: Value,
    confirmed_prod: bool,
) -> Result<Value> {
    let path = format!("{}/trees/{name}", trees_path(realm));
    let body = strip_server_fields(&body);
    versioned_call(tenant, "PUT", &path, Some(body), confirmed_prod)
        .send()
        .await
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
    confirmed_prod: bool,
) -> Result<Value> {
    validate_node_type(node_type)?;
    let path = format!("{}/{node_type}/{node_id}", nodes_path(realm));
    let body = strip_server_fields(&body);
    versioned_call(tenant, "PUT", &path, Some(body), confirmed_prod)
        .send()
        .await
}

pub async fn delete_tree(
    tenant: &str,
    realm: &str,
    name: &str,
    confirmed_prod: bool,
) -> Result<()> {
    let path = format!("{}/trees/{name}", trees_path(realm));
    versioned_call(tenant, "DELETE", &path, None, confirmed_prod)
        .send()
        .await?;
    Ok(())
}

pub async fn list_node_types(tenant: &str, realm: &str) -> Result<Vec<NodeType>> {
    let path = format!("{}?_action=getAllTypes", nodes_path(realm));
    let body = read_action_call(tenant, &path).send().await?;
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
    read_action_call(tenant, &path).send().await
}

pub async fn node_template(tenant: &str, realm: &str, node_type: &str) -> Result<Value> {
    validate_node_type(node_type)?;
    let path = format!("{}/{node_type}?_action=template", nodes_path(realm));
    read_action_call(tenant, &path).send().await
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

/// The node type whose config lists child nodes of its own.
const PAGE_NODE: &str = "PageNode";

/// One node a journey references: listed by the tree, or by a page's config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NodeRef {
    pub id: String,
    pub node_type: String,
    pub display_name: Option<String>,
    /// The page whose `nodes` lists it; `None` for a node the tree lists.
    pub page: Option<String>,
}

impl NodeRef {
    fn label(&self) -> String {
        format!("{}/{}", self.node_type, self.id)
    }
}

/// The child nodes a node's config lists.
///
/// Only a `PageNode` has any: its config carries
/// `nodes: [{_id, nodeType, displayName}]`, and each child is a realm node
/// object at `…/nodes/<type>/<id>` that the tree's own `nodes` map never
/// mentions (measured 2026-11-25, `docs/api/09-journeys.md`). A listed child
/// that cannot be addressed is an error, not a skip: leaving it out is how a
/// pull used to look complete while holding only the page. So is a page with
/// no `nodes` at all — every page AM returns carries the key, so its absence
/// is an incomplete read, not an empty page (`docs/api/23-promotion.md` treats
/// it the same way).
pub(crate) fn page_children(
    page_id: &str,
    node_type: &str,
    config: &Value,
) -> Result<Vec<NodeRef>> {
    if node_type != PAGE_NODE {
        return Ok(Vec::new());
    }
    let shape_error = |what: String| Error::Api {
        status: 0,
        body: format!("page node {page_id}: {what}"),
    };
    let children = config
        .get("nodes")
        .ok_or_else(|| shape_error("config has no `nodes`, so its children are unknown".into()))?;
    let children = children
        .as_array()
        .ok_or_else(|| shape_error(format!("`nodes` is not an array: {children}")))?;
    children
        .iter()
        .map(|child| {
            let field = |key| child.get(key).and_then(Value::as_str).map(str::to_owned);
            match (field("_id"), field("nodeType")) {
                (Some(id), Some(node_type)) => Ok(NodeRef {
                    id,
                    node_type,
                    display_name: field("displayName"),
                    page: Some(page_id.to_owned()),
                }),
                _ => Err(shape_error(format!(
                    "child needs a string `_id` and `nodeType`: {child}"
                ))),
            }
        })
        .collect()
}

/// Every node a journey references, each once, a page's children before the
/// page — the order a push writes them in — and the problems that make the
/// set ambiguous.
///
/// Lenient, for the read-only comparisons: a problem is reported, and the
/// walk carries on with what it can address. [`journey_refs`] is the strict
/// form every read from or write to the tenant goes through.
///
/// The problems:
/// - **one id under two node types.** A node is a realm object at
///   `…/nodes/<type>/<id>`, and the export keeps one config per id, so which
///   endpoint that config came from or goes to would be a guess. The first
///   listing wins here.
/// - **a page inside a page.** AM refuses one (measured 2026-11-25:
///   `400 Invalid config: Illegal child node type: PageNode`); every cycle is
///   one, since only a page lists nodes.
/// - **a page whose listing cannot be read** ([`page_children`]).
pub(crate) fn walk_refs(tree: &Value, nodes: &Map<String, Value>) -> (Vec<NodeRef>, Vec<String>) {
    struct Walk<'a> {
        nodes: &'a Map<String, Value>,
        types: std::collections::HashMap<String, String>,
        refs: Vec<NodeRef>,
        problems: Vec<String>,
    }
    impl Walk<'_> {
        fn visit(&mut self, node: NodeRef) {
            // Before the seen check: a page listing itself is seen already.
            if let (Some(page), PAGE_NODE) = (&node.page, node.node_type.as_str()) {
                self.problems.push(format!(
                    "page {page} holds page {}; AM refuses a page inside a page",
                    node.id
                ));
                return;
            }
            if let Some(first) = self.types.get(&node.id) {
                if *first != node.node_type {
                    self.problems.push(format!(
                        "node {} is listed as both {first} and {}",
                        node.id, node.node_type
                    ));
                }
                return;
            }
            self.types.insert(node.id.clone(), node.node_type.clone());
            if let Some(config) = self.nodes.get(&node.id) {
                match page_children(&node.id, &node.node_type, config) {
                    Ok(children) => children.into_iter().for_each(|child| self.visit(child)),
                    Err(error) => self.problems.push(error.to_string()),
                }
            }
            self.refs.push(node);
        }
    }

    let mut walk = Walk {
        nodes,
        types: Default::default(),
        refs: Vec::new(),
        problems: Vec::new(),
    };
    match node_refs(tree) {
        Ok(listed) => {
            let tree_nodes = tree.get("nodes");
            for (id, node_type) in listed {
                let display_name = tree_nodes
                    .and_then(|nodes| nodes.get(&id))
                    .and_then(|meta| meta.get("displayName"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                walk.visit(NodeRef {
                    id,
                    node_type,
                    display_name,
                    page: None,
                });
            }
        }
        Err(error) => walk.problems.push(error.to_string()),
    }
    (walk.refs, walk.problems)
}

/// [`walk_refs`], refusing any problem: nothing that reads a journey from the
/// tenant or writes one to it proceeds on an ambiguous set of nodes.
pub(crate) fn journey_refs(tree: &Value, nodes: &Map<String, Value>) -> Result<Vec<NodeRef>> {
    let (refs, problems) = walk_refs(tree, nodes);
    if problems.is_empty() {
        Ok(refs)
    } else {
        let name = tree.get("_id").and_then(Value::as_str).unwrap_or("?");
        Err(Error::Config(format!(
            "journey {name}: {}",
            problems.join("; ")
        )))
    }
}

/// The tree and every node it lists that exists, or an error naming each node
/// that could not be read. A page's children count as listed: they are read
/// after the page that names them, and recursively. The result is then held
/// to [`journey_refs`], so a journey whose nodes are ambiguous is refused, not
/// exported.
///
/// **A 404 is an answer; anything else is not.** This used to warn and leave
/// any unread node out, which every caller then read as content: `journey
/// status` called a 503 remote drift, `journey diff` showed the node deleted,
/// and `pull` wrote an export — and a snapshot — that a later status compared
/// equal to a tenant it had never fully read. So any failure other than 404
/// fails the whole export. A node the tree lists that answers 404 (measured
/// 2026-10-31: `404 Not Found` for a node id that does not exist) really is
/// absent, and is left out: that is what lets `journey push --force` re-create
/// it from a local export. Every node of every journey in alpha and bravo (960)
/// read 200 the same day, so neither case is routine.
async fn assemble_export_with<F, Fut>(tree: Value, mut fetch_node: F) -> Result<JourneyExport>
where
    F: FnMut(String, String) -> Fut,
    Fut: Future<Output = Result<Value>>,
{
    let mut queue: VecDeque<(String, String)> = node_refs(&tree)?.into();
    let mut seen: HashSet<String> = queue.iter().map(|(id, _)| id.clone()).collect();
    let mut nodes = Map::new();
    let mut unread = Vec::new();
    while let Some((id, node_type)) = queue.pop_front() {
        match fetch_node(node_type.clone(), id.clone()).await {
            Ok(node) => {
                match page_children(&id, &node_type, &node) {
                    Ok(children) => {
                        for child in children {
                            if seen.insert(child.id.clone()) {
                                queue.push_back((child.id, child.node_type));
                            }
                        }
                    }
                    Err(error) => unread.push(format!("{node_type}/{id} ({error})")),
                }
                nodes.insert(id, node);
            }
            Err(Error::Api { status: 404, .. }) => {}
            Err(error) => unread.push(format!("{node_type}/{id} ({error})")),
        }
    }
    if !unread.is_empty() {
        let name = tree.get("_id").and_then(Value::as_str).unwrap_or("?");
        return Err(Error::Config(format!(
            "journey {name}: could not read {} node(s) from the tenant: {}",
            unread.len(),
            unread.join("; ")
        )));
    }
    journey_refs(&tree, &nodes)?;
    Ok(JourneyExport { tree, nodes })
}

pub async fn pull(tenant: &str, realm: &str, name: &str) -> Result<JourneyExport> {
    let tree = read_tree(tenant, realm, name).await?;
    assemble_export_with(tree, |node_type, node_id| async move {
        read_node(tenant, realm, &node_type, &node_id).await
    })
    .await
}

/// The node writes a push makes, in order, and the tree-listed nodes it skips
/// because the export carries no config for them.
///
/// **A page's children come before the page.** AM validates a page on write
/// and refuses one naming a child that does not exist (measured 2026-11-25:
/// `400 Invalid config: Node does not exist: <id>`), so the order is not
/// cosmetic.
///
/// **A page child without a config refuses the push** rather than skipping:
/// an export written before `aic` carried children holds the page alone, and
/// writing that page either fails at AM (child absent on the tenant) or
/// silently points it at whatever child config the tenant has — neither is
/// the export the operator pushed. A tree-listed node without one is skipped
/// with a warning, as it always was.
fn push_plan(export: &JourneyExport) -> Result<(Vec<NodeRef>, Vec<NodeRef>)> {
    let (writes, absent): (Vec<NodeRef>, Vec<NodeRef>) = journey_refs(&export.tree, &export.nodes)?
        .into_iter()
        .partition(|node| export.nodes.contains_key(&node.id));
    let orphans: Vec<String> = absent
        .iter()
        .filter_map(|node| {
            node.page
                .as_ref()
                .map(|page| format!("{} (in page {page})", node.label()))
        })
        .collect();
    if !orphans.is_empty() {
        let name = export
            .tree
            .get("_id")
            .and_then(Value::as_str)
            .unwrap_or("?");
        return Err(Error::Config(format!(
            "journey {name}: the export has no config for page child node(s) {}; it was \
             probably pulled by an older `aic` — pull the journey again (local edits are \
             backed up), then push. Nothing was written",
            orphans.join(", ")
        )));
    }
    Ok((writes, absent))
}

/// A push that stopped partway: what landed before the failing write.
///
/// The writes are separate requests with no transaction, so a failure after
/// the first leaves those before it on the tenant — a child already rewritten
/// is live in an existing page. Reporting only the error would tell the
/// operator nothing changed (`.ai/core.md` §5, the batch rule). The snapshot
/// is not advanced: the next `status` compares against the tenant as it is.
#[derive(Debug)]
pub struct PushError {
    landed: Vec<String>,
    failed: String,
    total: usize,
    source: Box<Error>,
}

impl PushError {
    /// The writes that completed before the failure, as `type/id` or `tree`.
    pub fn landed(&self) -> &[String] {
        &self.landed
    }
}

impl std::fmt::Display for PushError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} of {} writes landed, then {} failed: {}",
            self.landed.len(),
            self.total,
            self.failed,
            self.source
        )?;
        if !self.landed.is_empty() {
            write!(
                f,
                "\n  already written to the tenant: {}",
                self.landed.join(", ")
            )?;
        }
        write!(
            f,
            "\n  the snapshot was not updated; `aic journey status` shows the tenant as it now is"
        )
    }
}

impl std::error::Error for PushError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

impl From<PushError> for Error {
    fn from(error: PushError) -> Self {
        Error::Config(error.to_string())
    }
}

/// The push's write loop: each planned node in order, then the tree. The
/// senders are injected so a test drives this loop and not a copy of it.
async fn push_with<N, NF, T, TF>(
    export: &JourneyExport,
    writes: Vec<NodeRef>,
    mut send_node: N,
    send_tree: T,
) -> std::result::Result<usize, PushError>
where
    N: FnMut(NodeRef, Value) -> NF,
    NF: Future<Output = Result<Value>>,
    T: FnOnce(Value) -> TF,
    TF: Future<Output = Result<Value>>,
{
    let total = writes.len() + 1;
    let mut landed = Vec::new();
    for node in writes {
        let label = node.label();
        let body = export.nodes[&node.id].clone();
        if let Err(source) = send_node(node, body).await {
            return Err(PushError {
                landed,
                failed: label,
                total,
                source: Box::new(source),
            });
        }
        landed.push(label);
    }
    if let Err(source) = send_tree(export.tree.clone()).await {
        return Err(PushError {
            landed,
            failed: "tree".into(),
            total,
            source: Box::new(source),
        });
    }
    Ok(total - 1)
}

/// Push an export: its nodes, then its tree. Planned in full before the first
/// write, so a malformed export leaves nothing on the tenant; a write that
/// fails after that is a [`PushError`] naming what landed.
pub async fn push(
    tenant: &str,
    realm: &str,
    name: &str,
    export: &JourneyExport,
    confirmed_prod: bool,
) -> Result<usize> {
    let (writes, absent) = push_plan(export)?;
    for node in &absent {
        eprintln!(
            "warning: journey references node {}, but export has no node config; skipping",
            node.label()
        );
    }
    let pushed = push_with(
        export,
        writes,
        |node, body| async move {
            upsert_node(
                tenant,
                realm,
                &node.node_type,
                &node.id,
                body,
                confirmed_prod,
            )
            .await
        },
        |tree| upsert_tree(tenant, realm, name, tree, confirmed_prod),
    )
    .await?;
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
    use crate::agent::Request;

    fn request(call: ApiCall<'_>) -> crate::agent::ApiCallRequest {
        let Request::ApiCall(request) = call.envelope() else {
            panic!("expected API call");
        };
        request
    }

    #[test]
    fn node_introspection_posts_bypass_prod_but_journey_writes_do_not() {
        for path in [
            format!("{}?_action=getAllTypes", nodes_path("alpha")),
            format!("{}/PageNode?_action=schema", nodes_path("alpha")),
            format!("{}/PageNode?_action=template", nodes_path("alpha")),
        ] {
            let request = request(read_action_call("prod", &path));
            assert_eq!(request.method, "POST");
            assert!(request.confirmed_prod);
        }

        let path = format!("{}/trees/Login", trees_path("alpha"));
        assert!(
            !request(versioned_call("prod", "PUT", &path, Some(json!({})), false,)).confirmed_prod
        );
        assert!(
            request(versioned_call("prod", "PUT", &path, Some(json!({})), true,)).confirmed_prod
        );
    }

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
                json!({"_id": "node-a", "nodes": []}),
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
    async fn a_failed_node_fetch_fails_the_export_but_an_absent_node_does_not() {
        let tree = json!({
            "nodes": {
                "good": {"nodeType": "SetSuccessUrlNode"},
                "gone": {"nodeType": "SetSuccessUrlNode"},
                "busy": {"nodeType": "OddBuiltInNode"}
            }
        });
        let fetch = |fail_busy: bool| {
            move |_: String, node_id: String| async move {
                match node_id.as_str() {
                    "gone" => Err(Error::Api {
                        status: 404,
                        body: "Not Found".into(),
                    }),
                    "busy" if fail_busy => Err(Error::Api {
                        status: 503,
                        body: "unavailable".into(),
                    }),
                    _ => Ok(json!({"_id": node_id})),
                }
            }
        };

        let error = assemble_export_with(tree.clone(), fetch(true))
            .await
            .expect_err("an unread node is not an absent one")
            .to_string();
        assert!(error.contains("OddBuiltInNode/busy"), "{error}");
        assert!(!error.contains("gone"), "{error}");

        let export = assemble_export_with(tree, fetch(false)).await.unwrap();
        let mut ids: Vec<&String> = export.nodes.keys().collect();
        ids.sort();
        assert_eq!(
            ids,
            ["busy", "good"],
            "the 404 node is absent, not an error"
        );
    }

    fn page_tree() -> Value {
        json!({
            "_id": "Example",
            "nodes": {
                "page": {"nodeType": "PageNode"},
                "zafter": {"nodeType": "SetSuccessUrlNode"}
            }
        })
    }

    fn page_config() -> Value {
        json!({"_id": "page", "nodes": [
            {"_id": "user", "nodeType": "UsernameCollectorNode", "displayName": "u"},
            {"_id": "script", "nodeType": "ScriptedDecisionNode", "displayName": "s"}
        ]})
    }

    #[tokio::test]
    async fn a_page_pulls_with_its_children_and_a_pageless_tree_is_unchanged() {
        let fetch = |node_type: String, id: String| {
            let seen = (node_type, id);
            async move {
                match seen.1.as_str() {
                    "page" => Ok(page_config()),
                    other => Ok(json!({"_id": other})),
                }
            }
        };
        let export = assemble_export_with(page_tree(), fetch).await.unwrap();
        let mut ids: Vec<&String> = export.nodes.keys().collect();
        ids.sort();
        assert_eq!(ids, ["page", "script", "user", "zafter"]);

        // The control: no page in the tree, so no child reads and the same
        // map the tree alone names.
        let tree = json!({"nodes": {"a": {"nodeType": "SetSuccessUrlNode"}}});
        let mut requests = Vec::new();
        let export = assemble_export_with(tree, |node_type, id| {
            requests.push((node_type, id.clone()));
            async move { Ok(json!({"_id": id})) }
        })
        .await
        .unwrap();
        assert_eq!(export.nodes.len(), 1);
        assert_eq!(
            requests,
            [("SetSuccessUrlNode".to_string(), "a".to_string())]
        );
    }

    #[tokio::test]
    async fn a_failed_child_read_fails_the_export_and_an_absent_child_does_not() {
        let fetch = |busy: bool| {
            move |_: String, id: String| async move {
                match id.as_str() {
                    "page" => Ok(page_config()),
                    "user" => Err(Error::Api {
                        status: 404,
                        body: "Not Found".into(),
                    }),
                    "script" if busy => Err(Error::Api {
                        status: 503,
                        body: "unavailable".into(),
                    }),
                    other => Ok(json!({"_id": other})),
                }
            }
        };
        let error = assemble_export_with(page_tree(), fetch(true))
            .await
            .expect_err("an unread child is not an absent one")
            .to_string();
        assert!(error.contains("ScriptedDecisionNode/script"), "{error}");
        assert!(!error.contains("user"), "{error}");
        let export = assemble_export_with(page_tree(), fetch(false))
            .await
            .unwrap();
        assert!(!export.nodes.contains_key("user"));
        assert!(export.nodes.contains_key("script"));
    }

    #[tokio::test]
    async fn a_page_read_without_its_nodes_fails_the_pull() {
        // Regression (review P1-2): a page answered without `nodes` was read
        // as an empty page, so the pull installed the page alone and moved
        // the snapshot to match.
        let fetch = |_: String, id: String| async move {
            match id.as_str() {
                "page" => Ok(json!({"_id": "page", "pageHeader": {}})),
                other => Ok(json!({"_id": other})),
            }
        };
        let error = assemble_export_with(page_tree(), fetch)
            .await
            .expect_err("a page without `nodes` is an incomplete read")
            .to_string();
        assert!(error.contains("PageNode/page"), "{error}");
        assert!(error.contains("no `nodes`"), "{error}");
    }

    #[tokio::test]
    async fn one_id_under_two_node_types_is_refused_on_pull_and_push() {
        // Regression (review P2-2): the tree lists `zafter` as a
        // SetSuccessUrlNode and the page lists it as a username collector.
        // The export keeps one config per id, so which endpoint it belongs to
        // would be a guess.
        let config = json!({"_id": "page", "nodes": [
            {"_id": "zafter", "nodeType": "UsernameCollectorNode"}
        ]});
        let fetch = |_: String, id: String| {
            let config = config.clone();
            async move {
                match id.as_str() {
                    "page" => Ok(config),
                    other => Ok(json!({"_id": other})),
                }
            }
        };
        let error = assemble_export_with(page_tree(), fetch)
            .await
            .expect_err("pull refuses")
            .to_string();
        let names_both = |error: &str| {
            error.contains("zafter")
                && error.contains("SetSuccessUrlNode")
                && error.contains("UsernameCollectorNode")
        };
        assert!(names_both(&error), "{error}");

        let export = export_of(&[("page", config.clone()), ("zafter", json!({}))]);
        let error = push_plan(&export).expect_err("push refuses").to_string();
        assert!(names_both(&error), "{error}");
    }

    fn export_of(nodes: &[(&str, Value)]) -> JourneyExport {
        JourneyExport {
            tree: page_tree(),
            nodes: nodes
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }

    fn whole_export() -> JourneyExport {
        export_of(&[
            ("page", page_config()),
            ("zafter", json!({"_id": "zafter"})),
            ("user", json!({})),
            ("script", json!({})),
        ])
    }

    fn ids(refs: &[NodeRef]) -> Vec<&str> {
        refs.iter().map(|node| node.id.as_str()).collect()
    }

    #[test]
    fn push_writes_a_pages_children_before_the_page() {
        let (writes, absent) = push_plan(&whole_export()).unwrap();
        assert_eq!(ids(&writes), ["user", "script", "page", "zafter"]);
        assert_eq!(writes[0].node_type, "UsernameCollectorNode");
        assert_eq!(writes[0].page.as_deref(), Some("page"));
        assert!(absent.is_empty());
    }

    /// Drives the real write loop with a recording sender. The
    /// discriminating input is a page with two children on an empty realm:
    /// a loop run in any other order sends the page before a child, which AM
    /// refuses.
    #[tokio::test]
    async fn the_push_loop_sends_children_then_the_page_then_the_tree() {
        let export = whole_export();
        let sent = std::cell::RefCell::new(Vec::new());
        let (writes, _) = push_plan(&export).unwrap();
        let pushed = push_with(
            &export,
            writes,
            |node, _| {
                sent.borrow_mut().push(node.label());
                async { Ok(json!({})) }
            },
            |_| {
                sent.borrow_mut().push("tree".to_string());
                async { Ok(json!({})) }
            },
        )
        .await
        .unwrap();
        assert_eq!(pushed, 4);
        assert_eq!(
            sent.into_inner(),
            [
                "UsernameCollectorNode/user",
                "ScriptedDecisionNode/script",
                "PageNode/page",
                "SetSuccessUrlNode/zafter",
                "tree"
            ]
        );
    }

    /// Regression (review P1-1): a failure on the second child returned the
    /// bare error, so the operator was not told the first child had already
    /// been rewritten on the tenant.
    #[tokio::test]
    async fn a_push_that_fails_partway_names_what_landed() {
        let export = whole_export();
        let sent = std::cell::RefCell::new(Vec::new());
        let (writes, _) = push_plan(&export).unwrap();
        let error = push_with(
            &export,
            writes,
            |node, _| {
                sent.borrow_mut().push(node.label());
                let busy = node.id == "script";
                async move {
                    if busy {
                        Err(Error::Api {
                            status: 503,
                            body: "unavailable".into(),
                        })
                    } else {
                        Ok(json!({}))
                    }
                }
            },
            |_| {
                sent.borrow_mut().push("tree".to_string());
                async { Ok(json!({})) }
            },
        )
        .await
        .expect_err("the second child fails");
        // Nothing after the failing write is attempted.
        assert_eq!(
            sent.into_inner(),
            ["UsernameCollectorNode/user", "ScriptedDecisionNode/script"]
        );
        assert_eq!(error.landed(), ["UsernameCollectorNode/user"]);
        let message = error.to_string();
        assert!(
            message.contains("1 of 5 writes landed, then ScriptedDecisionNode/script failed"),
            "{message}"
        );
        assert!(
            message.contains("already written to the tenant: UsernameCollectorNode/user"),
            "{message}"
        );

        // A failed tree write: every node landed.
        let (writes, _) = push_plan(&export).unwrap();
        let error = push_with(
            &export,
            writes,
            |_, _| async { Ok(json!({})) },
            |_| async {
                Err(Error::Api {
                    status: 400,
                    body: "bad tree".into(),
                })
            },
        )
        .await
        .expect_err("the tree fails");
        assert_eq!(error.landed().len(), 4);
        assert!(error.to_string().contains("then tree failed"));
    }

    /// Regression (review P2-1): an export pulled before children were
    /// carried holds the page alone, and push used to write that page anyway.
    #[test]
    fn an_export_without_its_page_children_refuses_the_push() {
        let error = push_plan(&export_of(&[
            ("page", page_config()),
            ("zafter", json!({})),
        ]))
        .expect_err("the page's children have no config")
        .to_string();
        assert!(
            error.contains("UsernameCollectorNode/user (in page page)"),
            "{error}"
        );
        assert!(error.contains("ScriptedDecisionNode/script"), "{error}");
        assert!(error.contains("pull the journey again"), "{error}");

        // The control: a tree-listed node without a config is skipped with a
        // warning, as before.
        let (writes, absent) = push_plan(&export_of(&[
            ("page", page_config()),
            ("user", json!({})),
            ("script", json!({})),
        ]))
        .unwrap();
        assert_eq!(ids(&writes), ["user", "script", "page"]);
        assert_eq!(ids(&absent), ["zafter"]);
    }

    #[test]
    fn a_pageless_push_plan_is_the_trees_own_order() {
        let tree = json!({"nodes": {"b": {"nodeType": "X"}, "a": {"nodeType": "Y"}}});
        let export = JourneyExport {
            tree: tree.clone(),
            nodes: [("a", json!({})), ("b", json!({}))]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        };
        let expected: Vec<String> = node_refs(&tree)
            .unwrap()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(ids(&push_plan(&export).unwrap().0), expected);
    }

    #[test]
    fn page_children_refuses_a_listing_it_cannot_read() {
        assert!(page_children("p", "PageNode", &json!({"nodes": [{"_id": "x"}]})).is_err());
        assert!(page_children("p", "PageNode", &json!({"nodes": "x"})).is_err());
        assert!(page_children("p", "PageNode", &json!({})).is_err());
        assert!(
            page_children("p", "PageNode", &json!({"nodes": []}))
                .unwrap()
                .is_empty()
        );
        // Only a page has children, whatever else carries a `nodes` key.
        let config = page_config();
        assert!(
            page_children("p", "InnerTreeEvaluatorNode", &config)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_child_listed_twice_is_written_once() {
        let twice = json!({"nodes": [
            {"_id": "user", "nodeType": "UsernameCollectorNode"},
            {"_id": "user", "nodeType": "UsernameCollectorNode"}
        ]});
        let export = export_of(&[("page", twice), ("zafter", json!({})), ("user", json!({}))]);
        assert_eq!(
            ids(&push_plan(&export).unwrap().0),
            ["user", "page", "zafter"]
        );
    }

    /// Regression (review P2-3): a page inside a page was planned and
    /// written child-first, so AM's refusal came only at the outer page,
    /// after two writes had landed. A page listing itself is the same case.
    #[test]
    fn a_page_inside_a_page_is_refused_before_any_write() {
        let outer = json!({"nodes": [
            {"_id": "inner", "nodeType": "PageNode"},
            {"_id": "user", "nodeType": "UsernameCollectorNode"}
        ]});
        let inner = json!({"nodes": [{"_id": "script", "nodeType": "ScriptedDecisionNode"}]});
        let export = export_of(&[
            ("page", outer),
            ("inner", inner),
            ("user", json!({})),
            ("script", json!({})),
            ("zafter", json!({})),
        ]);
        let error = push_plan(&export).expect_err("nested page").to_string();
        assert!(error.contains("page page holds page inner"), "{error}");

        let looping = json!({"nodes": [{"_id": "page", "nodeType": "PageNode"}]});
        let error = push_plan(&export_of(&[("page", looping), ("zafter", json!({}))]))
            .expect_err("a page listing itself")
            .to_string();
        assert!(error.contains("page page holds page page"), "{error}");
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
