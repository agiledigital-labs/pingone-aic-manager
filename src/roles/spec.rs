//! Tenant-free input parsing, validation, and role privilege transforms.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{Error, Result};

pub const KNOWN_PERMISSIONS: [&str; 5] = ["VIEW", "CREATE", "UPDATE", "DELETE", "ACTION"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessFlag {
    pub attribute: String,
    pub read_only: bool,
}

impl FromStr for AccessFlag {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let Some((attribute, mode)) = value.rsplit_once(':') else {
            return Err(format!(
                "invalid --attr {value:?}; expected <name>:<ro|rw> (valid suffixes: ro, rw)"
            ));
        };
        if attribute.is_empty() {
            return Err(
                "invalid --attr: attribute name cannot be empty (valid suffixes: ro, rw)".into(),
            );
        }
        let read_only = match mode {
            "ro" => true,
            "rw" => false,
            _ => {
                return Err(format!(
                    "invalid --attr suffix {mode:?} in {value:?}; valid suffixes are ro and rw"
                ));
            }
        };
        Ok(Self {
            attribute: attribute.to_string(),
            read_only,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivilegeSpec {
    pub name: Option<String>,
    pub path: String,
    pub actions: Vec<String>,
    pub permissions: Vec<String>,
    pub access_flags: Vec<AccessFlag>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Privilege {
    pub name: String,
    pub path: String,
    pub actions: Vec<String>,
    pub permissions: Vec<String>,
    pub access_flags: Vec<AccessFlag>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrivilegeMerge {
    pub amendment: RoleAmendment,
    pub replaced: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoleAmendment {
    pub body: Value,
    pub revision: String,
}

/// Build the exact five-key privilege body required by IDM.
pub fn build_privilege(spec: PrivilegeSpec) -> Result<Privilege> {
    if spec.access_flags.is_empty() {
        return Err(Error::Config(
            "a role privilege requires at least one --attr <name>:<ro|rw>; AIC rejects an empty accessFlags list"
                .into(),
        ));
    }
    if spec.path.is_empty() {
        return Err(Error::Config("privilege --path cannot be empty".into()));
    }
    Ok(Privilege {
        name: spec.name.unwrap_or_else(|| spec.path.clone()),
        path: spec.path,
        actions: spec.actions,
        permissions: spec.permissions,
        access_flags: spec.access_flags,
    })
}

/// Unknown permission values are advisory because IDM publishes no enum.
pub fn unknown_permissions(permissions: &[String]) -> Vec<&str> {
    permissions
        .iter()
        .map(String::as_str)
        .filter(|permission| !KNOWN_PERMISSIONS.contains(permission))
        .collect()
}

/// Extract the managed-object name that can be checked against live schema.
pub fn object_name(path: &str) -> Result<&str> {
    let Some(name) = path.strip_prefix("managed/") else {
        return Err(Error::Config(format!(
            "privilege path {path:?} is not a managed-object path; expected managed/<object>"
        )));
    };
    if name.is_empty() || name.contains('/') {
        return Err(Error::Config(format!(
            "privilege path {path:?} must name exactly one object as managed/<object>"
        )));
    }
    Ok(name)
}

/// Why an explicit access flag is refused for a measured system field.
///
/// `None` means the name is not one of those fields, so the schema check
/// decides. Checked before the schema: `_id` is a property of some objects
/// (alpha_user) and absent from others, and neither case is what exposes it.
pub fn system_field_refusal(attribute: &str) -> Option<String> {
    let message = match attribute {
        "_id" => concat!(
            "attribute \"_id\" cannot be an access flag: IDM returns `_id` on ",
            "managed-object reads with no accessFlags entry for it. An explicit ",
            "flag is not how that field is exposed — omit `--attr _id`"
        ),
        "_rev" => concat!(
            "attribute \"_rev\" cannot be an access flag: IDM returns `_rev` on ",
            "managed-object reads with no accessFlags entry for it, and rejects ",
            "an explicit flag (INVALID_ACCESS_FLAG_ATTRIBUTE). Omit `--attr _rev`"
        ),
        "_ref" | "_refProperties" | "_relationship" | "_notification" | "_parent" | "_schema" => {
            return Some(format!(
                "attribute {attribute:?} cannot be an access flag: IDM rejects it \
                 (INVALID_ACCESS_FLAG_ATTRIBUTE) and does not take an accessFlags entry for it"
            ));
        }
        "_meta" => concat!(
            "attribute \"_meta\" cannot be an access flag: IDM stored one on ",
            "managed/alpha_user and rejected it on managed/alpha_organization ",
            "(INVALID_ACCESS_FLAG_ATTRIBUTE), so it is not a field this command sets"
        ),
        _ => return None,
    };
    Some(message.to_string())
}

/// Validate every access flag against the target object's live properties.
pub fn validate_attributes(object: &Value, flags: &[AccessFlag], path: &str) -> Result<()> {
    let properties = object
        .pointer("/schema/properties")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            Error::Config(format!(
                "managed object for privilege path {path:?} has no schema.properties object"
            ))
        })?;
    for flag in flags {
        if let Some(refusal) = system_field_refusal(&flag.attribute) {
            return Err(Error::Config(refusal));
        }
        if !properties.contains_key(&flag.attribute) {
            return Err(Error::Config(format!(
                "attribute {:?} is not a property of privilege path {path:?}",
                flag.attribute
            )));
        }
    }
    Ok(())
}

/// Replace the privilege at the same path, or append when the path is new.
/// Every other role field is preserved verbatim for the destructive PUT.
///
/// Order is deliberately not preserved: AIC does not store privileges in the
/// order they are sent (verified 2026-08-10 — a replace-in-place came back with
/// the surrounding privileges reordered, consistent with the LDAP-backed store
/// behind `ou=roles,ou=internal`). Reads are self-consistent, so callers may
/// display what they read, but nothing should depend on position.
pub fn merge_privilege(role: &Value, privilege: Privilege) -> Result<PrivilegeMerge> {
    let mut role = role.clone();
    let object = role
        .as_object_mut()
        .ok_or_else(|| Error::Config("internal role response is not an object".into()))?;
    let privileges = privileges_mut(object)?;
    let before = privileges.len();
    privileges
        .retain(|entry| entry.get("path").and_then(Value::as_str) != Some(privilege.path.as_str()));
    let replaced = privileges.len() != before;
    privileges.push(serde_json::to_value(privilege)?);
    Ok(PrivilegeMerge {
        amendment: prepare_amendment(role)?,
        replaced,
    })
}

/// A name and/or description change. `None` leaves that field alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleTextEdit {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// One field a text edit will replace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChange {
    pub field: &'static str,
    pub before: Option<String>,
    pub after: String,
}

/// The fields that differ from `role`. An equal value is not a change.
///
/// The write is a `PATCH` of these fields only. A `PUT` that omits
/// `privileges` empties them (measured 2026-11-14, and again on 2026-08-10),
/// so a name change must not be a full replace.
pub fn plan_text_edit(role: &Value, edit: &RoleTextEdit) -> Result<Vec<TextChange>> {
    if !role.is_object() {
        return Err(Error::Config(
            "internal role response is not an object".into(),
        ));
    }
    let mut changes = Vec::new();
    if let Some(name) = &edit.name {
        push_change(&mut changes, role, "name", name)?;
    }
    if let Some(description) = &edit.description {
        push_change(&mut changes, role, "description", description)?;
    }
    Ok(changes)
}

fn push_change(
    changes: &mut Vec<TextChange>,
    role: &Value,
    field: &'static str,
    after: &str,
) -> Result<()> {
    let before = role_string(role, field)?;
    if before.as_deref() != Some(after) {
        changes.push(TextChange {
            field,
            before,
            after: after.to_string(),
        });
    }
    Ok(())
}

/// IDM patch document. `field` + `operation`, not JSON Patch `path` + `op`:
/// the JSON Patch spelling is a 400 (measured 2026-11-14).
pub fn text_patch(changes: &[TextChange]) -> Value {
    Value::Array(
        changes
            .iter()
            .map(|change| {
                json!({
                    "operation": "replace",
                    "field": format!("/{}", change.field),
                    "value": change.after,
                })
            })
            .collect(),
    )
}

/// `_rev` for `If-Match`. A stale value is a 412 and writes nothing.
pub fn role_revision(role: &Value) -> Result<String> {
    role.get("_rev")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Error::Config("internal role response has no string `_rev`".into()))
}

/// The edit landed only if a later read shows it. The write response is not
/// evidence (`.ai/core.md` §5). Privileges are compared as a bag because a
/// write may reorder them.
pub fn confirm_text_edit(before: &Value, after: &Value, changes: &[TextChange]) -> Result<()> {
    if privilege_bag(before)? != privilege_bag(after)? {
        return Err(Error::Config(
            "read-back privileges differ from the role that was patched; the name and description were not confirmed"
                .into(),
        ));
    }
    for change in changes {
        let got = role_string(after, change.field)?;
        if got.as_deref() != Some(change.after.as_str()) {
            return Err(Error::Config(format!(
                "read-back {} is {}; the edit sent {} and was not confirmed",
                change.field,
                display_string(got.as_deref()),
                display_string(Some(&change.after)),
            )));
        }
    }
    for field in ["name", "description"] {
        if changes.iter().any(|change| change.field == field) {
            continue;
        }
        let left = role_string(before, field)?;
        let right = role_string(after, field)?;
        if left != right {
            return Err(Error::Config(format!(
                "read-back {field} changed from {} to {} though the edit did not set it",
                display_string(left.as_deref()),
                display_string(right.as_deref()),
            )));
        }
    }
    Ok(())
}

fn display_string(value: Option<&str>) -> String {
    match value {
        Some(value) => format!("{value:?}"),
        None => "<none>".to_string(),
    }
}

fn role_string(role: &Value, key: &str) -> Result<Option<String>> {
    match role.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(other) => Err(Error::Config(format!(
            "internal role `{key}` is {other}, not a string"
        ))),
    }
}

fn privilege_bag(role: &Value) -> Result<Vec<String>> {
    let Some(privileges) = role.get("privileges") else {
        return Ok(Vec::new());
    };
    let privileges = privileges
        .as_array()
        .ok_or_else(|| Error::Config("internal role `privileges` is not an array".into()))?;
    let mut bag = privileges
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    bag.sort();
    Ok(bag)
}

/// Remove every duplicate of one path while preserving the rest of the role.
pub fn remove_privilege(role: &Value, path: &str) -> Result<(RoleAmendment, bool)> {
    let mut role = role.clone();
    let object = role
        .as_object_mut()
        .ok_or_else(|| Error::Config("internal role response is not an object".into()))?;
    let privileges = privileges_mut(object)?;
    let before = privileges.len();
    privileges.retain(|entry| entry.get("path").and_then(Value::as_str) != Some(path));
    let removed = privileges.len() != before;
    Ok((prepare_amendment(role)?, removed))
}

/// Separate the read revision from a full-replacement body.
///
/// A bare read is **not** writable as-is: IDM returns `temporalConstraints` and
/// then rejects it on write with `403 "Policy validation failed"`, even when the
/// value is the empty array the read itself produced. Dropping it is therefore
/// mandatory, not tidiness (`docs/api/18-internal-roles.md`).
///
/// `_id` and `_rev` are accepted in the body (verified 2026-08-10), so removing
/// them is a choice: the revision that counts travels in `If-Match`, and leaving
/// a stale copy in the JSON would invite a reader to trust the wrong one.
/// `condition` is writable and is deliberately preserved.
fn prepare_amendment(mut role: Value) -> Result<RoleAmendment> {
    let object = role
        .as_object_mut()
        .ok_or_else(|| Error::Config("internal role response is not an object".into()))?;
    let revision = object
        .remove("_rev")
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| Error::Config("internal role response has no string `_rev`".into()))?;
    object.remove("_id");
    object.remove("temporalConstraints");
    Ok(RoleAmendment {
        body: role,
        revision,
    })
}

fn privileges_mut(object: &mut Map<String, Value>) -> Result<&mut Vec<Value>> {
    let privileges = object
        .entry("privileges")
        .or_insert_with(|| Value::Array(Vec::new()));
    privileges
        .as_array_mut()
        .ok_or_else(|| Error::Config("internal role `privileges` is not an array".into()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_attribute_modes_table() {
        for (input, attribute, read_only) in [
            ("mail:rw", "mail", false),
            ("userName:ro", "userName", true),
        ] {
            let parsed: AccessFlag = input.parse().unwrap();
            assert_eq!(parsed.attribute, attribute);
            assert_eq!(parsed.read_only, read_only);
        }
    }

    #[test]
    fn invalid_attribute_suffix_names_both_valid_suffixes() {
        let error = "mail:write".parse::<AccessFlag>().unwrap_err();
        assert!(error.contains("ro"));
        assert!(error.contains("rw"));
    }

    #[test]
    fn privilege_body_has_all_five_mandatory_keys() {
        let privilege = build_privilege(PrivilegeSpec {
            name: None,
            path: "managed/alpha_user".into(),
            actions: Vec::new(),
            permissions: vec!["VIEW".into(), "UPDATE".into()],
            access_flags: vec!["mail:rw".parse().unwrap()],
        })
        .unwrap();
        let body = serde_json::to_value(privilege).unwrap();

        assert_eq!(body["name"], json!("managed/alpha_user"));
        for key in ["name", "path", "actions", "permissions", "accessFlags"] {
            assert!(body.get(key).is_some(), "missing mandatory key {key}");
        }
    }

    #[test]
    fn merge_by_path_replaces_without_duplicate() {
        let role = json!({
            "_id": "support",
            "_rev": "rev-1",
            "name": "Support",
            "description": "kept",
            "privileges": [
                {"name":"old", "path":"managed/alpha_user", "actions":[], "permissions":["VIEW"], "accessFlags":[{"attribute":"mail", "readOnly":true}]},
                {"name":"roles", "path":"managed/alpha_role", "actions":[], "permissions":["VIEW"], "accessFlags":[{"attribute":"name", "readOnly":true}]}
            ]
        });
        let replacement = build_privilege(PrivilegeSpec {
            name: Some("new".into()),
            path: "managed/alpha_user".into(),
            actions: vec!["reset".into()],
            permissions: vec!["UPDATE".into()],
            access_flags: vec!["mail:rw".parse().unwrap()],
        })
        .unwrap();

        let merged = merge_privilege(&role, replacement).unwrap();
        let privileges = merged.amendment.body["privileges"].as_array().unwrap();
        assert!(merged.replaced);
        assert_eq!(privileges.len(), 2);
        // Asserted by path, not by index: AIC does not store privileges in the
        // order they are sent, so an index-based assertion would be pinning down
        // the backend's ordering rather than this transform's behaviour.
        let by_path = |path: &str| {
            privileges
                .iter()
                .find(|entry| entry["path"] == json!(path))
                .unwrap_or_else(|| panic!("no privilege for {path}"))
        };
        assert_eq!(by_path("managed/alpha_user")["name"], json!("new"));
        assert_eq!(
            by_path("managed/alpha_user")["permissions"],
            json!(["UPDATE"])
        );
        assert_eq!(by_path("managed/alpha_role")["name"], json!("roles"));
    }

    #[test]
    fn merge_preserves_role_name_and_description() {
        let role = json!({
            "_id": "chosen-id",
            "_rev": "rev-1",
            "name": "Display name",
            "description": "Do not wipe",
            "privileges": []
        });
        let privilege = build_privilege(PrivilegeSpec {
            name: None,
            path: "managed/alpha_user".into(),
            actions: Vec::new(),
            permissions: vec!["VIEW".into()],
            access_flags: vec!["mail:ro".parse().unwrap()],
        })
        .unwrap();

        let merged = merge_privilege(&role, privilege).unwrap();
        assert_eq!(merged.amendment.body["name"], role["name"]);
        assert_eq!(merged.amendment.body["description"], role["description"]);
        assert_eq!(merged.amendment.revision, "rev-1");
        assert!(merged.amendment.body.get("_rev").is_none());
        assert!(merged.amendment.body.get("_id").is_none());
    }

    /// A bare read carries `temporalConstraints`, which IDM then rejects on
    /// write with a 403 even when it is the empty array the read produced. It
    /// must be dropped; `condition` is writable and must survive.
    #[test]
    fn amendment_drops_temporal_constraints_and_keeps_condition() {
        let bare_read = json!({
            "_id": "chosen-id",
            "_rev": "rev-1",
            "name": "Display name",
            "description": "Keep me",
            "privileges": [],
            "condition": null,
            "temporalConstraints": []
        });
        let privilege = build_privilege(PrivilegeSpec {
            name: None,
            path: "managed/alpha_user".into(),
            actions: Vec::new(),
            permissions: vec!["VIEW".into()],
            access_flags: vec!["mail:ro".parse().unwrap()],
        })
        .unwrap();

        let body = merge_privilege(&bare_read, privilege)
            .unwrap()
            .amendment
            .body;
        assert!(
            body.get("temporalConstraints").is_none(),
            "temporalConstraints must be stripped or the PUT 403s"
        );
        assert!(body.get("condition").is_some(), "condition is writable");

        let (removal, _) = remove_privilege(&bare_read, "managed/alpha_user").unwrap();
        assert!(
            removal.body.get("temporalConstraints").is_none(),
            "the removal path needs the same strip"
        );
    }

    #[test]
    fn text_edit_patches_only_fields_that_differ() {
        let role = json!({
            "_id": "zzemuR3N",
            "_rev": "rev-1",
            "name": "old-name",
            "description": "old-desc",
            "privileges": [{"name":"p","path":"managed/alpha_user","actions":[],"permissions":["VIEW"],"accessFlags":[{"attribute":"userName","readOnly":true}]}],
            "condition": null,
            "temporalConstraints": []
        });
        let same = plan_text_edit(
            &role,
            &RoleTextEdit {
                name: Some("old-name".into()),
                description: None,
            },
        )
        .unwrap();
        assert!(same.is_empty());

        let changes = plan_text_edit(
            &role,
            &RoleTextEdit {
                name: Some("new-name".into()),
                description: Some("new-desc".into()),
            },
        )
        .unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].before.as_deref(), Some("old-name"));
        assert_eq!(changes[0].after, "new-name");
        let patch = text_patch(&changes);
        assert_eq!(
            patch,
            json!([
                {"operation": "replace", "field": "/name", "value": "new-name"},
                {"operation": "replace", "field": "/description", "value": "new-desc"}
            ])
        );
        assert!(patch[0].get("op").is_none(), "JSON Patch spelling is a 400");
        assert_eq!(role_revision(&role).unwrap(), "rev-1");

        let mut after = role.clone();
        after["name"] = json!("new-name");
        after["description"] = json!("new-desc");
        after["_rev"] = json!("rev-2");
        confirm_text_edit(&role, &after, &changes).unwrap();

        let wiped = json!({
            "_id": "zzemuR3N",
            "_rev": "rev-2",
            "name": "new-name",
            "description": "new-desc",
            "privileges": []
        });
        let error = confirm_text_edit(&role, &wiped, &changes).unwrap_err();
        assert!(error.to_string().contains("privileges"));

        let ignored = after.clone();
        let error = confirm_text_edit(
            &role,
            &ignored,
            &[TextChange {
                field: "description",
                before: Some("old-desc".into()),
                after: "not-what-came-back".into(),
            }],
        )
        .unwrap_err();
        assert!(error.to_string().contains("not confirmed"));
    }

    #[test]
    fn system_fields_are_refused_even_when_the_schema_lists_them() {
        let object = json!({
            "schema": {"properties": {"userName": {"type": "string"}, "_id": {"type": "string"}}}
        });
        let flags = |names: &[&str]| {
            names
                .iter()
                .map(|name| AccessFlag {
                    attribute: (*name).to_string(),
                    read_only: true,
                })
                .collect::<Vec<_>>()
        };
        validate_attributes(&object, &flags(&["userName"]), "managed/alpha_user").unwrap();

        for (attribute, needle) in [
            ("_id", "returns `_id`"),
            ("_rev", "returns `_rev`"),
            ("_ref", "INVALID_ACCESS_FLAG_ATTRIBUTE"),
            ("_refProperties", "INVALID_ACCESS_FLAG_ATTRIBUTE"),
            ("_meta", "alpha_organization"),
        ] {
            let error = validate_attributes(&object, &flags(&[attribute]), "managed/alpha_user")
                .unwrap_err()
                .to_string();
            assert!(error.contains(needle), "{attribute} refusal was {error}");
            assert!(
                !error.contains("not a property"),
                "{attribute} fell through to the schema check"
            );
        }

        let missing = validate_attributes(&object, &flags(&["mail"]), "managed/alpha_user")
            .unwrap_err()
            .to_string();
        assert!(missing.contains("not a property"));
    }

    #[test]
    fn empty_access_flags_are_refused() {
        let error = build_privilege(PrivilegeSpec {
            name: None,
            path: "managed/alpha_user".into(),
            actions: Vec::new(),
            permissions: vec!["VIEW".into()],
            access_flags: Vec::new(),
        })
        .unwrap_err();
        assert!(error.to_string().contains("at least one --attr"));
        assert!(error.to_string().contains("accessFlags"));
    }
}
