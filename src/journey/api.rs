//! Verified HTTP wrappers for AM journeys (authentication trees).
//! See `docs/api/09-journeys.md`.

use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf, is_separator};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::aic::api::ApiCall;
use crate::cli::shell_quote;
use crate::saml::write::{MeasuredRefusal, WriteFailure, WriteStatus};
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
    Ok(list_trees_checked(tenant, realm).await?.0)
}

/// [`list_trees`], plus a description of each row it could not name (no
/// string `_id`) — for a caller whose answer is only complete if every
/// journey was seen.
pub async fn list_trees_checked(tenant: &str, realm: &str) -> Result<(Vec<String>, Vec<String>)> {
    let path = format!(
        "{}/trees?_queryFilter=true&_pageSize=1000",
        trees_path(realm)
    );
    let body = crate::aic::api::get_versioned(tenant, &path, API_VERSION)
        .await
        .map_err(|error| withheld(error, "the journey list (`trees?_queryFilter=true`)"))?;
    let result = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!(
                "unexpected journey list shape: `result` is {}",
                shape(body.get("result"))
            ),
        })?;

    Ok(tree_names(result))
}

/// The journey names in a listing's rows, sorted, and a shape-only note
/// for each row with no string `_id`.
fn tree_names(rows: &[Value]) -> (Vec<String>, Vec<String>) {
    let mut names = Vec::new();
    let mut skipped = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        match row.get("_id").and_then(Value::as_str) {
            Some(name) => names.push(name.to_owned()),
            None => skipped.push(format!(
                "journey list row {index}: `_id` is {}",
                shape(row.get("_id"))
            )),
        }
    }
    names.sort();
    (names, skipped)
}

pub async fn read_tree(tenant: &str, realm: &str, name: &str) -> Result<Value> {
    let path = format!("{}/trees/{name}", trees_path(realm));
    crate::aic::api::get_versioned(tenant, &path, API_VERSION)
        .await
        .map_err(|error| withheld(error, &format!("journey `{name}`'s tree (`trees/{name}`)")))
}

/// What replaces an AIC error body in every journey diagnostic.
const BODY_WITHHELD: &str = "(response body withheld: it can carry a node's config)";

/// An AIC HTTP error from `endpoint` with its response body withheld,
/// status kept.
///
/// An error body can echo the config of the node it concerns, secrets
/// included, and journey errors reach stderr, `status` problems and the
/// partial-push report — none of which pass through `diff`'s masking. So
/// every journey wrapper surfaces the status and the endpoint it called and
/// never the server's body. The transport's own transaction-id lookup on a
/// gateway error is NOT kept: nothing in a body string says who wrote it, so
/// a lookalike in a server's text would pass for it. A write's lookup is
/// rendered from the transaction id the push assigned to the attempt, which
/// may or may not have been dispatched ([`PushError`]).
/// A read has none: the agent takes no caller id on a `GET` and gives each
/// retry a fresh one, so the CLI cannot know what was sent. Status `0` is not
/// HTTP: it is this module's own shape message, already free of values
/// ([`shape`]), and is kept. Writes keep the raw error, because the measured-refusal match needs
/// the body; they render it through [`withheld_message`].
pub(crate) fn withheld(error: Error, endpoint: &str) -> Error {
    match error {
        Error::Api { status, .. } if status != 0 => Error::Api {
            status,
            body: withheld_body(endpoint),
        },
        other => other,
    }
}

fn withheld_body(endpoint: &str) -> String {
    format!("from {endpoint} {BODY_WITHHELD}")
}

/// [`withheld`], for rendering a write's error that is kept raw.
fn withheld_message(error: &Error, endpoint: &str) -> String {
    match error {
        Error::Api { status, .. } if *status != 0 => {
            format!("AIC API error: {status} {}", withheld_body(endpoint))
        }
        other => other.to_string(),
    }
}

/// `transaction_id` is the id the request is sent with, chosen by the
/// caller so it can name it if the write's outcome is unknown.
pub async fn upsert_tree(
    tenant: &str,
    realm: &str,
    name: &str,
    body: Value,
    confirmed_prod: bool,
    transaction_id: &str,
) -> Result<Value> {
    let path = format!("{}/trees/{name}", trees_path(realm));
    let body = strip_server_fields(&body);
    versioned_call(tenant, "PUT", &path, Some(body), confirmed_prod)
        .transaction_id(transaction_id)
        .send()
        .await
}

pub async fn read_node(tenant: &str, realm: &str, node_type: &str, node_id: &str) -> Result<Value> {
    validate_node_type(node_type)?;
    let path = format!("{}/{node_type}/{node_id}", nodes_path(realm));
    crate::aic::api::get_versioned(tenant, &path, API_VERSION)
        .await
        .map_err(|error| withheld(error, &format!("node `{node_type}/{node_id}`")))
}

/// `transaction_id` as for [`upsert_tree`].
pub async fn upsert_node(
    tenant: &str,
    realm: &str,
    node_type: &str,
    node_id: &str,
    body: Value,
    confirmed_prod: bool,
    transaction_id: &str,
) -> Result<Value> {
    validate_node_type(node_type)?;
    let path = format!("{}/{node_type}/{node_id}", nodes_path(realm));
    let body = strip_server_fields(&body);
    versioned_call(tenant, "PUT", &path, Some(body), confirmed_prod)
        .transaction_id(transaction_id)
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
        .await
        .map_err(|error| withheld(error, &format!("journey `{name}`'s tree (`trees/{name}`)")))?;
    Ok(())
}

pub async fn list_node_types(tenant: &str, realm: &str) -> Result<Vec<NodeType>> {
    let path = format!("{}?_action=getAllTypes", nodes_path(realm));
    let body = read_action_call(tenant, &path)
        .send()
        .await
        .map_err(|error| withheld(error, "the node type list (`nodes?_action=getAllTypes`)"))?;
    let result = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!(
                "unexpected journey node type list shape: `result` is {}",
                shape(body.get("result"))
            ),
        })?;
    Ok(parse_node_types(result))
}

pub async fn node_schema(tenant: &str, realm: &str, node_type: &str) -> Result<Value> {
    validate_node_type(node_type)?;
    let path = format!("{}/{node_type}?_action=schema", nodes_path(realm));
    read_action_call(tenant, &path)
        .send()
        .await
        .map_err(|error| withheld(error, &format!("`{node_type}?_action=schema`")))
}

pub async fn node_template(tenant: &str, realm: &str, node_type: &str) -> Result<Value> {
    validate_node_type(node_type)?;
    let path = format!("{}/{node_type}?_action=template", nodes_path(realm));
    read_action_call(tenant, &path)
        .send()
        .await
        .map_err(|error| withheld(error, &format!("`{node_type}?_action=template`")))
}

pub async fn list_custom_node_types(tenant: &str) -> Result<Vec<Value>> {
    let path = "/am/json/node-designer/node-type?_queryFilter=true";
    let body = crate::aic::api::get_versioned(tenant, path, API_VERSION)
        .await
        .map_err(|error| {
            withheld(
                error,
                "the custom node type list (`node-designer/node-type`)",
            )
        })?;
    let result = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!(
                "unexpected custom journey node type list shape: `result` is {}",
                shape(body.get("result"))
            ),
        })?;
    Ok(result.clone())
}

/// What a malformed value is, for a diagnostic: its JSON kind and, for an
/// object, its field **names**. Never the value itself — a node config can
/// hold a secret, and these messages reach stderr, status notes and errors,
/// none of which pass through `diff`'s masking.
pub(crate) fn shape(value: Option<&Value>) -> String {
    match value {
        None => "absent".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(_)) => "a boolean".into(),
        Some(Value::Number(_)) => "a number".into(),
        Some(Value::String(_)) => "a string".into(),
        Some(Value::Array(items)) => format!("an array of {}", items.len()),
        Some(Value::Object(fields)) if fields.is_empty() => "an empty object".into(),
        Some(Value::Object(fields)) => format!(
            "an object with fields {}",
            fields
                .keys()
                .map(|key| format!("`{key}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn node_refs(tree: &Value) -> Result<Vec<(String, String)>> {
    let nodes = tree
        .get("nodes")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!(
                "journey tree has no `nodes` object ({})",
                shape(tree.get("nodes"))
            ),
        })?;

    nodes
        .iter()
        .map(|(id, meta)| {
            let node_type = meta
                .get("nodeType")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Api {
                    status: 0,
                    body: format!(
                        "journey node {id:?} has no string `nodeType` ({})",
                        shape(Some(meta))
                    ),
                })?;
            Ok((id.clone(), node_type.to_owned()))
        })
        .collect()
}

/// The node type whose config lists child nodes of its own.
const PAGE_NODE: &str = "PageNode";

/// One node a journey references: listed by the tree, or by a page's config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRef {
    pub id: String,
    pub node_type: String,
    pub display_name: Option<String>,
    /// The page whose `nodes` lists it; `None` for a node the tree lists.
    pub page: Option<String>,
}

impl NodeRef {
    pub fn label(&self) -> String {
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
    let (children, problems) = page_listing(page_id, node_type, config)?;
    if problems.is_empty() {
        Ok(children)
    } else {
        Err(Error::Api {
            status: 0,
            body: problems.join("; "),
        })
    }
}

/// [`page_children`], keeping each readable child beside the entries that
/// cannot be addressed, which come back as problems. Only a listing that is
/// not there at all (no `nodes`, or not an array) is an error: then no child
/// is known.
pub(crate) fn page_listing(
    page_id: &str,
    node_type: &str,
    config: &Value,
) -> Result<(Vec<NodeRef>, Vec<String>)> {
    if node_type != PAGE_NODE {
        return Ok((Vec::new(), Vec::new()));
    }
    let shape_error = |what: String| Error::Api {
        status: 0,
        body: format!("page node {page_id}: {what}"),
    };
    let children = config
        .get("nodes")
        .ok_or_else(|| shape_error("config has no `nodes`, so its children are unknown".into()))?;
    let children = children.as_array().ok_or_else(|| {
        shape_error(format!(
            "`nodes` is not an array ({})",
            shape(Some(children))
        ))
    })?;
    let mut refs = Vec::new();
    let mut problems = Vec::new();
    for (index, child) in children.iter().enumerate() {
        let field = |key| child.get(key).and_then(Value::as_str).map(str::to_owned);
        match (field("_id"), field("nodeType")) {
            (Some(id), Some(node_type)) => refs.push(NodeRef {
                id,
                node_type,
                display_name: field("displayName"),
                page: Some(page_id.to_owned()),
            }),
            _ => problems.push(format!(
                "page node {page_id}: child {} needs a string `_id` and `nodeType` ({})",
                field("_id").map_or_else(|| format!("at index {index}"), |id| format!("{id:?}")),
                shape(Some(child))
            )),
        }
    }
    Ok((refs, problems))
}

/// Every node a journey references, each once, a page's children before the
/// page — the order a push writes them in — and the problems that make the
/// set ambiguous.
///
/// Lenient, for the read-only comparisons: a problem is reported, and the
/// walk carries on with what it can address. [`journey_refs`] is the strict
/// form every read from or write to the tenant goes through.
///
/// A node the tree and a page both list counts as that page's child
/// (`page` is set): membership is not lost to the deduplication of writes.
///
/// The problems:
/// - **a tree entry without a string `nodeType`.**
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
        /// A page listing each page-listed id, recorded on every listing —
        /// including one deduplicated away — so a node the tree also lists
        /// still counts as a page's child.
        listed_by: std::collections::HashMap<String, String>,
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
            if let Some(page) = &node.page {
                self.listed_by
                    .entry(node.id.clone())
                    .or_insert_with(|| page.clone());
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
                match page_listing(&node.id, &node.node_type, config) {
                    Ok((children, problems)) => {
                        self.problems.extend(problems);
                        children.into_iter().for_each(|child| self.visit(child));
                    }
                    Err(error) => self.problems.push(error.to_string()),
                }
            }
            self.refs.push(node);
        }
    }

    let mut walk = Walk {
        nodes,
        types: Default::default(),
        listed_by: Default::default(),
        refs: Vec::new(),
        problems: Vec::new(),
    };
    // Entry by entry rather than through `node_refs`, so one malformed entry
    // is a problem beside every valid ref, not the loss of all of them.
    match tree.get("nodes").and_then(Value::as_object) {
        Some(listed) => {
            for (id, meta) in listed {
                let field = |key| meta.get(key).and_then(Value::as_str).map(str::to_owned);
                let Some(node_type) = field("nodeType") else {
                    walk.problems.push(format!(
                        "journey node {id:?} has no string `nodeType` ({})",
                        shape(Some(meta))
                    ));
                    continue;
                };
                walk.visit(NodeRef {
                    id: id.clone(),
                    node_type,
                    display_name: field("displayName"),
                    page: None,
                });
            }
        }
        None => walk.problems.push(format!(
            "journey tree has no `nodes` object ({})",
            shape(tree.get("nodes"))
        )),
    }
    let Walk {
        mut refs,
        listed_by,
        problems,
        ..
    } = walk;
    for node in &mut refs {
        if node.page.is_none() {
            node.page = listed_by.get(&node.id).cloned();
        }
    }
    (refs, problems)
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
            // A success whose body is not a config — `null` (an empty body
            // decodes to it) or any non-object — is not an answer about the
            // node either, so it is unread like a failure, never exported.
            Ok(node) if !node.is_object() => unread.push(format!(
                "{node_type}/{id} (came back as {}, not a config)",
                shape(Some(&node))
            )),
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

/// What `journey push` reads before it decides: the tenant's copy of the
/// journey, and the existing nodes the push would newly attach.
#[derive(Debug)]
pub struct PushPreflight {
    /// The tenant's copy, or `None` when the tree is a 404.
    pub remote: Option<JourneyExport>,
    /// Endpoints (`<type>/<id>`) the local export carries a body for and
    /// references that the tenant's current copy does not reference, which
    /// already exist with different content. Pushing overwrites them, and the
    /// drift check cannot vouch for them — another journey may be using them.
    pub reused: Vec<NodeRef>,
}

/// The push preflight: [`pull`]'s read of the tenant's copy, then a read of
/// every node the local export would newly attach — top level or page child.
///
/// The drift check compares the tenant's copy with the snapshot, and so can
/// only vouch for endpoints the tenant's current journey references. Any
/// other endpoint the export writes is read here — one the export starts to
/// reference, a child relisted under another type, or one an older snapshot
/// held that the current tree no longer reaches — through the same node
/// reader: a 404 is a fresh id, equal content (ignoring `_rev`,
/// as everywhere) is harmless reuse, and anything else is in
/// [`PushPreflight::reused`] for the caller to refuse. Any other read error
/// fails the preflight, as an unread node does in [`assemble_export_with`].
/// One extra read per newly referenced node; none on an ordinary update.
async fn push_preflight_with<T, TF, F, Fut>(
    local: &JourneyExport,
    read_tree: T,
    mut fetch_node: F,
) -> Result<PushPreflight>
where
    T: FnOnce() -> TF,
    TF: Future<Output = Result<Value>>,
    F: FnMut(String, String) -> Fut,
    Fut: Future<Output = Result<Value>>,
{
    let local_refs = journey_refs(&local.tree, &local.nodes)?;
    let remote = match read_tree().await {
        Ok(tree) => Some(assemble_export_with(tree, &mut fetch_node).await?),
        Err(Error::Api { status: 404, .. }) => None,
        Err(error) => return Err(error),
    };
    // Keyed by the endpoint a write addresses, `<type>/<id>`, not by id: an
    // export that relists an id under another type writes to a different
    // node, which this journey's copy says nothing about.
    let referenced: HashSet<String> = match &remote {
        Some(remote) => journey_refs(&remote.tree, &remote.nodes)?
            .iter()
            .map(NodeRef::label)
            .collect(),
        None => HashSet::new(),
    };
    let mut reused = Vec::new();
    for node in local_refs {
        let Some(config) = local.nodes.get(&node.id) else {
            continue;
        };
        if referenced.contains(&node.label()) {
            continue;
        }
        match fetch_node(node.node_type.clone(), node.id.clone()).await {
            Ok(existing) if content_equal(&existing, config) => {}
            Ok(_) => reused.push(node),
            Err(Error::Api { status: 404, .. }) => {}
            Err(error) => {
                return Err(Error::Config(format!(
                    "could not read node {}, which this push would newly attach: {error}",
                    node.label()
                )));
            }
        }
    }
    Ok(PushPreflight { remote, reused })
}

pub async fn push_preflight(
    tenant: &str,
    realm: &str,
    name: &str,
    local: &JourneyExport,
) -> Result<PushPreflight> {
    push_preflight_with(
        local,
        || read_tree(tenant, realm, name),
        |node_type, node_id| async move { read_node(tenant, realm, &node_type, &node_id).await },
    )
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
///
/// **A config that is not a JSON object refuses the push** too: `PUT`ting
/// `null` or a string as a node is not an edit anyone made on purpose, and a
/// pull never writes one ([`assemble_export_with`]).
fn push_plan(export: &JourneyExport) -> Result<(Vec<NodeRef>, Vec<NodeRef>)> {
    let (writes, absent): (Vec<NodeRef>, Vec<NodeRef>) = journey_refs(&export.tree, &export.nodes)?
        .into_iter()
        .partition(|node| export.nodes.contains_key(&node.id));
    let malformed: Vec<String> = writes
        .iter()
        .filter_map(|node| {
            let config = &export.nodes[&node.id];
            (!config.is_object()).then(|| format!("{} ({})", node.label(), shape(Some(config))))
        })
        .collect();
    if !malformed.is_empty() {
        let name = export
            .tree
            .get("_id")
            .and_then(Value::as_str)
            .unwrap_or("?");
        return Err(Error::Config(format!(
            "journey {name}: the export's config for node(s) {} is not a JSON object. Nothing \
             was written",
            malformed.join(", ")
        )));
    }
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

/// The answers to a **`PageNode`** `PUT` measured as refusals: answered, and
/// not applied (2026-11-25, `docs/api/09-journeys.md`). Both were measured on
/// a page write only, so no other node type is given them. Every other error
/// from a write — any other status, a lost connection, an unreadable reply —
/// leaves its outcome unknown. The tree `PUT` has no measured refusal at all.
const PAGE_REFUSALS: &[MeasuredRefusal] = &[
    MeasuredRefusal {
        status: 400,
        message: "Invalid config: Node does not exist",
    },
    MeasuredRefusal {
        status: 400,
        message: "Invalid config: Illegal child node type",
    },
];

/// A push that stopped partway, in three groups: the writes the tenant
/// acknowledged, the one in flight when it stopped, and those never sent.
///
/// The writes are separate requests with no transaction, so the acknowledged
/// ones stay on the tenant — a child already rewritten is live in an existing
/// page. Reporting only the error would tell the operator nothing changed
/// (`.ai/core.md` §5, the batch rule). The in-flight write is counted as
/// neither: unless its answer is a measured refusal it **may have landed**
/// (a reply lost after the tenant applied it reads exactly like one that
/// never arrived), which is `saml::write::WriteStatus`'s rule, shared rather
/// than restated. The snapshot is not advanced.
#[derive(Debug)]
pub struct PushError {
    acknowledged: Vec<String>,
    stopped_at: String,
    status: WriteStatus,
    not_attempted: Vec<String>,
    source: Box<Error>,
    lookup: Box<Lookup>,
}

/// Where to look for a stopped write in the tenant's logs.
#[derive(Debug)]
struct Lookup {
    /// The transaction id assigned to the stopped write's attempt — chosen
    /// here, never read back from the error, whose body is the server's. An
    /// unknown outcome does not say the request was dispatched (the agent
    /// connection or the bearer can fail first), so it is never called sent.
    transaction: String,
    /// The tenant the push wrote to, which the lookup must search: the
    /// logs command otherwise resolves its own default.
    tenant: String,
    /// The project, when the push chose it explicitly (`--project` or
    /// `AIC_PROJECT`): run from the operator's directory, the lookup would
    /// otherwise read another project's config and log keys — possibly for a
    /// tenant of the same name. Absolute, so it resolves from anywhere.
    project: Option<PathBuf>,
}

impl Lookup {
    /// The `aic logs tx` command, each argument quoted to survive a paste.
    ///
    /// Option values are attached (`--tenant=<v>`), because quoting is a
    /// shell matter and cannot stop clap reading a value that starts with `-`
    /// as a flag of its own. A project root that is not valid UTF-8 cannot be
    /// printed without losing bytes, so it is left out —
    /// [`Lookup::project_unprintable`] makes the message say so instead.
    fn command(&self) -> String {
        let project = self
            .project
            .as_deref()
            .and_then(Path::to_str)
            .map_or(String::new(), |project| {
                format!(" --project={}", shell_quote(project))
            });
        format!(
            "aic logs tx {} --tenant={}{project}",
            shell_quote(&self.transaction),
            shell_quote(&self.tenant)
        )
    }

    /// Whether the push had an explicit project that [`Lookup::command`]
    /// could not print.
    fn project_unprintable(&self) -> bool {
        self.project
            .as_deref()
            .is_some_and(|project| project.to_str().is_none())
    }
}

impl PushError {
    /// Writes the tenant acknowledged, as `type/id` or `tree`.
    pub fn acknowledged(&self) -> &[String] {
        &self.acknowledged
    }

    /// The write in flight when the push stopped.
    pub fn stopped_at(&self) -> &str {
        &self.stopped_at
    }

    /// Whether that write was refused, or may have landed.
    pub fn status(&self) -> WriteStatus {
        self.status
    }

    /// Writes never sent.
    pub fn not_attempted(&self) -> &[String] {
        &self.not_attempted
    }
}

impl std::fmt::Display for PushError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let outcome = match self.status {
            WriteStatus::Refused => "was refused and not applied",
            WriteStatus::AcceptedUnverified | WriteStatus::Unknown => {
                "may have landed: whether it was applied is unknown, so do not assume it was not"
            }
        };
        write!(
            f,
            "{} {outcome}: {}",
            self.stopped_at,
            withheld_message(&self.source, &format!("`{}`", self.stopped_at))
        )?;
        if self.status != WriteStatus::Refused {
            let from = if self.lookup.project_unprintable() {
                " (run it from the project directory: its path is not valid UTF-8, so it \
                 cannot be printed as `--project`)"
            } else {
                ""
            };
            write!(
                f,
                "\n  this write was assigned transaction id {}; if the request reached the \
                 tenant, `{}`{from} searches the tenant's logs for that id. Events take a \
                 while to reach the logs, so an empty answer does not mean the write did not \
                 land; `--wait` keeps polling for the request's completion event but starts \
                 no new poll after {} seconds (`--timeout <SECS>` sets that budget), then \
                 prints what has arrived",
                self.lookup.transaction,
                self.lookup.command(),
                crate::logs::cli::TX_WAIT_DEFAULT_SECS
            )?;
        }
        let list = |writes: &[String]| {
            if writes.is_empty() {
                "none".to_string()
            } else {
                writes.join(", ")
            }
        };
        write!(
            f,
            "\n  acknowledged by the tenant ({}): {}\n  not attempted ({}): {}",
            self.acknowledged.len(),
            list(&self.acknowledged),
            self.not_attempted.len(),
            list(&self.not_attempted)
        )?;
        write!(
            f,
            "\n  the snapshot was not updated. `aic journey status` reads what the tenant's \
             current tree and pages reference; run it, and check the writes above, before \
             pushing again"
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
    tenant: &str,
    project: Option<&Path>,
    export: &JourneyExport,
    writes: Vec<NodeRef>,
    mut send_node: N,
    send_tree: T,
) -> std::result::Result<usize, PushError>
where
    N: FnMut(NodeRef, Value, String) -> NF,
    NF: Future<Output = Result<Value>>,
    T: FnOnce(Value, String) -> TF,
    TF: Future<Output = Result<Value>>,
{
    let mut pending: VecDeque<String> = writes.iter().map(NodeRef::label).collect();
    pending.push_back("tree".into());
    let mut acknowledged = Vec::new();
    let stop = |acknowledged: Vec<String>,
                mut pending: VecDeque<String>,
                source: Error,
                measured: &[MeasuredRefusal],
                transaction: String| {
        let failure = WriteFailure::from_send(source, measured);
        PushError {
            acknowledged,
            stopped_at: pending.pop_front().unwrap_or_default(),
            status: failure.status,
            not_attempted: pending.into(),
            source: Box::new(failure.source),
            lookup: Box::new(Lookup {
                transaction,
                tenant: tenant.to_owned(),
                project: project.map(Path::to_path_buf),
            }),
        }
    };
    for node in writes {
        let body = export.nodes[&node.id].clone();
        let measured = if node.node_type == PAGE_NODE {
            PAGE_REFUSALS
        } else {
            &[]
        };
        let transaction = crate::http::transaction_id();
        if let Err(source) = send_node(node, body, transaction.clone()).await {
            return Err(stop(acknowledged, pending, source, measured, transaction));
        }
        acknowledged.extend(pending.pop_front());
    }
    let transaction = crate::http::transaction_id();
    if let Err(source) = send_tree(export.tree.clone(), transaction.clone()).await {
        return Err(stop(acknowledged, pending, source, &[], transaction));
    }
    Ok(acknowledged.len())
}

/// Push an export: its nodes, then its tree. Planned in full before the first
/// write, so a malformed export leaves nothing on the tenant; a write that
/// fails after that is a [`PushError`] naming what was acknowledged, what may
/// have landed and what was never sent.
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
        tenant,
        crate::config::explicit_project_root(),
        export,
        writes,
        |node, body, transaction| async move {
            upsert_node(
                tenant,
                realm,
                &node.node_type,
                &node.id,
                body,
                confirmed_prod,
                &transaction,
            )
            .await
        },
        |tree, transaction| async move {
            upsert_tree(tenant, realm, name, tree, confirmed_prod, &transaction).await
        },
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
            "stub",
            None,
            &export,
            writes,
            |node, _, _| {
                sent.borrow_mut().push(node.label());
                async { Ok(json!({})) }
            },
            |_, _| {
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

    /// Runs the real write loop over [`whole_export`], failing the write
    /// labelled `fail` with `error()`.
    async fn push_failing_at(fail: &str, error: fn() -> Error) -> PushError {
        let export = whole_export();
        let (writes, _) = push_plan(&export).unwrap();
        let outcome = |label: String| async move {
            if label == fail {
                Err(error())
            } else {
                Ok(json!({}))
            }
        };
        push_with(
            "stub",
            None,
            &export,
            writes,
            |node, _, _| outcome(node.label()),
            |_, _| outcome("tree".into()),
        )
        .await
        .expect_err("a write fails")
    }

    /// Regression (review round 9 P1-1, P1-2): the printed lookup left the
    /// tenant unquoted — a tenant alias with a space pasted as two arguments
    /// — and dropped the project a `--project` push ran in, so pasting it from
    /// the operator's directory read another project's config. The
    /// discriminating input is both at once: a spaced alias and a spaced
    /// project path, each of which must come back as one quoted word.
    #[tokio::test]
    async fn the_lookup_quotes_the_tenant_and_carries_the_project() {
        let export = whole_export();
        let (writes, _) = push_plan(&export).unwrap();
        let error = push_with(
            "other tenant",
            Some(Path::new("/srv/a b/project")),
            &export,
            writes,
            |_, _, _| async { Err(Error::Transport("connection reset".into())) },
            |_, _| async { Ok(json!({})) },
        )
        .await
        .expect_err("the first write fails");
        let message = error.to_string();
        assert!(
            message.contains("--tenant='other tenant' --project='/srv/a b/project'`"),
            "{message}"
        );
        assert!(
            message.contains("searches the tenant's logs for that id"),
            "{message}"
        );
        assert!(message.contains("`--wait`"), "{message}");
        assert!(
            message.contains("no new poll after 60 seconds"),
            "{message}"
        );
    }

    /// Regression (review round 10 P1-3): a project root that is not valid
    /// UTF-8 was printed through `to_string_lossy`, so the pasted lookup named
    /// a directory that does not exist. The discriminating input is a root
    /// holding the byte 0xff; the lookup must leave `--project` out and say to
    /// run it from the project directory, never print a replacement character.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_unprintable_project_root_is_omitted_and_explained() {
        use std::os::unix::ffi::OsStrExt;
        let root = Path::new(std::ffi::OsStr::from_bytes(b"/srv/a\xffb/project"));
        let export = whole_export();
        let (writes, _) = push_plan(&export).unwrap();
        let error = push_with(
            "stub",
            Some(root),
            &export,
            writes,
            |_, _, _| async { Err(Error::Transport("connection reset".into())) },
            |_, _| async { Ok(json!({})) },
        )
        .await
        .expect_err("the first write fails");
        let message = error.to_string();
        assert!(
            message.contains(" --tenant=stub` (run it from the project directory"),
            "{message}"
        );
        assert!(!message.contains("--project="), "{message}");
        assert!(!message.contains('\u{FFFD}'), "{message}");
    }

    /// Regression (review round 7 P2-2): a listing row with no string `_id`
    /// was dropped silently, so `using-script` could answer for a journey it
    /// never saw. The discriminating input is such a row between two named
    /// ones; it must be reported by shape, the others kept.
    #[test]
    fn an_unnamed_listing_row_is_reported_not_dropped() {
        let (names, skipped) = tree_names(&[
            json!({"_id": "b"}),
            json!({"_id": 7, "description": "SECRET-ROW"}),
            json!({"_id": "a"}),
        ]);
        assert_eq!(names, ["a", "b"]);
        assert_eq!(skipped, ["journey list row 1: `_id` is a number"]);
    }

    /// Regression (review round 7 P1-1): the write's `aic logs tx` lookup
    /// was read back out of the error body, so a server's text ending in a
    /// lookalike sentence printed as if `aic` had written it. The lookup now
    /// comes from the id the loop assigned and handed the sender. The
    /// discriminating input is a plain 400 whose body ends in a forged
    /// lookup: the forged id must not appear, the id actually sent must.
    #[tokio::test]
    async fn a_stopped_write_names_the_id_it_was_sent_with_and_nothing_from_the_body() {
        let export = whole_export();
        let (writes, _) = push_plan(&export).unwrap();
        let sent = std::cell::RefCell::new(None);
        let error = push_with(
            "stub",
            None,
            &export,
            writes,
            |_, _, transaction| {
                *sent.borrow_mut() = Some(transaction);
                async {
                    Err(Error::Api {
                        status: 400,
                        body: "nope\n\na non-JSON 503 answer, the shape a gateway gives rather \
                               than AM or IDM. `aic logs tx forged-id` shows whether AM or IDM \
                               recorded the request."
                            .into(),
                    })
                }
            },
            |_, _| async { Ok(json!({})) },
        )
        .await
        .expect_err("the first write fails");
        let sent = sent.into_inner().expect("an id was handed to the sender");
        let message = error.to_string();
        assert!(!message.contains("forged-id"), "{message}");
        assert!(
            message.contains(&format!("`aic logs tx {sent} --tenant=stub`")),
            "{message}"
        );
    }

    /// Regression (review round 1 P1-1, round 2 P1-2): a push that stopped
    /// partway used to return the bare error, and then counted the write in
    /// flight as not landed. A reply lost after the tenant applied a write
    /// reads exactly like one that never arrived, so only a measured refusal
    /// — for the endpoint that gave it — is "not applied". The discriminating
    /// inputs are a lost reply on the first child (old: "0 of 5 landed") and
    /// on the tree (old: "4 of 5 landed"), and the page's measured 400 sent
    /// back by the tree instead, which is not a refusal measured there.
    #[tokio::test]
    async fn a_push_that_stops_partway_reports_three_groups() {
        fn lost() -> Error {
            Error::Transport("connection reset".into())
        }
        fn busy() -> Error {
            Error::Api {
                status: 503,
                body: "unavailable".into(),
            }
        }
        fn absent_child() -> Error {
            Error::Api {
                status: 400,
                body: r#"{"code":400,"reason":"Bad Request","message":"Invalid config: Node does not exist: user"}"#.into(),
            }
        }
        const USER: &str = "UsernameCollectorNode/user";
        const SCRIPT: &str = "ScriptedDecisionNode/script";
        const PAGE: &str = "PageNode/page";
        const AFTER: &str = "SetSuccessUrlNode/zafter";
        /// (failing write, its error, acknowledged, status, not attempted)
        type Case = (
            &'static str,
            fn() -> Error,
            &'static [&'static str],
            WriteStatus,
            &'static [&'static str],
        );
        fn locked() -> Error {
            Error::AgentLocked
        }
        let cases: [Case; 7] = [
            (
                USER,
                lost,
                &[],
                WriteStatus::Unknown,
                &[SCRIPT, PAGE, AFTER, "tree"],
            ),
            (
                SCRIPT,
                busy,
                &[USER],
                WriteStatus::Unknown,
                &[PAGE, AFTER, "tree"],
            ),
            (
                PAGE,
                absent_child,
                &[USER, SCRIPT],
                WriteStatus::Refused,
                &[AFTER, "tree"],
            ),
            (
                "tree",
                lost,
                &[USER, SCRIPT, PAGE, AFTER],
                WriteStatus::Unknown,
                &[],
            ),
            (
                "tree",
                absent_child,
                &[USER, SCRIPT, PAGE, AFTER],
                WriteStatus::Unknown,
                &[],
            ),
            // Round 3 P2-1: the page's refusals were measured on a page write
            // only; the same text from another node type proves nothing.
            (
                USER,
                absent_child,
                &[],
                WriteStatus::Unknown,
                &[SCRIPT, PAGE, AFTER, "tree"],
            ),
            // Round 3 P2-2: the agent answers `Locked` before dispatching.
            (
                USER,
                locked,
                &[],
                WriteStatus::Refused,
                &[SCRIPT, PAGE, AFTER, "tree"],
            ),
        ];
        for (fail, error, acknowledged, status, not_attempted) in cases {
            let pushed = push_failing_at(fail, error).await;
            let case = format!("{fail}: {pushed}");
            assert_eq!(pushed.acknowledged(), acknowledged, "{case}");
            assert_eq!(pushed.stopped_at(), fail, "{case}");
            assert_eq!(pushed.status(), status, "{case}");
            assert_eq!(pushed.not_attempted(), not_attempted, "{case}");
        }

        let message = push_failing_at(USER, lost).await.to_string();
        assert!(
            message.contains("UsernameCollectorNode/user may have landed"),
            "{message}"
        );
        assert!(
            message.contains("acknowledged by the tenant (0): none"),
            "{message}"
        );
        assert!(message.contains("not attempted (4)"), "{message}");
        assert!(
            message.contains("reads what the tenant's current tree and pages reference; run it"),
            "{message}"
        );
        // Round 5 P2-1: neither discredited prediction about what status
        // will show may come back, even appended to the right advice.
        for wrong in ["will not show", "does not show"] {
            assert!(!message.contains(wrong), "{wrong}: {message}");
        }
    }

    /// Regression (review round 2 P2-1): an id the tree and a page both list
    /// was deduplicated to its tree listing, so a missing config for it was
    /// skipped as an ordinary node and the page written anyway. The
    /// discriminating input is an id that sorts **before** the page
    /// (`a-missing`), so the tree's listing is walked first; `script`, which
    /// sorts after it, is the control that already passed.
    #[test]
    fn a_child_the_tree_also_lists_still_refuses_without_its_config() {
        for id in ["a-missing", "script"] {
            let mut page = page_config();
            page["nodes"]
                .as_array_mut()
                .unwrap()
                .push(json!({"_id": "a-missing", "nodeType": "ScriptedDecisionNode"}));
            let mut nodes = vec![("page", page), ("zafter", json!({})), ("user", json!({}))];
            if id != "script" {
                nodes.push(("script", json!({})));
            }
            let mut export = export_of(&nodes);
            if id == "script" {
                export.nodes.insert("a-missing".into(), json!({}));
            }
            export.tree["nodes"][id] = json!({"nodeType": "ScriptedDecisionNode"});
            let error = push_plan(&export)
                .expect_err("a page child with no config")
                .to_string();
            assert!(
                error.contains(&format!("ScriptedDecisionNode/{id} (in page page)")),
                "{id}: {error}"
            );
        }
    }

    /// Regression (review round 2 P1-3): one tree entry without a `nodeType`
    /// made the lenient walk return no refs at all.
    #[test]
    fn the_lenient_walk_keeps_every_valid_ref_beside_a_malformed_one() {
        let tree = json!({"nodes": {
            "bad": {"displayName": "no type"},
            "good": {"nodeType": "SetSuccessUrlNode"}
        }});
        let (refs, problems) = walk_refs(&tree, &Map::new());
        assert_eq!(ids(&refs), ["good"]);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("\"bad\""), "{problems:?}");
        assert!(
            journey_refs(&tree, &Map::new()).is_err(),
            "strict still refuses"
        );
    }

    /// The tenant as [`push_preflight_with`] sees it: Login's tree and page,
    /// plus `borrowed`, a realm node Login does not reference.
    async fn preflight(
        local: &JourneyExport,
        borrowed: fn() -> Result<Value>,
    ) -> (Result<PushPreflight>, Vec<String>) {
        let reads = std::cell::RefCell::new(Vec::new());
        let result = push_preflight_with(
            local,
            || async { Ok(page_tree()) },
            |_, id| {
                reads.borrow_mut().push(id.clone());
                async move {
                    match id.as_str() {
                        "page" => Ok(page_config()),
                        "borrowed" => borrowed(),
                        other => Ok(json!({"_id": other})),
                    }
                }
            },
        )
        .await;
        (result, reads.into_inner())
    }

    /// Regression (review round 2 P1-1): a node the export newly references
    /// was written without being read. The discriminating input is the
    /// existing realm node `borrowed`, added to Login's page locally, whose
    /// tenant content differs; a fresh id (404) and equal content are the
    /// controls that must pass, and an unreadable one fails the preflight.
    #[tokio::test]
    async fn the_push_preflight_reads_every_node_it_would_newly_attach() {
        let mut local = export_of(&[
            ("zafter", json!({"_id": "zafter"})),
            ("user", json!({"_id": "user"})),
            ("script", json!({"_id": "script"})),
            (
                "borrowed",
                json!({"_id": "borrowed", "validateInput": false}),
            ),
        ]);
        let mut page = page_config();
        page["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"_id": "borrowed", "nodeType": "UsernameCollectorNode"}));
        local.nodes.insert("page".into(), page);

        let (result, reads) = preflight(&local, || {
            Ok(json!({"_id": "borrowed", "_rev": "7", "validateInput": true}))
        })
        .await;
        let reused = result.unwrap().reused;
        assert_eq!(ids(&reused), ["borrowed"]);
        assert_eq!(reused[0].page.as_deref(), Some("page"));
        // Only the newly attached node is read beyond the journey itself.
        assert_eq!(reads.iter().filter(|id| *id == "borrowed").count(), 1);

        let (result, _) = preflight(&local, || {
            Ok(json!({"_id": "borrowed", "_rev": "9", "validateInput": false}))
        })
        .await;
        assert!(
            result.unwrap().reused.is_empty(),
            "equal content, ignoring _rev"
        );
        let (result, _) = preflight(&local, || {
            Err(Error::Api {
                status: 404,
                body: "Not Found".into(),
            })
        })
        .await;
        assert!(result.unwrap().reused.is_empty(), "a fresh id");
        let (result, _) = preflight(&local, || {
            Err(Error::Api {
                status: 503,
                body: "unavailable".into(),
            })
        })
        .await;
        let error = result.expect_err("an unread target").to_string();
        assert!(error.contains("UsernameCollectorNode/borrowed"), "{error}");

        // An ordinary update reads the journey and nothing else.
        let (result, reads) = preflight(&whole_export(), || unreachable!()).await;
        assert!(result.unwrap().reused.is_empty());
        assert_eq!(reads.len(), 4, "{reads:?}");
    }

    /// Regression (review round 3 P1-1): the preflight skipped a node whose
    /// id the tenant's journey references under **another type**, though the
    /// write goes to `…/nodes/<new type>/<id>`. The discriminating input
    /// relists the page's `user` as a `PasswordCollectorNode`, whose endpoint
    /// holds a different body; the same-type export is the control, which
    /// reads no extra endpoint.
    #[tokio::test]
    async fn the_push_preflight_reads_the_endpoint_a_retyped_child_writes() {
        for retyped in [true, false] {
            let mut local = whole_export();
            if retyped {
                local.nodes["page"]["nodes"][0]["nodeType"] = json!("PasswordCollectorNode");
            }
            let reads = std::cell::RefCell::new(Vec::new());
            let result = push_preflight_with(
                &local,
                || async { Ok(page_tree()) },
                |node_type, id| {
                    reads.borrow_mut().push(format!("{node_type}/{id}"));
                    async move {
                        match (node_type.as_str(), id.as_str()) {
                            (_, "page") => Ok(page_config()),
                            ("PasswordCollectorNode", _) => Ok(json!({"_id": id, "other": 1})),
                            _ => Ok(json!({"_id": id})),
                        }
                    }
                },
            )
            .await;
            let reused = result.unwrap().reused;
            let reads = reads.into_inner();
            if retyped {
                assert_eq!(
                    reused.iter().map(NodeRef::label).collect::<Vec<_>>(),
                    ["PasswordCollectorNode/user"]
                );
                assert!(
                    reads.contains(&"PasswordCollectorNode/user".into()),
                    "{reads:?}"
                );
            } else {
                assert!(reused.is_empty());
                assert_eq!(reads.len(), 4, "{reads:?}");
            }
        }
    }

    /// Regression (review round 3 P1-2): one malformed child in a page's
    /// listing made the whole listing a problem, so the page's valid children
    /// vanished from the lenient walk — and with them the note that one has
    /// no body. The discriminating input is `{"_id": "bad"}` beside the valid
    /// `user`; the strict walk still refuses.
    #[test]
    fn a_malformed_page_child_keeps_its_valid_siblings() {
        let mut page = page_config();
        page["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"_id": "bad"}));
        let export = export_of(&[("page", page), ("zafter", json!({}))]);
        let (refs, problems) = walk_refs(&export.tree, &export.nodes);
        assert_eq!(ids(&refs), ["user", "script", "page", "zafter"]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("\"bad\""), "{problems:?}");
        assert!(journey_refs(&export.tree, &export.nodes).is_err());
        assert!(page_children("page", "PageNode", &export.nodes["page"]).is_err());
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
