//! Verified HTTP wrappers for the IDM `managed` config document.
//! All HTTP goes through `aic::api` → agent. See `docs/api/10-managed-objects.md`.

use crate::{Error, Result};
use serde_json::Value;

/// `GET /openidm/config/managed` → the full schema document
/// (`{ _id: "managed", objects: [...] }`). No `_rev`.
pub async fn get_managed(tenant: &str) -> Result<Value> {
    crate::aic::api::get(tenant, "/openidm/config/managed").await
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

/// Find one object definition by `name`.
pub fn object_named<'a>(doc: &'a Value, name: &str) -> Result<&'a Value> {
    objects(doc)?
        .iter()
        .find(|o| o.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| Error::Config(format!("no managed object named '{name}' on this tenant")))
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
}
