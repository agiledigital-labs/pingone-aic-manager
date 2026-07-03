//! Verified HTTP wrappers for IDM sync mappings and reconciliation.
//! See `docs/api/16-sync-mappings.md`.

use serde_json::{Value, json};
use url::form_urlencoded::Serializer;

use crate::scripts::sync_mapping::{WHOLE_MAPPING_SLOTS, is_inline_script};
use crate::{Error, Result};

const SYNC_PATH: &str = "/openidm/config/sync";
const RECON_PATH: &str = "/openidm/recon";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingSummary {
    pub name: String,
    pub source: String,
    pub target: String,
    pub inline_script_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconStatus {
    pub id: String,
    pub mapping: String,
    pub state: String,
    pub stage: String,
    pub stage_description: String,
    pub created: u64,
    pub updated: u64,
    pub deleted: u64,
    pub processed: u64,
    pub ended: Option<String>,
    pub duration: Option<i64>,
}

pub async fn list_mappings(tenant: &str) -> Result<Vec<MappingSummary>> {
    let doc = crate::aic::api::get(tenant, SYNC_PATH).await?;
    summaries_from_doc(&doc)
}

pub async fn start_recon(tenant: &str, mapping: &str, confirmed_prod: bool) -> Result<String> {
    let query = {
        let mut query = Serializer::new(String::new());
        query
            .append_pair("_action", "recon")
            .append_pair("mapping", mapping);
        query.finish()
    };
    let path = format!("{RECON_PATH}?{query}");
    let body = crate::aic::api::post(tenant, &path, json!({}), confirmed_prod).await?;
    string_field(&body, "_id")
}

pub async fn recon_status(tenant: &str, recon_id: &str) -> Result<ReconStatus> {
    let path = format!("{RECON_PATH}/{recon_id}");
    let body = crate::aic::api::get(tenant, &path).await?;
    parse_recon_status(&body)
}

pub fn state_is_terminal(state: &str) -> bool {
    !(state == "ACTIVE" || state.starts_with("ACTIVE_"))
}

fn summaries_from_doc(doc: &Value) -> Result<Vec<MappingSummary>> {
    let mappings = mappings_of(doc)?;
    let mut out = Vec::with_capacity(mappings.len());
    for (idx, mapping) in mappings.iter().enumerate() {
        out.push(MappingSummary {
            name: required_mapping_str(mapping, idx, "name")?,
            source: required_mapping_str(mapping, idx, "source")?,
            target: required_mapping_str(mapping, idx, "target")?,
            inline_script_count: inline_script_count(mapping)?,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn mappings_of(doc: &Value) -> Result<&Vec<Value>> {
    doc.get("mappings")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected /openidm/config/sync shape: {doc}"),
        })
}

fn required_mapping_str(mapping: &Value, idx: usize, key: &str) -> Result<String> {
    mapping
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("sync mapping at index {idx} has no string {key}: {mapping}"),
        })
}

fn inline_script_count(mapping: &Value) -> Result<usize> {
    let mut count = WHOLE_MAPPING_SLOTS
        .iter()
        .filter(|slot| mapping.get(**slot).is_some_and(is_inline_script))
        .count();

    let Some(properties) = mapping.get("properties") else {
        return Ok(count);
    };
    let properties = properties.as_array().ok_or_else(|| Error::Api {
        status: 0,
        body: format!("sync mapping properties is not an array: {mapping}"),
    })?;
    for property in properties {
        if property.get("transform").is_some_and(is_inline_script) {
            count += 1;
        }
        if property.get("condition").is_some_and(is_inline_script) {
            count += 1;
        }
    }
    Ok(count)
}

fn parse_recon_status(value: &Value) -> Result<ReconStatus> {
    Ok(ReconStatus {
        id: string_field(value, "_id")?,
        mapping: string_field(value, "mapping")?,
        state: string_field(value, "state")?,
        stage: string_field(value, "stage")?,
        stage_description: string_field(value, "stageDescription")?,
        created: optional_u64_path(value, &["progress", "target", "created"])?,
        updated: optional_u64_path(value, &["progress", "target", "updated"])?,
        deleted: optional_u64_path(value, &["progress", "target", "deleted"])?,
        processed: optional_u64_path(value, &["progress", "source", "existing", "processed"])?,
        ended: optional_string_field(value, "ended")?,
        duration: optional_i64_field(value, "duration")?,
    })
}

fn string_field(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("expected string field {key} in response: {value}"),
        })
}

fn optional_string_field(value: &Value, key: &str) -> Result<Option<String>> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_str()
            .map(|s| Some(s.to_string()))
            .ok_or_else(|| Error::Api {
                status: 0,
                body: format!("expected optional string field {key} in response: {value}"),
            }),
    }
}

fn optional_i64_field(value: &Value, key: &str) -> Result<Option<i64>> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_i64().map(Some).ok_or_else(|| Error::Api {
            status: 0,
            body: format!("expected optional integer field {key} in response: {value}"),
        }),
    }
}

fn optional_u64_path(value: &Value, path: &[&str]) -> Result<u64> {
    let mut cursor = value;
    for key in &path[..path.len().saturating_sub(1)] {
        match cursor {
            Value::Object(object) => match object.get(*key) {
                Some(next) => cursor = next,
                None => return Ok(0),
            },
            _ => {
                return Err(Error::Api {
                    status: 0,
                    body: format!(
                        "expected object before {} in response: {value}",
                        path.join(".")
                    ),
                });
            }
        }
    }

    let Some(last) = path.last() else {
        return Ok(0);
    };
    match cursor {
        Value::Object(object) => match object.get(*last) {
            Some(found) => found.as_u64().ok_or_else(|| Error::Api {
                status: 0,
                body: format!(
                    "expected unsigned integer at {} in response: {value}",
                    path.join(".")
                ),
            }),
            None => Ok(0),
        },
        _ => Err(Error::Api {
            status: 0,
            body: format!(
                "expected object before {} in response: {value}",
                path.join(".")
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn summaries_count_inline_scripts_and_skip_file_backed_slots() {
        let doc = json!({
            "_id": "sync",
            "mappings": [
                {
                    "name": "z_map",
                    "source": "managed/source",
                    "target": "managed/target",
                    "correlationQuery": [
                        {"type": "text/javascript", "source": "not a sync script slot"}
                    ],
                    "onUpdate": {"type": "text/javascript", "source": "target.name = source.name;"},
                    "onDelete": {"type": "text/javascript", "file": "sync/onDelete.js"},
                    "properties": [
                        {
                            "target": "name",
                            "source": "name",
                            "transform": {"type": "text/javascript", "source": "source"}
                        },
                        {
                            "target": "age",
                            "source": "age",
                            "condition": {"type": "application/javascript", "source": "true"},
                            "transform": {"type": "text/javascript", "file": "sync/age.js"}
                        }
                    ]
                },
                {
                    "name": "a_map",
                    "source": "system/ldap/account",
                    "target": "managed/user",
                    "validSource": {"type": "text/javascript", "source": "true"}
                }
            ]
        });

        let summaries = summaries_from_doc(&doc).unwrap();

        assert_eq!(
            summaries,
            vec![
                MappingSummary {
                    name: "a_map".into(),
                    source: "system/ldap/account".into(),
                    target: "managed/user".into(),
                    inline_script_count: 1,
                },
                MappingSummary {
                    name: "z_map".into(),
                    source: "managed/source".into(),
                    target: "managed/target".into(),
                    inline_script_count: 3,
                },
            ]
        );
    }

    #[test]
    fn recon_status_parses_numbers_without_touching_string_totals() {
        let status = json!({
            "_id": "recon-123",
            "mapping": "managedSource_managedTarget",
            "state": "SUCCESS",
            "stage": "COMPLETED_SUCCESS",
            "stageDescription": "reconciliation completed",
            "progress": {
                "source": {"existing": {"processed": 42, "total": "42"}},
                "target": {
                    "existing": {"processed": 20, "total": "?"},
                    "created": 2,
                    "unchanged": 30,
                    "updated": 7,
                    "deleted": 3,
                    "retried": 0
                }
            },
            "started": "2026-06-18T23:25:10.520Z",
            "ended": "2026-06-18T23:25:10.533Z",
            "duration": 13
        });

        let parsed = parse_recon_status(&status).unwrap();

        assert_eq!(
            parsed,
            ReconStatus {
                id: "recon-123".into(),
                mapping: "managedSource_managedTarget".into(),
                state: "SUCCESS".into(),
                stage: "COMPLETED_SUCCESS".into(),
                stage_description: "reconciliation completed".into(),
                created: 2,
                updated: 7,
                deleted: 3,
                processed: 42,
                ended: Some("2026-06-18T23:25:10.533Z".into()),
                duration: Some(13),
            }
        );
    }

    #[test]
    fn active_recon_status_without_progress_defaults_counters_to_zero() {
        let status = json!({
            "_id": "recon-123",
            "mapping": "managedSource_managedTarget",
            "state": "ACTIVE",
            "stage": "ACTIVE_QUERY_ENTRIES",
            "stageDescription": "querying source entries"
        });

        let parsed = parse_recon_status(&status).unwrap();

        assert_eq!(parsed.created, 0);
        assert_eq!(parsed.updated, 0);
        assert_eq!(parsed.deleted, 0);
        assert_eq!(parsed.processed, 0);
        assert_eq!(parsed.state, "ACTIVE");
    }

    #[test]
    fn active_states_are_not_terminal() {
        assert!(!state_is_terminal("ACTIVE"));
        assert!(!state_is_terminal("ACTIVE_QUERY_ENTRIES"));
        assert!(state_is_terminal("SUCCESS"));
        assert!(state_is_terminal("FAILED"));
    }
}
