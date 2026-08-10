//! Tenant-free input parsing, validation, and role privilege transforms.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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
