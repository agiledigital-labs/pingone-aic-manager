//! Verified HTTP wrappers for AM journeys (authentication trees).
//! See `docs/api/09-journeys.md`.

use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::path::is_separator;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::aic::api::ApiCall;
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
                        "journey node {id:?} has no string `nodeType`: {meta}"
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
        None => walk
            .problems
            .push(format!("journey tree has no `nodes` object: {tree}")),
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
    /// Nodes the local export carries and references that the tenant's copy
    /// does not reference, which already exist on the tenant with different
    /// content. Pushing overwrites them, and nothing in this journey's
    /// snapshot is a baseline for them — another journey may be using them.
    pub reused: Vec<NodeRef>,
}

/// The push preflight: [`pull`]'s read of the tenant's copy, then a read of
/// every node the local export would newly attach — top level or page child.
///
/// The drift check compares the tenant's copy with the snapshot, and so can
/// only see nodes the journey already references. An existing realm node the
/// export starts to reference is outside both, so it is read here, through
/// the same node reader: a 404 is a fresh id, equal content (ignoring `_rev`,
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
    let referenced: HashSet<String> = match &remote {
        Some(remote) => journey_refs(&remote.tree, &remote.nodes)?
            .into_iter()
            .map(|node| node.id)
            .collect(),
        None => HashSet::new(),
    };
    let mut reused = Vec::new();
    for node in local_refs {
        let Some(config) = local.nodes.get(&node.id) else {
            continue;
        };
        if referenced.contains(&node.id) {
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

/// The answers to a node `PUT` measured as refusals: answered, and not
/// applied (2026-11-25, `docs/api/09-journeys.md`). Every other error from a
/// write — any other status, a lost connection, an unreadable reply — leaves
/// its outcome unknown. The tree `PUT` has no measured refusal at all.
const NODE_REFUSALS: &[MeasuredRefusal] = &[
    MeasuredRefusal {
        status: 400,
        message: "Node does not exist",
    },
    MeasuredRefusal {
        status: 400,
        message: "Illegal child node type",
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
        write!(f, "{} {outcome}: {}", self.stopped_at, self.source)?;
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
            "\n  the snapshot was not updated. `aic journey status` reads only the nodes the \
             tenant's tree reaches, so a node written by a create that stopped here, or newly \
             attached by this push, will not show there — check the writes above before \
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
    let mut pending: VecDeque<String> = writes.iter().map(NodeRef::label).collect();
    pending.push_back("tree".into());
    let mut acknowledged = Vec::new();
    let stop = |acknowledged: Vec<String>,
                mut pending: VecDeque<String>,
                source: Error,
                measured: &[MeasuredRefusal]| {
        let failure = WriteFailure::from_send(source, measured);
        PushError {
            acknowledged,
            stopped_at: pending.pop_front().unwrap_or_default(),
            status: failure.status,
            not_attempted: pending.into(),
            source: Box::new(failure.source),
        }
    };
    for node in writes {
        let body = export.nodes[&node.id].clone();
        if let Err(source) = send_node(node, body).await {
            return Err(stop(acknowledged, pending, source, NODE_REFUSALS));
        }
        acknowledged.extend(pending.pop_front());
    }
    if let Err(source) = send_tree(export.tree.clone()).await {
        return Err(stop(acknowledged, pending, source, &[]));
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
            &export,
            writes,
            |node, _| outcome(node.label()),
            |_| outcome("tree".into()),
        )
        .await
        .expect_err("a write fails")
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
                body: "Invalid config: Node does not exist: user".into(),
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
        let cases: [Case; 5] = [
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
            message.contains("reads only the nodes the tenant's tree reaches"),
            "{message}"
        );
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
