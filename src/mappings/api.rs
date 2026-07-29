//! Verified HTTP wrappers for IDM sync mappings and reconciliation.
//! See `docs/api/16-sync-mappings.md`.

use serde_json::{Value, json};
use url::form_urlencoded::Serializer;

use crate::scripts::sync_mapping::{WHOLE_MAPPING_SLOTS, is_inline_script};
use crate::{Error, Result};

const SYNC_PATH: &str = "/openidm/config/sync";
const RECON_PATH: &str = "/openidm/recon";
const QUEUE_PATH: &str = "/openidm/sync/queue";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingSummary {
    pub name: String,
    pub source: String,
    pub target: String,
    pub inline_script_count: usize,
    pub queued_sync: Option<QueuedSync>,
}

/// Per-mapping asynchronous implicit-sync configuration. Missing wire fields
/// default because older mappings and hand-edited config are not uniform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedSync {
    pub enabled: bool,
    pub page_size: u64,
    pub polling_interval_ms: u64,
    pub max_queue_size: u64,
    pub max_retries: u64,
    pub retry_delay_ms: u64,
    pub post_retry_action: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueItem {
    pub id: String,
    pub mapping: String,
    pub node_id: Option<String>,
    pub create_date: Option<String>,
    pub sync_action: Option<String>,
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

/// Start a single-source reconciliation. With `wait`, IDM returns the final
/// synchronous response body, including the otherwise-hidden failure reason.
pub async fn start_recon_by_id(
    tenant: &str,
    mapping: &str,
    source_id: &str,
    wait: bool,
    confirmed_prod: bool,
) -> Result<Value> {
    let mut query = Serializer::new(String::new());
    query
        .append_pair("_action", "reconById")
        .append_pair("mapping", mapping)
        .append_pair("ids", source_id);
    if wait {
        query.append_pair("waitForCompletion", "true");
    }
    let path = format!("{RECON_PATH}?{}", query.finish());
    crate::aic::api::post(tenant, &path, json!({}), confirmed_prod).await
}

pub async fn recon_list(tenant: &str) -> Result<Vec<ReconStatus>> {
    let body = crate::aic::api::get(tenant, RECON_PATH).await?;
    parse_recon_list(&body)
}

/// Query a queue total. IDM silently downgrades EXACT to ESTIMATE; `None`
/// represents its -1/missing unknown sentinel, never a count of -1.
pub async fn queue_depth(tenant: &str, mapping: Option<&str>) -> Result<Option<u64>> {
    queue_count(tenant, mapping_filter(mapping).as_deref()).await
}

/// Count an equality-filtered queue dimension. This must not be used for
/// presence filters (`nodeId pr`), whose total is collection-wide in IDM.
pub async fn queue_count(tenant: &str, filter: Option<&str>) -> Result<Option<u64>> {
    let path = queue_query_path(filter, 1, None, "_id", true);
    let body = crate::aic::api::get(tenant, &path).await?;
    parse_queue_depth(&body)
}

pub async fn queue_boundary(
    tenant: &str,
    mapping: Option<&str>,
    newest: bool,
) -> Result<Option<QueueItem>> {
    let sort = if newest { "-createDate" } else { "createDate" };
    let path = queue_query_path(
        mapping_filter(mapping).as_deref(),
        1,
        Some(sort),
        "_id,createDate,mapping",
        false,
    );
    let body = crate::aic::api::get(tenant, &path).await?;
    parse_queue_items(&body).map(|mut items| items.pop())
}

/// Run the same ordered projection query used by the poller's claim phase.
pub async fn queue_claim_probe(tenant: &str, mapping: Option<&str>, page_size: u64) -> Result<()> {
    let path = queue_query_path(
        mapping_filter(mapping).as_deref(),
        page_size,
        Some("createDate"),
        "_id",
        false,
    );
    crate::aic::api::get(tenant, &path).await.map(|_| ())
}

/// Sample the head of the queue. Bounded on purpose: claimed-vs-unclaimed
/// cannot be counted, because a `nodeId pr` total is collection-wide.
pub async fn queue_sample(tenant: &str, mapping: Option<&str>) -> Result<Vec<QueueItem>> {
    let path = queue_query_path(
        mapping_filter(mapping).as_deref(),
        1000,
        Some("createDate"),
        "_id,nodeId,createDate,mapping,syncAction",
        false,
    );
    let body = crate::aic::api::get(tenant, &path).await?;
    parse_queue_items(&body)
}

fn mapping_filter(mapping: Option<&str>) -> Option<String> {
    mapping.map(|name| format!("mapping eq \"{name}\""))
}

fn queue_query_path(
    filter: Option<&str>,
    page_size: u64,
    sort: Option<&str>,
    fields: &str,
    count_policy: bool,
) -> String {
    let mut query = Serializer::new(String::new());
    query.append_pair("_queryFilter", filter.unwrap_or("true"));
    query.append_pair("_pageSize", &page_size.to_string());
    if count_policy {
        query.append_pair("_totalPagedResultsPolicy", "EXACT");
    }
    if let Some(sort) = sort {
        query.append_pair("_sortKeys", sort);
    }
    query.append_pair("_fields", fields);
    format!("{QUEUE_PATH}?{}", query.finish())
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
            queued_sync: queued_sync(mapping),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn queued_sync(mapping: &Value) -> Option<QueuedSync> {
    let value = mapping.get("queuedSync")?.as_object()?;
    Some(QueuedSync {
        enabled: value
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        page_size: value.get("pageSize").and_then(Value::as_u64).unwrap_or(0),
        polling_interval_ms: value
            .get("pollingInterval")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        max_queue_size: value
            .get("maxQueueSize")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        max_retries: value.get("maxRetries").and_then(Value::as_u64).unwrap_or(0),
        retry_delay_ms: value.get("retryDelay").and_then(Value::as_u64).unwrap_or(0),
        post_retry_action: value
            .get("postRetryAction")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
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

pub fn parse_recon_status(value: &Value) -> Result<ReconStatus> {
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

/// Parse IDM's recent reconciliation envelope. An ACTIVE item may not have a
/// progress object yet, which `parse_recon_status` intentionally tolerates.
pub fn parse_recon_list(value: &Value) -> Result<Vec<ReconStatus>> {
    let runs = value
        .get("reconciliations")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected /openidm/recon list shape: {value}"),
        })?;
    let mut parsed = runs
        .iter()
        .map(parse_recon_status)
        .collect::<Result<Vec<_>>>()?;
    parsed.reverse();
    Ok(parsed)
}

pub fn parse_queue_depth(value: &Value) -> Result<Option<u64>> {
    match value.get("totalPagedResults") {
        None | Some(Value::Null) => Ok(None),
        Some(total) => match total.as_i64() {
            Some(total) if total >= 0 => Ok(Some(total as u64)),
            Some(_) => Ok(None),
            None => Err(Error::Api {
                status: 0,
                body: format!("expected integer totalPagedResults in response: {value}"),
            }),
        },
    }
}

pub fn parse_queue_items(value: &Value) -> Result<Vec<QueueItem>> {
    let items = value
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected /openidm/sync/queue shape: {value}"),
        })?;
    items
        .iter()
        .map(|item| {
            Ok(QueueItem {
                id: string_field(item, "_id")?,
                mapping: item
                    .get("mapping")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                node_id: optional_string_field(item, "nodeId")?,
                create_date: optional_string_field(item, "createDate")?,
                sync_action: optional_string_field(item, "syncAction")?,
            })
        })
        .collect()
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
                    queued_sync: None,
                },
                MappingSummary {
                    name: "z_map".into(),
                    source: "managed/source".into(),
                    target: "managed/target".into(),
                    inline_script_count: 3,
                    queued_sync: None,
                },
            ]
        );
    }

    #[test]
    fn summaries_parse_present_absent_partial_and_disabled_queued_sync() {
        let doc = json!({"mappings": [
            {"name":"absent", "source":"managed/a", "target":"managed/b"},
            {"name":"disabled", "source":"managed/a", "target":"managed/b",
             "queuedSync":{"enabled":false,"pageSize":20,"pollingInterval":1000}},
            {"name":"partial", "source":"managed/a", "target":"managed/b",
             "queuedSync":{"enabled":true,"pageSize":10}},
            {"name":"present", "source":"managed/a", "target":"managed/b",
             "queuedSync":{"enabled":true,"pageSize":100,"pollingInterval":1000,
             "maxQueueSize":200,"maxRetries":5,"retryDelay":300,"postRetryAction":"logged-ignore"}}
        ]});
        let summaries = summaries_from_doc(&doc).unwrap();
        assert_eq!(summaries[0].queued_sync, None);
        assert!(!summaries[1].queued_sync.as_ref().unwrap().enabled);
        assert_eq!(
            summaries[2]
                .queued_sync
                .as_ref()
                .unwrap()
                .polling_interval_ms,
            0
        );
        assert_eq!(
            summaries[3].queued_sync.as_ref().unwrap().post_retry_action,
            "logged-ignore"
        );
    }

    #[test]
    fn queue_depth_treats_negative_and_missing_totals_as_unknown() {
        assert_eq!(
            parse_queue_depth(&json!({"totalPagedResults": 12})).unwrap(),
            Some(12)
        );
        assert_eq!(
            parse_queue_depth(&json!({"totalPagedResults": -1})).unwrap(),
            None
        );
        assert_eq!(parse_queue_depth(&json!({})).unwrap(), None);
    }

    #[test]
    fn recon_list_parses_newest_first_and_active_without_progress() {
        let list = json!({"_id":"recon","reconciliations":[
            {"_id":"old","mapping":"m","state":"SUCCESS","stage":"COMPLETED_SUCCESS","stageDescription":"done","duration":10},
            {"_id":"new","mapping":"m","state":"ACTIVE","stage":"ACTIVE_QUERY_ENTRIES","stageDescription":"running"}
        ]});
        let parsed = parse_recon_list(&list).unwrap();
        assert_eq!(parsed[0].id, "new");
        assert_eq!(parsed[0].processed, 0);
        assert_eq!(parsed[1].id, "old");
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
