//! `aic journey status` / `aic journey diff`: where a journey's three copies —
//! the workspace export, the last-synced snapshot, the tenant — disagree.
//!
//! Read-only by construction: nothing here writes, and the CLI arms that call
//! it never refresh the snapshot, so asking the question cannot change the
//! answer the next `push` gets. Tenant-free; `cli.rs` supplies the copies.
//!
//! Each copy is an export value as `cli::export_value` builds it,
//! `{"tree": …, "nodes": {id: config}}`, compared with `api::content_equal`,
//! which ignores `_rev` everywhere — the same comparison `push` decides on.

use super::api::{NodeRef, content_equal, walk_refs};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// The state of one journey, named as `aic script status` names a script's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    InSync,
    LocallyModified,
    RemotelyModified,
    /// Local and remote both moved away from the snapshot.
    Conflict,
    /// Local and remote moved to the same content; only the snapshot is stale.
    Converged,
    /// No snapshot, so no baseline. `matches_remote` says whether the local
    /// export already equals the tenant's, which is what a push asks.
    NoSnapshot {
        matches_remote: bool,
    },
    /// The tenant has it; the workspace has no export.
    NotPulled,
    /// The workspace has it; the tenant does not (a 404 on the tree).
    RemoteMissing,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            State::InSync => "in sync",
            State::LocallyModified => "modified locally",
            State::RemotelyModified => "modified on remote",
            State::Conflict => "conflict",
            State::Converged => "converged",
            State::NoSnapshot {
                matches_remote: true,
            } => "no snapshot (local matches remote)",
            State::NoSnapshot {
                matches_remote: false,
            } => "no snapshot (local differs from remote)",
            State::NotPulled => "not pulled",
            State::RemoteMissing => "not on the tenant",
        }
    }

    /// Whether the workspace and the tenant hold the same journey.
    pub fn matches_tenant(self) -> bool {
        matches!(
            self,
            State::InSync
                | State::Converged
                | State::NoSnapshot {
                    matches_remote: true
                }
        )
    }
}

pub fn state(local: Option<&Value>, snapshot: Option<&Value>, remote: Option<&Value>) -> State {
    let (local, remote) = match (local, remote) {
        (None, _) => return State::NotPulled,
        (Some(_), None) => return State::RemoteMissing,
        (Some(local), Some(remote)) => (local, remote),
    };
    let Some(snapshot) = snapshot else {
        return State::NoSnapshot {
            matches_remote: content_equal(local, remote),
        };
    };
    match (
        content_equal(local, snapshot),
        content_equal(remote, snapshot),
    ) {
        (true, true) => State::InSync,
        (false, true) => State::LocallyModified,
        (true, false) => State::RemotelyModified,
        (false, false) if content_equal(local, remote) => State::Converged,
        (false, false) => State::Conflict,
    }
}

/// One piece of an export that differs between two copies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "part", rename_all = "snake_case")]
pub enum Part {
    Tree,
    Node {
        id: String,
        /// From whichever copy lists it: its tree, or a page's config.
        node_type: Option<String>,
        display_name: Option<String>,
        /// The page listing it, for a page's child.
        page: Option<String>,
    },
}

impl Part {
    pub fn label(&self) -> String {
        match self {
            Part::Tree => "tree".into(),
            Part::Node {
                id,
                node_type,
                display_name,
                page,
            } => {
                let kind = node_type.as_deref().unwrap_or("node");
                let name = display_name
                    .as_ref()
                    .map(|name| format!(" ({name})"))
                    .unwrap_or_default();
                let page = page
                    .as_ref()
                    .map(|page| format!(" in page {page}"))
                    .unwrap_or_default();
                format!("{kind} {id}{name}{page}")
            }
        }
    }

    pub fn pointer(&self) -> String {
        match self {
            Part::Tree => "/tree".into(),
            Part::Node { id, .. } => format!("/nodes/{}", id.replace('~', "~0").replace('/', "~1")),
        }
    }
}

/// Every node a copy references — the tree's, and each held page's children —
/// as `api::walk_refs` reads it. Lenient: a copy whose references are
/// ambiguous is compared on what can be addressed; the tenant's copy never is
/// one, because `api::pull` refuses it.
fn walk(copy: &Value) -> (Vec<NodeRef>, Vec<String>) {
    let empty = Map::new();
    let nodes = copy
        .get("nodes")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    walk_refs(copy.get("tree").unwrap_or(&Value::Null), nodes)
}

fn refs(copy: &Value) -> Vec<NodeRef> {
    walk(copy).0
}

/// What makes a copy's node references ambiguous or unreadable — a tree
/// entry without a `nodeType`, one id under two types, a page inside a page,
/// a page listing that cannot be read. The comparison carries on with every
/// reference it can address; these say what it could not.
pub fn reference_problems(copy: &Value) -> Vec<String> {
    walk(copy).1
}

/// The parts that differ between two copies: the tree, then each node whose
/// config differs or exists on one side only, in id order.
///
/// A node's **type** comes from the first of `left`, `right` that lists it —
/// so the caller puts the side whose change is being described first: a
/// local retype is described by the local export's endpoint, the one a push
/// writes. Its **page** comes from `current` first (the tenant's copy where
/// one was read), since a child may have moved between pages since the
/// snapshot, and its page is what `shared_with` reads sharing from. The
/// display name falls through `left`, `right`, `current` to the first that
/// has one.
pub fn changed_parts(left: &Value, right: &Value, current: Option<&Value>) -> Vec<Part> {
    let mut out = Vec::new();
    let tree_equal = match (left.get("tree"), right.get("tree")) {
        (Some(a), Some(b)) => content_equal(a, b),
        (None, None) => true,
        _ => false,
    };
    if !tree_equal {
        out.push(Part::Tree);
    }
    let nodes = |copy: &Value| {
        copy.get("nodes")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    };
    let (a, b) = (nodes(left), nodes(right));
    let named: Vec<NodeRef> = [left, right].into_iter().flat_map(refs).collect();
    let placed: Vec<NodeRef> = current.into_iter().flat_map(refs).collect();
    let ids: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    for id in ids {
        let same = match (a.get(id), b.get(id)) {
            (Some(x), Some(y)) => content_equal(x, y),
            _ => false,
        };
        if !same {
            let of = |copies: &'_ [NodeRef]| {
                copies
                    .iter()
                    .filter(|node| node.id == *id)
                    .cloned()
                    .collect::<Vec<_>>()
            };
            let (named, placed) = (of(&named), of(&placed));
            let first = named.first();
            out.push(Part::Node {
                id: id.clone(),
                node_type: first.or(placed.first()).map(|node| node.node_type.clone()),
                display_name: named
                    .iter()
                    .chain(&placed)
                    .find_map(|node| node.display_name.clone()),
                page: placed.first().or(first).and_then(|node| node.page.clone()),
            });
        }
    }
    out
}

/// Every node id a copy's tree lists.
pub fn node_ids(copy: &Value) -> BTreeSet<String> {
    copy.pointer("/tree/nodes")
        .and_then(Value::as_object)
        .map(|nodes| nodes.keys().cloned().collect())
        .unwrap_or_default()
}

/// The nodes `copy` lists that its `nodes` map lacks — an export written by a
/// pull that left unread nodes out, or by one that predates page children.
/// Comparing such a copy says nothing about those nodes. "Lists" covers the
/// tree's nodes and the children each held page config names.
pub fn missing_nodes(copy: &Value) -> Vec<String> {
    let held = copy.get("nodes").and_then(Value::as_object);
    let mut missing: Vec<String> = refs(copy)
        .into_iter()
        .map(|node| node.id)
        .filter(|id| !held.is_some_and(|nodes| nodes.contains_key(id)))
        .collect();
    missing.sort();
    missing
}

/// The other journeys that list a changed node. A node config is a realm
/// object keyed by id, so pushing either journey rewrites it for both.
/// `owners` maps node id to every journey whose tree lists it. A page's child
/// is in no tree, so it is shared through its page: a child some other page
/// lists directly is not seen.
pub fn shared_with(
    owners: &std::collections::BTreeMap<String, Vec<String>>,
    journey: &str,
    part: &Part,
) -> Vec<String> {
    match part {
        // A page's child is in no tree, so it is shared through its page.
        Part::Node { id, page, .. } => {
            let names: BTreeSet<&String> = [Some(id), page.as_ref()]
                .into_iter()
                .flatten()
                .filter_map(|id| owners.get(id))
                .flatten()
                .filter(|n| *n != journey)
                .collect();
            names.into_iter().cloned().collect()
        }
        Part::Tree => Vec::new(),
    }
}

/// Whether a key names a value that must not be printed. A name rule, not a
/// schema one: AM's node schemas mark no property as secret —
/// `product-CaptchaNode.secretKey` is a plain `"type": "string"` (read
/// 2026-10-31) — so the schema cannot be asked. A hidden value still counts as
/// a difference; only its content is withheld.
pub fn is_secret_key(key: &str) -> bool {
    // Separators are dropped so `private_key`, `api-key` and `apiKey` are one
    // name. Over-hiding costs a `<hidden>` where a value was harmless;
    // under-hiding prints a secret, so the vocabulary errs wide.
    let key: String = key
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    [
        "password",
        "passphrase",
        "secret",
        "token",
        "credential",
        "privatekey",
        "apikey",
        "clientkey",
        "signingkey",
        "encryptionkey",
        "sharedkey",
        "hmac",
    ]
    .iter()
    .any(|word| key.contains(word))
        // A label naming *where* a secret lives is not the secret.
        && !key.ends_with("identifier")
        && !key.ends_with("label")
}

/// `value` with every secret-keyed value replaced by a placeholder, at any
/// depth, for printing.
pub fn redacted(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, child)| {
                    let child = if is_secret_key(key) && !child.is_null() {
                        Value::String(HIDDEN.into())
                    } else {
                        redacted(child)
                    };
                    (key.clone(), child)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redacted).collect()),
        other => other.clone(),
    }
}

pub const HIDDEN: &str = "<hidden>";

/// Whether a JSON pointer passes through a secret-named key.
pub fn path_is_secret(pointer: &str) -> bool {
    pointer
        .split('/')
        .any(|segment| is_secret_key(&segment.replace("~1", "/").replace("~0", "~")))
}

/// One difference, printable: values redacted when the path or a key inside
/// the value is secret-named.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Shown {
    pub path: String,
    pub left: Option<Value>,
    pub right: Option<Value>,
}

/// The differences between two copies, ready to print.
pub fn shown_changes(left: &Value, right: &Value) -> Vec<Shown> {
    crate::jsondiff::changes(&strip_revs(left), &strip_revs(right))
        .into_iter()
        .map(|change| {
            let hide = |value: Option<Value>| {
                value.map(|value| {
                    if path_is_secret(&change.path) {
                        Value::String(HIDDEN.into())
                    } else {
                        redacted(&value)
                    }
                })
            };
            Shown {
                left: hide(change.left.clone()),
                right: hide(change.right.clone()),
                path: change.path,
            }
        })
        .collect()
}

fn strip_revs(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| key.as_str() != "_rev")
                .map(|(key, child)| (key.clone(), strip_revs(child)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(strip_revs).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn export(page_text: &str, captcha_secret: &str) -> Value {
        json!({
            "tree": {"_id": "Login", "_rev": "1", "entryNodeId": "p", "nodes": {
                "p": {"nodeType": "PageNode", "displayName": "Page", "connections": {}},
                "c": {"nodeType": "product-CaptchaNode", "displayName": "Captcha", "connections": {}}}},
            "nodes": {
                "p": {"_id": "p", "_rev": "1", "pageHeader": {"en": page_text}},
                "c": {"_id": "c", "_rev": "1", "secretKey": captcha_secret, "siteKey": "site"}}
        })
    }

    #[test]
    fn every_three_way_case_is_named() {
        let base = export("a", "s");
        let local = export("b", "s");
        let remote = export("a", "t");
        assert_eq!(state(Some(&base), Some(&base), Some(&base)), State::InSync);
        assert_eq!(
            state(Some(&local), Some(&base), Some(&base)),
            State::LocallyModified
        );
        assert_eq!(
            state(Some(&base), Some(&base), Some(&remote)),
            State::RemotelyModified
        );
        assert_eq!(
            state(Some(&local), Some(&base), Some(&remote)),
            State::Conflict
        );
        assert_eq!(
            state(Some(&local), Some(&base), Some(&local)),
            State::Converged
        );
        assert_eq!(
            state(Some(&local), None, Some(&local)),
            State::NoSnapshot {
                matches_remote: true
            }
        );
        assert_eq!(state(None, Some(&base), Some(&base)), State::NotPulled);
        assert_eq!(state(Some(&base), Some(&base), None), State::RemoteMissing);
        // `_rev` alone is not a change — the tenant bumps it on every write.
        let mut bumped = base.clone();
        bumped["nodes"]["p"]["_rev"] = json!("9");
        assert_eq!(
            state(Some(&base), Some(&base), Some(&bumped)),
            State::InSync
        );
    }

    #[test]
    fn changed_parts_name_the_node_from_its_tree_entry() {
        let parts = changed_parts(&export("a", "s"), &export("b", "s"), None);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].label(), "PageNode p (Page)");
        let mut retargeted = export("a", "s");
        retargeted["tree"]["entryNodeId"] = json!("c");
        assert_eq!(
            changed_parts(&export("a", "s"), &retargeted, None),
            vec![Part::Tree]
        );
    }

    /// The discriminating case: a secret change is still a change, but its
    /// value is never printed — neither at its own path nor inside a value
    /// printed whole for a node present on one side only.
    #[test]
    fn secrets_count_as_changes_and_are_never_shown() {
        let left = export("a", "s3cret-one");
        let right = export("a", "s3cret-two");
        assert_eq!(
            state(Some(&left), Some(&left), Some(&right)),
            State::RemotelyModified
        );
        let shown = shown_changes(&left, &right);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].path, "/nodes/c/secretKey");
        assert_eq!(shown[0].left, Some(json!(HIDDEN)));
        assert_eq!(shown[0].right, Some(json!(HIDDEN)));

        let mut without = left.clone();
        without["nodes"].as_object_mut().unwrap().remove("c");
        let whole = shown_changes(&without, &right);
        let printed = serde_json::to_string(&whole).unwrap();
        assert!(!printed.contains("s3cret"), "{printed}");
        assert!(printed.contains("site"), "non-secret fields still print");
    }

    /// The persistent-cookie nodes wrap their key as `{"$string": …}`, so
    /// the changed leaf is `/hmacSigningKey/$string` and the secret name is
    /// one segment up.
    #[test]
    fn a_wrapped_hmac_signing_key_is_never_shown() {
        let node = |key: &str| {
            json!({"nodes": {"k": {"_id": "k", "nodeType": "SetPersistentCookieNode",
                "hmacSigningKey": {"$string": key}, "idleTimeout": 5}}})
        };
        let shown = shown_changes(&node("DEMO_KEY_ONE"), &node("DEMO_KEY_TWO"));
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].left, Some(json!(HIDDEN)));
        let added = shown_changes(&json!({"nodes": {}}), &node("DEMO_KEY_TWO"));
        let printed = serde_json::to_string(&added).unwrap();
        assert!(!printed.contains("DEMO_KEY"), "{printed}");
        assert!(printed.contains("idleTimeout"), "{printed}");
    }

    #[test]
    fn a_copy_missing_a_listed_node_is_incomplete() {
        let whole = export("a", "s");
        assert!(missing_nodes(&whole).is_empty());
        let mut partial = whole.clone();
        partial["nodes"].as_object_mut().unwrap().remove("c");
        assert_eq!(missing_nodes(&partial), vec!["c"]);
    }

    fn paged(with_children: bool) -> Value {
        let mut nodes = json!({"p": {"_id": "p", "nodes": [
            {"_id": "k", "nodeType": "UsernameCollectorNode", "displayName": "user"}]}});
        if with_children {
            nodes["k"] = json!({"_id": "k", "_rev": "1"});
        }
        json!({"tree": {"nodes": {"p": {"nodeType": "PageNode"}}}, "nodes": nodes})
    }

    #[test]
    fn a_page_child_is_missing_when_the_copy_carries_the_page_alone() {
        assert!(missing_nodes(&paged(true)).is_empty());
        assert_eq!(missing_nodes(&paged(false)), vec!["k"]);
    }

    #[test]
    fn a_changed_page_child_is_named_from_its_pages_listing() {
        let mut remote = paged(true);
        remote["nodes"]["k"]["x"] = json!(1);
        let parts = changed_parts(&paged(true), &remote, None);
        assert_eq!(
            parts,
            vec![Part::Node {
                id: "k".into(),
                node_type: Some("UsernameCollectorNode".into()),
                display_name: Some("user".into()),
                page: Some("p".into()),
            }]
        );
        assert_eq!(parts[0].label(), "UsernameCollectorNode k (user) in page p");
    }

    /// Regression (review P1-3): any held config with a `nodes` array could
    /// name a changed child. The discriminating input is a custom node whose
    /// config sorts before the page and lists the child's id with unrelated
    /// metadata.
    #[test]
    fn only_a_page_names_its_children() {
        let decoy = |copy: &mut Value| {
            copy["tree"]["nodes"]["a"] = json!({"nodeType": "CustomListNode"});
            copy["nodes"]["a"] = json!({"nodes": [
                {"_id": "k", "nodeType": "Unrelated", "displayName": "decoy"}]});
        };
        let (mut left, mut right) = (paged(true), paged(true));
        decoy(&mut left);
        decoy(&mut right);
        right["nodes"]["k"]["x"] = json!(1);
        let parts = changed_parts(&left, &right, None);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].label(), "UsernameCollectorNode k (user) in page p");
        // Nor does such a config make its entries count as missing.
        assert!(missing_nodes(&left).is_empty());
    }

    /// Regression (review round 2 P1-3): one malformed tree entry hid the
    /// note for a valid missing body, and the problem itself was dropped.
    #[test]
    fn a_malformed_entry_is_a_problem_beside_the_valid_missing_one() {
        let copy = json!({"tree": {"nodes": {
            "bad": {"displayName": "no type"},
            "missing": {"nodeType": "SetSuccessUrlNode"}
        }}, "nodes": {}});
        assert_eq!(missing_nodes(&copy), vec!["missing"]);
        let problems = reference_problems(&copy);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("\"bad\""), "{problems:?}");
        assert!(reference_problems(&paged(true)).is_empty());
    }

    /// Regression (review round 2 P1-4): a child that moved between pages was
    /// placed in its snapshot page, and its sharing read from that. The
    /// discriminating input is `k`, under page `old` in the snapshot and under
    /// `p` on the tenant.
    #[test]
    fn a_moved_child_is_placed_by_the_current_copy() {
        let snapshot = json!({"tree": {"nodes": {"old": {"nodeType": "PageNode"}}},
            "nodes": {"old": {"nodes": [{"_id": "k", "nodeType": "UsernameCollectorNode"}]},
                      "k": {"x": 0}}});
        let mut remote = paged(true);
        remote["nodes"]["k"]["x"] = json!(1);
        let page = |current| {
            changed_parts(&snapshot, &remote, current)
                .into_iter()
                .find_map(|part| match part {
                    Part::Node { id, page, .. } if id == "k" => Some(page),
                    _ => None,
                })
                .expect("k changed")
        };
        assert_eq!(page(Some(&remote)).as_deref(), Some("p"));
        // The control: with no current copy, the first of the pair decides.
        assert_eq!(page(None).as_deref(), Some("old"));
    }

    /// Regression (review round 2 P3-1): the display name came only from the
    /// first copy listing the node, so a name removed on one side was lost.
    #[test]
    fn a_display_name_falls_through_to_the_other_copy() {
        let named = export("a", "s");
        let mut unnamed = export("b", "s");
        unnamed["tree"]["nodes"]["p"]
            .as_object_mut()
            .unwrap()
            .remove("displayName");
        let parts = changed_parts(&unnamed, &named, None);
        assert_eq!(parts.last().unwrap().label(), "PageNode p (Page)");
    }

    #[test]
    fn a_node_listed_by_another_journey_is_reported_as_shared() {
        let owners = std::collections::BTreeMap::from([
            (
                "p".to_string(),
                vec!["Login".to_string(), "Register".to_string()],
            ),
            ("c".to_string(), vec!["Login".to_string()]),
        ]);
        let in_page = |id: &str, page: Option<&str>| Part::Node {
            id: id.into(),
            node_type: None,
            display_name: None,
            page: page.map(str::to_string),
        };
        let node = |id: &str| in_page(id, None);
        assert_eq!(shared_with(&owners, "Login", &node("p")), vec!["Register"]);
        // Regression (review P2-4): a page's child is in no tree, so it was
        // never found shared. It is shared through its page.
        let child = in_page("k", Some("p"));
        assert_eq!(shared_with(&owners, "Login", &child), vec!["Register"]);
        assert!(shared_with(&owners, "Login", &node("c")).is_empty());
        assert!(shared_with(&owners, "Login", &node("unknown")).is_empty());
        assert!(shared_with(&owners, "Login", &Part::Tree).is_empty());
    }

    #[test]
    fn secret_names_are_matched_but_labels_for_them_are_not() {
        for key in [
            "secretKey",
            "password",
            "clientSecret",
            "apiKeySecret",
            "accessToken",
            // Documented in docs/api/09-journeys.md on the persistent-cookie
            // nodes, and missed by the first vocabulary.
            "hmacSigningKey",
            "private_key",
            "api-key",
            "Client_Secret",
            "signing_key",
        ] {
            assert!(is_secret_key(key), "{key}");
        }
        for key in [
            "siteKey",
            "publicKey",
            "secretLabelIdentifier",
            "tokenLabel",
            "username",
        ] {
            assert!(!is_secret_key(key), "{key}");
        }
    }
}
