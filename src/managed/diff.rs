//! `aic managed diff`: one tenant object against a repository file, after
//! normalising away differences that carry no meaning — each a named rule,
//! listed in the output and switched off together by `--strict`.
//!
//! Tenant-free: `cli.rs` reads both sides and prints what this returns.

use serde::Serialize;
use serde_json::{Map, Value};

/// A difference this module normalises away, named so the output can say
/// which ones it applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Rule {
    /// `_refProperties.properties._id` is written three ways: by the console
    /// as `{"propName":"_id","required":false,"type":"string"}`, by Ping's
    /// shipped objects as `{"description":"_refProperties object ID",
    /// "type":"string"}`, and by `aic` without a description. Only `type`
    /// means anything; both shapes were read from the sandbox's `alpha_user`.
    RefPropertiesId,
    /// An event hook's body: the tenant holds `source`, a repository may hold
    /// `file`, and both are scripts `aic script status` compares. The hook's
    /// presence, `type` and `globals` are still compared.
    HookBody,
}

impl Rule {
    pub fn describe(self) -> &'static str {
        match self {
            Rule::RefPropertiesId => {
                "_refProperties._id: only `type` compared (console adds propName/required, \
                 shipped objects a description)"
            }
            Rule::HookBody => {
                "hook bodies (`source`/`file`) not compared; `aic script status` compares scripts"
            }
        }
    }
}

/// Picks the object named `name` out of a repository file holding one object
/// definition (`{"name": …, "schema": …}`) or a whole `config/managed`
/// document (`{"objects": […]}`).
pub fn select_object(file: &Value, name: &str) -> Result<Value, String> {
    if let Some(objects) = file.get("objects").and_then(Value::as_array) {
        return objects
            .iter()
            .find(|object| object.get("name").and_then(Value::as_str) == Some(name))
            .cloned()
            .ok_or_else(|| format!("the file's `objects` has no object named {name}"));
    }
    match file.get("name").and_then(Value::as_str) {
        Some(found) if found == name => Ok(file.clone()),
        Some(found) => Err(format!("the file defines object {found}, not {name}")),
        None if file.get("schema").is_some() => Err(format!(
            "the file has a `schema` but no `name`; expected the definition of {name}"
        )),
        None => Err(
            "the file is neither an object definition ({\"name\", \"schema\"}) nor a \
             config/managed document ({\"objects\"})"
                .into(),
        ),
    }
}

fn is_hook(value: &Map<String, Value>) -> bool {
    value.contains_key("type") && (value.contains_key("source") || value.contains_key("file"))
}

/// `value` with every rule applied, and the rules that changed something.
pub fn normalise(value: &Value) -> (Value, Vec<Rule>) {
    let mut applied = Vec::new();
    let out = walk(value, false, &mut applied);
    applied.sort_by_key(|rule| *rule as u8);
    applied.dedup();
    (out, applied)
}

fn walk(value: &Value, in_ref_properties: bool, applied: &mut Vec<Rule>) -> Value {
    match value {
        Value::Object(map) if is_hook(map) => {
            applied.push(Rule::HookBody);
            Value::Object(
                map.iter()
                    .filter(|(key, _)| !matches!(key.as_str(), "source" | "file"))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            )
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, child)| {
                    let child = if in_ref_properties && key == "_id" {
                        match child.get("type") {
                            Some(kind) => {
                                let reduced = serde_json::json!({ "type": kind });
                                if &reduced != child {
                                    applied.push(Rule::RefPropertiesId);
                                }
                                reduced
                            }
                            None => walk(child, false, applied),
                        }
                    } else if key == "_refProperties" {
                        match child.get("properties") {
                            Some(_) => {
                                let mut inner = child.clone();
                                if let Some(properties) = inner.get_mut("properties") {
                                    *properties = walk(properties, true, applied);
                                }
                                walk_except_properties(&inner, applied)
                            }
                            None => walk(child, false, applied),
                        }
                    } else {
                        walk(child, false, applied)
                    };
                    (key.clone(), child)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| walk(item, false, applied))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Walks a `_refProperties` node whose `properties` were already normalised.
fn walk_except_properties(node: &Value, applied: &mut Vec<Rule>) -> Value {
    match node {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, child)| {
                    let child = if key == "properties" {
                        child.clone()
                    } else {
                        walk(child, false, applied)
                    };
                    (key.clone(), child)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// One place the two sides disagree, as a JSON pointer into the object.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Difference {
    TenantOnly {
        path: String,
        tenant: Value,
    },
    RepoOnly {
        path: String,
        repo: Value,
    },
    Changed {
        path: String,
        tenant: Value,
        repo: Value,
    },
}

impl Difference {
    pub fn path(&self) -> &str {
        match self {
            Difference::TenantOnly { path, .. }
            | Difference::RepoOnly { path, .. }
            | Difference::Changed { path, .. } => path,
        }
    }
}

/// Every difference between two normalised values. Keys compare as a set;
/// arrays by position, because order is meaning in `schema.order` and in a
/// `resourceCollection`.
pub fn differences(tenant: &Value, repo: &Value) -> Vec<Difference> {
    crate::jsondiff::changes(tenant, repo)
        .into_iter()
        .filter_map(|change| match (change.left, change.right) {
            (Some(tenant), Some(repo)) => Some(Difference::Changed {
                path: change.path,
                tenant,
                repo,
            }),
            (Some(tenant), None) => Some(Difference::TenantOnly {
                path: change.path,
                tenant,
            }),
            (None, Some(repo)) => Some(Difference::RepoOnly {
                path: change.path,
                repo,
            }),
            (None, None) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object(id: Value, hook: Value) -> Value {
        json!({"name": "widget", "onCreate": hook, "schema": {
            "order": ["owner", "label"],
            "properties": {
                "label": {"type": "string", "title": "Label"},
                "owner": {"type": "relationship", "properties": {
                    "_ref": {"type": "string"},
                    "_refProperties": {"type": "object", "properties": {"_id": id}}}}}}})
    }

    #[test]
    fn console_and_shipped_ref_property_ids_compare_equal_unless_the_type_differs() {
        let console = object(
            json!({"propName": "_id", "required": false, "type": "string"}),
            json!({"type": "text/javascript", "source": "a"}),
        );
        let shipped = object(
            json!({"description": "_refProperties object ID", "type": "string"}),
            json!({"type": "text/javascript", "file": "widget.onCreate.js"}),
        );
        let (a, rules_a) = normalise(&console);
        let (b, rules_b) = normalise(&shipped);
        assert!(differences(&a, &b).is_empty());
        assert_eq!(rules_a, vec![Rule::RefPropertiesId, Rule::HookBody]);
        assert_eq!(rules_b, vec![Rule::RefPropertiesId, Rule::HookBody]);
        // Discriminating: a real change inside the same node still shows.
        let typed = object(
            json!({"type": "number"}),
            json!({"type": "text/javascript", "source": "a"}),
        );
        let (c, _) = normalise(&typed);
        assert_eq!(
            differences(&a, &c)
                .iter()
                .map(Difference::path)
                .collect::<Vec<_>>(),
            vec!["/schema/properties/owner/properties/_refProperties/properties/_id/type"]
        );
    }

    #[test]
    fn an_id_outside_ref_properties_is_compared_whole() {
        let a = json!({"schema": {"properties": {"_id": {"type": "string", "required": false}}}});
        let b = json!({"schema": {"properties": {"_id": {"type": "string"}}}});
        let (a, rules) = normalise(&a);
        let (b, _) = normalise(&b);
        assert!(rules.is_empty());
        assert_eq!(differences(&a, &b).len(), 1);
    }

    #[test]
    fn hook_type_and_presence_are_still_compared() {
        let base = object(
            json!({"type": "string"}),
            json!({"type": "text/javascript", "source": "a"}),
        );
        let mut groovy = base.clone();
        groovy["onCreate"]["type"] = json!("groovy");
        let mut gone = base.clone();
        gone.as_object_mut().unwrap().remove("onCreate");
        let (n, _) = normalise(&base);
        assert_eq!(differences(&n, &normalise(&groovy).0).len(), 1);
        assert!(matches!(
            differences(&n, &normalise(&gone).0)[..],
            [Difference::TenantOnly { .. }]
        ));
    }

    #[test]
    fn order_is_positional_and_keys_escape() {
        let a = json!({"order": ["a", "b"], "x/y": 1});
        let b = json!({"order": ["b", "a", "c"], "x/y": 2});
        let paths: Vec<String> = differences(&a, &b)
            .iter()
            .map(|d| d.path().to_string())
            .collect();
        assert_eq!(paths, vec!["/order/0", "/order/1", "/order/2", "/x~1y"]);
    }

    #[test]
    fn select_reads_an_object_or_a_whole_document() {
        let one = json!({"name": "widget", "schema": {}});
        assert_eq!(select_object(&one, "widget").unwrap(), one);
        assert!(
            select_object(&one, "gadget")
                .unwrap_err()
                .contains("defines object widget")
        );
        let doc = json!({"_id": "managed", "objects": [{"name": "a"}, one.clone()]});
        assert_eq!(select_object(&doc, "widget").unwrap(), one);
        assert!(select_object(&doc, "gadget").is_err());
        assert!(select_object(&json!({"schema": {}}), "widget").is_err());
        assert!(select_object(&json!([1]), "widget").is_err());
    }
}
