//! Plain input specifications for managed-object field transforms.
//!
//! These types deliberately contain no TUI state so the same schema changes
//! can be performed by both the interactive editor and command-line callers.
//! `aic managed get` presentation (field globs, property filtering, hook-source
//! elision) lives here for the same reason: the CLI should not own the
//! transforms.

use std::collections::HashSet;

use serde_json::Value;

use crate::managed::api::is_inline_hook;
use crate::tui::widgets::{JsonShape, ValueShape};

/// Scalar property type supported by the managed field creator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarFieldType {
    String,
    Boolean,
    Number,
    StringArray,
}

impl ScalarFieldType {
    pub fn shape(self) -> ValueShape {
        match self {
            Self::String => ValueShape::Text,
            Self::Boolean => ValueShape::Bool,
            Self::Number => ValueShape::Decimal,
            Self::StringArray => ValueShape::Json(JsonShape::Array),
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::String => Self::Boolean,
            Self::Boolean => Self::Number,
            Self::Number => Self::StringArray,
            Self::StringArray => Self::String,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::String => Self::StringArray,
            Self::Boolean => Self::String,
            Self::Number => Self::Boolean,
            Self::StringArray => Self::Number,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Boolean => "boolean",
            Self::Number => "number",
            Self::StringArray => "string[]",
        }
    }
}

/// Requested changes to one managed property.
///
/// Each optional attribute is left untouched when it is `None`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FieldEditSpec {
    pub new_key: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub required: Option<bool>,
    pub searchable: Option<bool>,
    pub viewable: Option<bool>,
    pub user_editable: Option<bool>,
    pub enum_change: EnumChange,
    pub default_change: DefaultChange,
    pub allow_narrowing: bool,
}

/// One allowed value and its optional display label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumValue {
    pub value: String,
    pub title: Option<String>,
}

/// A property's allowed-value constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumSpec {
    pub values: Vec<EnumValue>,
}

impl EnumSpec {
    /// Renders the constraint back into the item grammar.
    pub fn to_items(&self) -> Vec<String> {
        self.values
            .iter()
            .map(|item| match &item.title {
                Some(title) => format!("{}:{title}", item.value),
                None => item.value.clone(),
            })
            .collect()
    }
}

/// What an edit should do to a property's allowed-value constraint.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum EnumChange {
    /// Leave the constraint — or its absence — alone.
    #[default]
    Unchanged,
    /// Replace the constraint with this set.
    Set(EnumSpec),
    /// Drop the constraint. Widening, so always safe.
    Clear,
}

/// What a change should do to a property's default value.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum DefaultChange {
    /// Leave the default — or its absence — alone.
    #[default]
    Unchanged,
    /// Set the default from this raw text, coerced against the property's type.
    Set(String),
    /// Drop the default.
    Clear,
}

/// Parses `value` / `value:Title` items into a constraint.
pub fn parse_enum_items<S: AsRef<str>>(items: &[S]) -> Result<EnumSpec, String> {
    let mut values = Vec::new();
    for item in items {
        let item = item.as_ref().trim();
        if item.is_empty() {
            continue;
        }
        let (value, title) = match item.split_once(':') {
            Some((value, title)) => (value.trim(), Some(title.trim().to_string())),
            None => (item, None),
        };
        if value.is_empty() {
            return Err(format!("enum item '{item}' has an empty value"));
        }
        if values
            .iter()
            .any(|existing: &EnumValue| existing.value == value)
        {
            return Err(format!("duplicate enum value '{value}'"));
        }
        values.push(EnumValue {
            value: value.to_string(),
            title,
        });
    }
    if values.is_empty() {
        return Err("enum needs at least one value; use Clear to remove it".into());
    }
    Ok(EnumSpec { values })
}

/// Definition of a scalar managed property to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddFieldSpec {
    pub key: String,
    pub field_type: ScalarFieldType,
    pub title: Option<String>,
    pub description: Option<String>,
    pub required: bool,
    pub searchable: bool,
    pub viewable: bool,
    pub user_editable: bool,
    pub enum_values: Option<EnumSpec>,
    pub default_value: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn enum_items_parse_and_round_trip() {
        let parsed = parse_enum_items(&[" new ", "done:All done: now", " "]).unwrap();
        assert_eq!(parsed.values[0].value, "new");
        assert_eq!(parsed.values[1].title.as_deref(), Some("All done: now"));
        assert_eq!(parse_enum_items(&parsed.to_items()).unwrap(), parsed);
    }

    #[test]
    fn enum_items_reject_empty_values_duplicates_and_empty_lists() {
        assert!(
            parse_enum_items(&[":Title"])
                .unwrap_err()
                .contains("empty value")
        );
        assert!(
            parse_enum_items(&["new", "new"])
                .unwrap_err()
                .contains("new")
        );
        assert!(parse_enum_items(&["", " "]).is_err());
    }

    #[test]
    fn scalar_types_map_to_value_shapes() {
        assert_eq!(ScalarFieldType::String.shape(), ValueShape::Text);
        assert_eq!(ScalarFieldType::Boolean.shape(), ValueShape::Bool);
        assert_eq!(ScalarFieldType::Number.shape(), ValueShape::Decimal);
        assert_eq!(
            ScalarFieldType::StringArray.shape(),
            ValueShape::Json(JsonShape::Array)
        );
    }

    fn sample_object() -> Value {
        json!({
            "name": "alpha_user",
            "iconClass": "fa fa-user",
            "schema": {
                "properties": {
                    "name": {"type": "string"},
                    "userName": {"type": "string"},
                    "custom_idProofingEligibility": {"type": "string"},
                    "x_custom_idProofingEligibility": {"type": "string"},
                    "custom_a": {"type": "string"},
                    "custom_ab": {"type": "string"},
                    "custom_": {"type": "string"},
                    "scriptish": {"type": "text/javascript", "source": "not a hook"}
                },
                "order": [
                    "name",
                    "userName",
                    "custom_idProofingEligibility",
                    "x_custom_idProofingEligibility",
                    "custom_a",
                    "custom_ab",
                    "custom_",
                    "scriptish",
                    "ghost"
                ],
                "required": ["name", "userName", "ghost"]
            },
            "onCreate": {
                "type": "text/javascript",
                "source": "create body",
                "globals": {"a": 1}
            },
            "onUpdate": {"type": "text/javascript", "source": "update body"},
            "onDelete": {"type": "text/javascript", "file": "roles/onDelete-roles.js"}
        })
    }

    fn get(fields: &[&str], include_hook_sources: bool) -> ManagedGetView {
        prepare_managed_get(
            sample_object(),
            "alpha_user",
            &fields.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
            include_hook_sources,
        )
        .unwrap()
    }

    fn property_keys(object: &Value) -> Vec<&str> {
        object
            .pointer("/schema/properties")
            .and_then(Value::as_object)
            .map(|properties| properties.keys().map(String::as_str).collect())
            .unwrap_or_default()
    }

    #[test]
    fn field_glob_literal_is_exact_not_substring() {
        assert!(field_glob_match("name", "name"));
        assert!(!field_glob_match("name", "userName"));
        assert!(!field_glob_match("name", "names"));
    }

    #[test]
    fn field_glob_star_is_anchored() {
        assert!(field_glob_match(
            "custom_idProofing*",
            "custom_idProofingEligibility"
        ));
        assert!(!field_glob_match(
            "custom_idProofing*",
            "x_custom_idProofingEligibility"
        ));
        assert!(field_glob_match(
            "*Proofing*",
            "custom_idProofingEligibility"
        ));
        assert!(field_glob_match(
            "*Proofing*",
            "x_custom_idProofingEligibility"
        ));
    }

    #[test]
    fn field_glob_question_is_exactly_one() {
        assert!(field_glob_match("custom_?", "custom_a"));
        assert!(!field_glob_match("custom_?", "custom_ab"));
        assert!(!field_glob_match("custom_?", "custom_"));
    }

    #[test]
    fn field_glob_other_characters_are_literal() {
        // A regex compile without escaping would treat `.` as "any character".
        assert!(field_glob_match("user.name", "user.name"));
        assert!(!field_glob_match("user.name", "userXname"));
        assert!(!field_glob_match("name+", "name"));
    }

    #[test]
    fn fields_literal_does_not_match_a_substring() {
        let view = get(&["name"], true);
        assert_eq!(property_keys(&view.object), vec!["name"]);
        assert_eq!(view.fields_shown, Some((1, 8)));
        assert!(view.unmatched_patterns.is_empty());
    }

    #[test]
    fn fields_prefix_glob_is_anchored_at_the_start() {
        let view = get(&["custom_idProofing*"], true);
        assert_eq!(
            property_keys(&view.object),
            vec!["custom_idProofingEligibility"]
        );
        assert!(
            !view.object["schema"]["properties"]
                .as_object()
                .unwrap()
                .contains_key("x_custom_idProofingEligibility")
        );
    }

    #[test]
    fn fields_unanchored_star_matches_either_side() {
        let view = get(&["*Proofing*"], true);
        assert_eq!(
            property_keys(&view.object),
            vec![
                "custom_idProofingEligibility",
                "x_custom_idProofingEligibility"
            ]
        );
    }

    #[test]
    fn fields_question_mark_is_exactly_one_character() {
        let view = get(&["custom_?"], true);
        assert_eq!(property_keys(&view.object), vec!["custom_a"]);
    }

    #[test]
    fn fields_or_keeps_both_matches_in_document_key_order() {
        // Patterns are reverse-alphabetical so walking the patterns to build
        // the result would emit userName then name. The document keeps its
        // own property and order-array sequence.
        let view = get(&["userName", "name"], true);
        assert_eq!(property_keys(&view.object), vec!["name", "userName"]);
        assert_eq!(view.object["schema"]["order"], json!(["name", "userName"]));
        assert_eq!(
            view.object["schema"]["required"],
            json!(["name", "userName"])
        );
        assert_eq!(view.object["iconClass"], json!("fa fa-user"));
        assert_eq!(
            view.object["onDelete"]["file"],
            json!("roles/onDelete-roles.js")
        );
    }

    #[test]
    fn fields_no_match_names_the_patterns_and_the_property_count() {
        let error = prepare_managed_get(
            sample_object(),
            "alpha_user",
            &["nope*".into(), "also_nope".into()],
            true,
        )
        .unwrap_err();
        assert!(error.contains("nope*"), "{error}");
        assert!(error.contains("also_nope"), "{error}");
        assert!(error.contains("alpha_user"), "{error}");
        assert!(error.contains("8 properties"), "{error}");
    }

    #[test]
    fn fields_partial_match_records_the_patterns_that_hit_nothing() {
        let view = get(&["name", "typo*", "userName"], true);
        assert_eq!(property_keys(&view.object), vec!["name", "userName"]);
        assert_eq!(view.unmatched_patterns, vec!["typo*"]);
        assert_eq!(view.fields_shown, Some((2, 8)));
    }

    #[test]
    fn get_omits_inline_hook_sources_and_leaves_file_hooks_alone() {
        let view = get(&[], false);
        assert_eq!(
            view.object["onCreate"]["source"],
            json!("<script: managed/alpha_user.onCreate>")
        );
        assert_eq!(view.object["onCreate"]["type"], json!("text/javascript"));
        assert_eq!(view.object["onCreate"]["globals"]["a"], json!(1));
        assert_eq!(
            view.object["onUpdate"]["source"],
            json!("<script: managed/alpha_user.onUpdate>")
        );
        assert_eq!(
            view.object["onDelete"],
            json!({"type": "text/javascript", "file": "roles/onDelete-roles.js"})
        );
        assert_eq!(
            view.object["schema"]["properties"]["scriptish"]["source"],
            json!("not a hook")
        );
        assert_eq!(view.elided_hooks, 2);
        assert!(view.fields_shown.is_none());
    }

    #[test]
    fn get_keeps_hook_sources_when_asked() {
        let view = get(&[], true);
        assert_eq!(view.object["onCreate"]["source"], json!("create body"));
        assert_eq!(view.object["onUpdate"]["source"], json!("update body"));
        assert_eq!(view.elided_hooks, 0);
    }
}

/// Input for deleting a managed property.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DeleteFieldSpec;

/// Requested replacement key for a managed property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameFieldSpec {
    pub new_key: String,
}

/// Presentation of one managed object for `aic managed get`.
#[derive(Debug)]
pub struct ManagedGetView {
    pub object: Value,
    /// `(shown, total)` when `--fields` ran. Absent when the document is unfiltered.
    pub fields_shown: Option<(usize, usize)>,
    pub unmatched_patterns: Vec<String>,
    pub elided_hooks: usize,
}

/// Filter properties and optionally replace inline hook sources with script-pull
/// references. Fails when `--fields` matched nothing, so the caller cannot
/// print an empty `properties` map and call that success.
pub fn prepare_managed_get(
    object: Value,
    object_name: &str,
    fields: &[String],
    include_hook_sources: bool,
) -> Result<ManagedGetView, String> {
    let (mut object, fields_shown, unmatched_patterns) = if fields.is_empty() {
        (object, None, Vec::new())
    } else {
        let filtered = filter_object_fields(object, fields);
        if filtered.shown == 0 {
            return Err(no_fields_match_message(object_name, filtered.total, fields));
        }
        (
            filtered.object,
            Some((filtered.shown, filtered.total)),
            filtered.unmatched_patterns,
        )
    };
    let elided_hooks = if include_hook_sources {
        0
    } else {
        elide_inline_hook_sources(&mut object, object_name)
    };
    Ok(ManagedGetView {
        object,
        fields_shown,
        unmatched_patterns,
        elided_hooks,
    })
}

struct FieldFilter {
    object: Value,
    total: usize,
    shown: usize,
    unmatched_patterns: Vec<String>,
}

/// Narrow `schema.properties` (and the matching `order` / `required` entries)
/// to keys matching any of `patterns`. Other object keys are left intact.
fn filter_object_fields(mut object: Value, patterns: &[String]) -> FieldFilter {
    let total = object
        .pointer("/schema/properties")
        .and_then(Value::as_object)
        .map_or(0, serde_json::Map::len);

    let unmatched_patterns = object
        .pointer("/schema/properties")
        .and_then(Value::as_object)
        .map(|properties| {
            patterns
                .iter()
                .filter(|pattern| !properties.keys().any(|key| field_glob_match(pattern, key)))
                .cloned()
                .collect()
        })
        .unwrap_or_else(|| patterns.to_vec());

    let keep: HashSet<String> = object
        .pointer("/schema/properties")
        .and_then(Value::as_object)
        .map(|properties| {
            properties
                .keys()
                .filter(|key| {
                    patterns
                        .iter()
                        .any(|pattern| field_glob_match(pattern, key))
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let shown = keep.len();

    if let Some(properties) = object
        .pointer_mut("/schema/properties")
        .and_then(Value::as_object_mut)
    {
        properties.retain(|key, _| keep.contains(key));
    }
    retain_listed_keys(&mut object, "/schema/order", &keep);
    retain_listed_keys(&mut object, "/schema/required", &keep);

    FieldFilter {
        object,
        total,
        shown,
        unmatched_patterns,
    }
}

fn retain_listed_keys(object: &mut Value, pointer: &str, keep: &HashSet<String>) {
    if let Some(list) = object.pointer_mut(pointer).and_then(Value::as_array_mut) {
        list.retain(|entry| entry.as_str().is_some_and(|key| keep.contains(key)));
    }
}

/// Case-sensitive, both-ends-anchored glob for `--fields`. `*` is any run
/// (including empty), `?` is exactly one character, everything else is
/// literal. No character classes — this is not AM URL-wildcard matching.
fn field_glob_match(pattern: &str, name: &str) -> bool {
    let pattern = pattern.as_bytes();
    let name = name.as_bytes();
    let (mut pi, mut ni) = (0usize, 0usize);
    let (mut star_p, mut star_n) = (None, 0usize);
    while ni < name.len() {
        if pi < pattern.len() && pattern[pi] == b'*' {
            star_p = Some(pi);
            pi += 1;
            star_n = ni;
            continue;
        }
        if pi < pattern.len() && (pattern[pi] == b'?' || pattern[pi] == name[ni]) {
            pi += 1;
            ni += 1;
            continue;
        }
        if let Some(star) = star_p {
            pi = star + 1;
            star_n += 1;
            ni = star_n;
            continue;
        }
        return false;
    }
    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }
    pi == pattern.len()
}

/// Replace each top-level inline hook's `source` with the argument `aic script
/// pull` takes. File-backed hooks and anything under `schema` are left alone.
fn elide_inline_hook_sources(object: &mut Value, object_name: &str) -> usize {
    let Some(map) = object.as_object_mut() else {
        return 0;
    };
    let mut count = 0;
    for (key, value) in map.iter_mut() {
        if key == "schema" {
            continue;
        }
        if is_inline_hook(value) {
            value["source"] = Value::String(format!("<script: managed/{object_name}.{key}>"));
            count += 1;
        }
    }
    count
}

fn no_fields_match_message(object_name: &str, total: usize, patterns: &[String]) -> String {
    let patterns = patterns
        .iter()
        .map(|pattern| format!("{pattern:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let noun = if total == 1 { "property" } else { "properties" };
    format!(
        "--fields matched no properties of {object_name} ({total} {noun}); patterns: {patterns}"
    )
}
