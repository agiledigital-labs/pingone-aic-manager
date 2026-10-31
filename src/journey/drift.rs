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

use super::api::content_equal;
use serde::Serialize;
use serde_json::Value;
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
        /// From whichever copy's tree lists it.
        node_type: Option<String>,
        display_name: Option<String>,
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
            } => {
                let kind = node_type.as_deref().unwrap_or("node");
                match display_name {
                    Some(name) => format!("{kind} {id} ({name})"),
                    None => format!("{kind} {id}"),
                }
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

fn node_meta(copies: &[&Value], id: &str, key: &str) -> Option<String> {
    copies.iter().find_map(|copy| {
        copy.pointer("/tree/nodes")
            .and_then(|nodes| nodes.get(id))
            .and_then(|meta| meta.get(key))
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

/// The parts that differ between two copies: the tree, then each node whose
/// config differs or exists on one side only, in id order.
pub fn changed_parts(left: &Value, right: &Value) -> Vec<Part> {
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
    let ids: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    for id in ids {
        let same = match (a.get(id), b.get(id)) {
            (Some(x), Some(y)) => content_equal(x, y),
            _ => false,
        };
        if !same {
            out.push(Part::Node {
                id: id.clone(),
                node_type: node_meta(&[left, right], id, "nodeType"),
                display_name: node_meta(&[left, right], id, "displayName"),
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

/// The nodes `copy`'s tree lists that its `nodes` map lacks — an export
/// written by a pull that left unread nodes out. Comparing such a copy says
/// nothing about those nodes.
pub fn missing_nodes(copy: &Value) -> Vec<String> {
    let held = copy.get("nodes").and_then(Value::as_object);
    node_ids(copy)
        .into_iter()
        .filter(|id| !held.is_some_and(|nodes| nodes.contains_key(id)))
        .collect()
}

/// The other journeys that list a changed node. A node config is a realm
/// object keyed by id, so pushing either journey rewrites it for both.
/// `owners` maps node id to every journey listing it.
pub fn shared_with(
    owners: &std::collections::BTreeMap<String, Vec<String>>,
    journey: &str,
    part: &Part,
) -> Vec<String> {
    match part {
        Part::Node { id, .. } => owners
            .get(id)
            .map(|names| names.iter().filter(|n| *n != journey).cloned().collect())
            .unwrap_or_default(),
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
        let parts = changed_parts(&export("a", "s"), &export("b", "s"));
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].label(), "PageNode p (Page)");
        let mut retargeted = export("a", "s");
        retargeted["tree"]["entryNodeId"] = json!("c");
        assert_eq!(
            changed_parts(&export("a", "s"), &retargeted),
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

    #[test]
    fn a_node_listed_by_another_journey_is_reported_as_shared() {
        let owners = std::collections::BTreeMap::from([
            (
                "p".to_string(),
                vec!["Login".to_string(), "Register".to_string()],
            ),
            ("c".to_string(), vec!["Login".to_string()]),
        ]);
        let node = |id: &str| Part::Node {
            id: id.into(),
            node_type: None,
            display_name: None,
        };
        assert_eq!(shared_with(&owners, "Login", &node("p")), vec!["Register"]);
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
