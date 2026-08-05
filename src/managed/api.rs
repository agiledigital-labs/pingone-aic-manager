//! Verified HTTP wrappers for the IDM `managed` config document.
//! All HTTP goes through `aic::api` → agent. See `docs/api/10-managed-objects.md`.

use crate::{Error, Result};
use serde_json::Value;

/// What a config write must be able to observe before it is believed.
///
/// `config/managed` is not read-your-writes consistent, so a 200 on the PUT is
/// not evidence the change is stored (Q14). Every writer states what it expects
/// to see and the write is retried until the tenant agrees.
#[derive(Debug)]
pub enum ConfigConfirm {
    /// The object exists and its content equals this exactly.
    ObjectContent { name: String, content: Value },
    /// The object exists; content unconstrained.
    ObjectPresent { name: String },
    /// No object of this name exists.
    ObjectAbsent { name: String },
    /// The whole document equals this.
    DocumentEquals(Value),
}

impl ConfigConfirm {
    /// Whether this confirmation condition holds for a fetched managed document.
    pub fn holds(&self, doc: &Value) -> bool {
        match self {
            Self::ObjectContent { name, content } => {
                object_named(doc, name).is_ok_and(|object| object_content_equal(object, content))
            }
            Self::ObjectPresent { name } => object_named(doc, name).is_ok(),
            Self::ObjectAbsent { name } => objects(doc).is_ok_and(|objects| {
                !objects
                    .iter()
                    .any(|object| object.get("name").and_then(Value::as_str) == Some(name))
            }),
            Self::DocumentEquals(expected) => doc == expected,
        }
    }

    fn description(&self) -> String {
        match self {
            Self::ObjectContent { name, .. } => {
                format!("managed object '{name}' with the requested content")
            }
            Self::ObjectPresent { name } => format!("managed object '{name}' to be present"),
            Self::ObjectAbsent { name } => format!("managed object '{name}' to be absent"),
            Self::DocumentEquals(_) => "the complete managed config document to match".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordCount {
    Exact(usize),
    AtLeast(usize),
}

impl std::fmt::Display for RecordCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exact(n) => write!(f, "{n}"),
            Self::AtLeast(n) => write!(f, "{n}+"),
        }
    }
}

/// Gets a bounded record count for a rename warning without exhausting pages.
pub async fn count_records(tenant: &str, object_name: &str) -> Result<RecordCount> {
    let path =
        format!("/openidm/managed/{object_name}?_queryFilter=true&_fields=_id&_pageSize=100");
    let body = crate::aic::api::get(tenant, &path).await?;
    let count = body
        .get("result")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if body
        .get("pagedResultsCookie")
        .is_some_and(|value| !value.is_null())
    {
        Ok(RecordCount::AtLeast(count))
    } else {
        Ok(RecordCount::Exact(count))
    }
}

/// `GET /openidm/config/managed` → the full schema document
/// (`{ _id: "managed", objects: [...] }`). No `_rev`.
pub async fn get_managed(tenant: &str) -> Result<Value> {
    crate::aic::api::get(tenant, "/openidm/config/managed").await
}

/// Unconfirmed `PUT /openidm/config/managed` with the complete managed config
/// document.
///
/// The API has no object-level patch endpoint: callers must read, mutate the
/// intended `objects[]` entry, and send the whole `{ "_id": "managed", ... }`
/// envelope back. External callers should use [`replace_managed_confirmed`].
pub async fn replace_managed(tenant: &str, doc: Value, confirmed_prod: bool) -> Result<Value> {
    crate::aic::api::put(tenant, "/openidm/config/managed", doc, confirmed_prod).await
}

/// Replaces a managed config document until every requested condition is visible.
///
/// A successful PUT can still be followed by a stale GET (Q14). Retry the exact
/// same whole document rather than rebuilding it from that stale read: a
/// relationship may span two objects, and a partial rebase could corrupt its
/// other end. This endpoint has no `If-Match` and is already last-writer-wins,
/// so resending the body is no more dangerous than the original PUT.
pub async fn replace_managed_confirmed(
    tenant: &str,
    doc: Value,
    expect: &[ConfigConfirm],
    confirmed_prod: bool,
) -> Result<()> {
    if expect.is_empty() {
        return Err(Error::Config(
            "managed config write requires at least one confirmation condition".into(),
        ));
    }

    const ATTEMPTS: u32 = 6;
    let mut failed = None;
    for attempt in 0..ATTEMPTS {
        replace_managed(tenant, doc.clone(), confirmed_prod).await?;
        let fetched = get_managed(tenant).await?;
        failed = expect.iter().find(|condition| !condition.holds(&fetched));
        if failed.is_none() {
            return Ok(());
        }
        if attempt + 1 < ATTEMPTS {
            tokio::time::sleep(std::time::Duration::from_millis(500 * (1 << attempt))).await;
        }
    }

    let failed = failed.map_or_else(
        || "an unknown confirmation condition".to_string(),
        ConfigConfirm::description,
    );
    Err(Error::Config(format!(
        "managed config write for tenant '{tenant}' was accepted but not persisted: expected {failed}; \
         see scripts/experiment-managed-lost-updates.sh and docs/api/99-quirks-and-open-questions.md Q14"
    )))
}

/// The `objects` array of a managed document.
pub fn objects(doc: &Value) -> Result<&Vec<Value>> {
    doc.get("objects")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected /openidm/config/managed shape: {doc}"),
        })
}

/// The mutable `objects` array of a managed document.
pub fn objects_mut(doc: &mut Value) -> Result<&mut Vec<Value>> {
    if doc.get("objects").and_then(Value::as_array).is_none() {
        return Err(Error::Api {
            status: 0,
            body: format!("unexpected /openidm/config/managed shape: {doc}"),
        });
    }
    Ok(doc
        .get_mut("objects")
        .and_then(Value::as_array_mut)
        .expect("checked objects array above"))
}

/// Find one object definition by `name`.
pub fn object_named<'a>(doc: &'a Value, name: &str) -> Result<&'a Value> {
    objects(doc)?
        .iter()
        .find(|o| o.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| Error::Config(format!("no managed object named '{name}' on this tenant")))
}

/// Find one mutable object definition by `name`.
pub fn object_named_mut<'a>(doc: &'a mut Value, name: &str) -> Result<&'a mut Value> {
    objects_mut(doc)?
        .iter_mut()
        .find(|o| o.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| Error::Config(format!("no managed object named '{name}' on this tenant")))
}

/// Fetch the full managed document and clone out the requested object.
pub async fn get_managed_with_object(tenant: &str, name: &str) -> Result<(Value, Value)> {
    let doc = get_managed(tenant).await?;
    let object = object_named(&doc, name)?.clone();
    Ok((doc, object))
}

/// Replace one `objects[]` entry in an already-fetched managed document,
/// preserving the full document envelope and every other object verbatim.
pub fn replace_object(doc: &mut Value, name: &str, object: Value) -> Result<()> {
    let slot = object_named_mut(doc, name)?;
    *slot = object;
    Ok(())
}

/// Content comparison for managed object subtrees. `serde_json::Value`
/// equality compares object maps independent of key insertion order, which is
/// exactly what we need for snapshot-based drift checks.
pub fn object_content_equal(a: &Value, b: &Value) -> bool {
    a == b
}

/// Summary row for `aic managed list`.
pub struct ObjectSummary {
    pub name: String,
    pub properties: usize,
    /// Inline-source hooks (syncable via `aic script`).
    pub hooks_inline: Vec<String>,
    /// File-backed hooks (server-side files — read-only markers).
    pub hooks_file: Vec<String>,
}

pub fn summarize(doc: &Value) -> Result<Vec<ObjectSummary>> {
    let mut out = Vec::new();
    for obj in objects(doc)? {
        let Some(name) = obj.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(map) = obj.as_object() else { continue };
        let properties = obj
            .pointer("/schema/properties")
            .and_then(Value::as_object)
            .map(|m| m.len())
            .unwrap_or(0);
        let mut hooks_inline = Vec::new();
        let mut hooks_file = Vec::new();
        for (key, value) in map {
            if key == "schema" || !value.is_object() {
                continue;
            }
            let is_js = value
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains("javascript"));
            if !is_js {
                continue;
            }
            if value.get("source").is_some_and(Value::is_string) {
                hooks_inline.push(key.clone());
            } else if value.get("file").is_some_and(Value::is_string) {
                hooks_file.push(key.clone());
            }
        }
        hooks_inline.sort();
        hooks_file.sort();
        out.push(ObjectSummary {
            name: name.to_string(),
            properties,
            hooks_inline,
            hooks_file,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summarize_classifies_hooks_and_counts_properties() {
        let doc = json!({"objects": [{
            "name": "alpha_user",
            "schema": {"properties": {"a": {}, "b": {}}},
            "onCreate": {"type": "text/javascript", "source": "x"},
            "onDelete": {"type": "text/javascript", "file": "roles/onDelete-roles.js"},
            "iconClass": "fa fa-user"
        }]});
        let s = summarize(&doc).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "alpha_user");
        assert_eq!(s[0].properties, 2);
        assert_eq!(s[0].hooks_inline, vec!["onCreate"]);
        assert_eq!(s[0].hooks_file, vec!["onDelete"]);
    }

    #[test]
    fn object_named_errors_helpfully() {
        let doc = json!({"objects": []});
        assert!(
            object_named(&doc, "ghost")
                .unwrap_err()
                .to_string()
                .contains("ghost")
        );
    }

    #[test]
    fn replace_object_preserves_document_envelope() {
        let mut doc = json!({
            "_id": "managed",
            "objects": [
                {"name": "alpha_user", "schema": {"properties": {"a": {}}}},
                {"name": "alpha_role", "schema": {"properties": {}}}
            ],
            "extra": true
        });

        replace_object(
            &mut doc,
            "alpha_user",
            json!({"name": "alpha_user", "schema": {"properties": {"b": {}}}}),
        )
        .unwrap();

        assert_eq!(doc["_id"], json!("managed"));
        assert_eq!(doc["extra"], json!(true));
        assert_eq!(doc["objects"][0]["schema"]["properties"], json!({"b": {}}));
        assert_eq!(doc["objects"][1]["name"], json!("alpha_role"));
    }

    #[test]
    fn object_content_equal_ignores_map_insertion_order() {
        let a = json!({"name": "alpha_user", "schema": {"required": ["a"], "properties": {}}});
        let b = json!({"schema": {"properties": {}, "required": ["a"]}, "name": "alpha_user"});
        assert!(object_content_equal(&a, &b));
    }

    #[test]
    fn config_confirm_object_content_is_order_independent_but_value_sensitive() {
        let doc = json!({"objects": [{"name": "alpha_user", "schema": {"properties": {"a": 1}}}]});
        assert!(
            ConfigConfirm::ObjectContent {
                name: "alpha_user".into(),
                content: json!({"schema": {"properties": {"a": 1}}, "name": "alpha_user"}),
            }
            .holds(&doc)
        );
        assert!(
            !ConfigConfirm::ObjectContent {
                name: "alpha_user".into(),
                content: json!({"name": "alpha_user", "schema": {"properties": {"a": 2}}}),
            }
            .holds(&doc)
        );
    }

    #[test]
    fn config_confirm_object_presence_and_absence() {
        let doc = json!({"objects": [{"name": "alpha_user"}]});
        assert!(
            ConfigConfirm::ObjectPresent {
                name: "alpha_user".into()
            }
            .holds(&doc)
        );
        assert!(
            !ConfigConfirm::ObjectPresent {
                name: "alpha_role".into()
            }
            .holds(&doc)
        );
        assert!(
            !ConfigConfirm::ObjectAbsent {
                name: "alpha_user".into()
            }
            .holds(&doc)
        );
        assert!(
            ConfigConfirm::ObjectAbsent {
                name: "alpha_role".into()
            }
            .holds(&doc)
        );
    }

    #[test]
    fn config_confirm_document_equals_distinguishes_documents() {
        let doc = json!({"objects": []});
        assert!(ConfigConfirm::DocumentEquals(json!({"objects": []})).holds(&doc));
        assert!(!ConfigConfirm::DocumentEquals(json!({"objects": [{}]})).holds(&doc));
    }

    #[tokio::test]
    async fn confirmed_replace_rejects_empty_expectations_before_network_io() {
        let error = replace_managed_confirmed("unused", json!({}), &[], false)
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::Config(message) if message.contains("confirmation condition"))
        );
    }
}
