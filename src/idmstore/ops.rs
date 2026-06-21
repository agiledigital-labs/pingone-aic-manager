//! Background work for the IDM record store tab.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use chrono::{SecondsFormat, Utc};
use futures::{StreamExt, TryStreamExt, stream};
use rusqlite::Connection;
use serde_json::Value;

use crate::app::App;
use crate::idmstore::state::{ObjectStatus, ObjectSyncResult, SyncMode, SyncReport};
use crate::idmstore::{api, db, state};
use crate::{Error, Result};

const OBJECT_CONCURRENCY: usize = 4;

pub fn refresh(_app: &mut App, _force: bool) {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdDiff {
    pub creates: Vec<String>,
    pub deletes: Vec<String>,
}

#[derive(Debug, Clone)]
struct SyncObject {
    name: String,
    schema: Value,
}

pub async fn sync_tenant(tenant: &str, requested: &[String]) -> Result<SyncReport> {
    let managed = crate::managed::api::get_managed(tenant).await?;
    let objects = select_objects(&managed, requested)?;
    drop(open_store(tenant)?);

    let mut results = stream::iter(objects)
        .map(|object| async move {
            let mut conn = db::open(state::store_path(tenant))?;
            sync_object(&mut conn, tenant, &object).await
        })
        .buffer_unordered(OBJECT_CONCURRENCY)
        .try_collect::<Vec<_>>()
        .await?;
    results.sort_by(|left, right| left.object.cmp(&right.object));

    Ok(SyncReport {
        tenant: tenant.to_string(),
        store_path: state::store_path(tenant),
        objects: results,
    })
}

pub fn status(tenant: &str) -> Result<Vec<ObjectStatus>> {
    let path = state::store_path(tenant);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let conn = db::open_readonly(&path)?;
    db::list_sync_states(&conn)?
        .into_iter()
        .map(|sync| {
            Ok(ObjectStatus {
                rows: db::object_row_count(&conn, &sync.object)?,
                object: sync.object,
                incremental_supported: sync.incremental_supported,
                watermark: sync.watermark,
                last_full_sync: sync.last_full_sync,
            })
        })
        .collect()
}

pub fn syncable_object_names(doc: &Value) -> Result<Vec<String>> {
    let mut names = crate::managed::api::objects(doc)?
        .iter()
        .filter_map(|object| object.get("name").and_then(Value::as_str))
        .filter(|name| is_syncable_object_name(name))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    names.sort();
    Ok(names)
}

pub fn is_syncable_object_name(name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    !lower.ends_with("meta")
        && !lower.starts_with('_')
        && !lower.starts_with("internal")
        && !lower.starts_with("system")
}

pub fn diff_ids(local: &BTreeSet<String>, remote: &BTreeSet<String>) -> IdDiff {
    IdDiff {
        creates: remote.difference(local).cloned().collect(),
        deletes: local.difference(remote).cloned().collect(),
    }
}

fn open_store(tenant: &str) -> Result<Connection> {
    fs::create_dir_all(state::store_dir())?;
    Ok(db::open(state::store_path(tenant))?)
}

fn select_objects(doc: &Value, requested: &[String]) -> Result<Vec<SyncObject>> {
    let mut objects = BTreeMap::new();
    for object in crate::managed::api::objects(doc)? {
        let Some(name) = object.get("name").and_then(Value::as_str) else {
            continue;
        };
        if is_syncable_object_name(name) {
            objects.insert(
                name.to_string(),
                SyncObject {
                    name: name.to_string(),
                    schema: object.clone(),
                },
            );
        }
    }

    if requested.is_empty() {
        return Ok(objects.into_values().collect());
    }

    let mut selected = Vec::with_capacity(requested.len());
    for name in requested {
        let object = objects.get(name).cloned().ok_or_else(|| {
            Error::Config(format!(
                "managed object '{name}' is not syncable or is not present on this tenant"
            ))
        })?;
        selected.push(object);
    }
    Ok(selected)
}

async fn sync_object(
    conn: &mut Connection,
    tenant: &str,
    object: &SyncObject,
) -> Result<ObjectSyncResult> {
    let stored = db::read_sync_state(conn, &object.name)?;
    let incremental_supported = match &stored {
        Some(state) => state.incremental_supported,
        None => api::probe_incremental_supported(tenant, &object.name).await?,
    };

    if incremental_supported {
        if let Some(state) = stored.as_ref().filter(|state| state.watermark.is_some()) {
            return incremental_sync(conn, tenant, object, state).await;
        }
    }

    full_sync(conn, tenant, object, incremental_supported).await
}

async fn full_sync(
    conn: &mut Connection,
    tenant: &str,
    object: &SyncObject,
    incremental_supported: bool,
) -> Result<ObjectSyncResult> {
    let records = fetch_all_records(tenant, &object.name, incremental_supported).await?;
    let remote_ids = ids_from_records(&records);
    let local_ids = db::local_ids(conn, &object.name)?;
    let deletes = diff_ids(&local_ids, &remote_ids).deletes;
    let overrides = infer_array_overrides(&object.schema, &records);
    let store = db::ObjectStore::new(&object.name, &object.schema, &overrides)?;

    db::create_schema(conn, &store)?;
    db::upsert_many(conn, &store, records.iter())?;
    db::delete_records_by_id(conn, &object.name, &deletes)?;

    let last_full_sync = now_iso();
    let watermark = if incremental_supported {
        max_record_watermark(&records)
    } else {
        None
    };
    db::write_sync_state(
        conn,
        &db::SyncState {
            object: object.name.clone(),
            incremental_supported,
            watermark: watermark.clone(),
            last_full_sync: Some(last_full_sync.clone()),
        },
    )?;

    Ok(ObjectSyncResult {
        object: object.name.clone(),
        mode: SyncMode::Full,
        incremental_supported,
        fetched: records.len(),
        upserted: records.len(),
        deleted: deletes.len(),
        rows: db::object_row_count(conn, &object.name)?,
        watermark,
        last_full_sync: Some(last_full_sync),
    })
}

async fn incremental_sync(
    conn: &mut Connection,
    tenant: &str,
    object: &SyncObject,
    stored: &db::SyncState,
) -> Result<ObjectSyncResult> {
    let watermark = stored
        .watermark
        .as_deref()
        .ok_or_else(|| Error::Config(format!("{} has no sync watermark", object.name)))?;
    let changes = api::list_changed_meta(tenant, &object.name, watermark).await?;
    let meta_ids = changes
        .iter()
        .map(|change| change.meta_id.clone())
        .collect::<Vec<_>>();
    let changed_records = db::record_ids_for_meta_ids(conn, &object.name, &meta_ids)?;

    let remote_ids = api::list_record_ids(tenant, &object.name)
        .await?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let local_ids = db::local_ids(conn, &object.name)?;
    let diff = diff_ids(&local_ids, &remote_ids);

    let mut fetch_ids = BTreeSet::new();
    for id in changed_records.values() {
        if remote_ids.contains(id) {
            fetch_ids.insert(id.clone());
        }
    }
    fetch_ids.extend(diff.creates.iter().cloned());

    let records = fetch_records_by_id(tenant, &object.name, &fetch_ids, true).await?;
    let overrides = infer_array_overrides(&object.schema, &records);
    let store = db::ObjectStore::new(&object.name, &object.schema, &overrides)?;
    db::create_schema(conn, &store)?;
    db::upsert_many(conn, &store, records.iter())?;
    db::delete_records_by_id(conn, &object.name, &diff.deletes)?;

    let next_watermark = advance_watermark(watermark, &changes, &records);
    db::write_sync_state(
        conn,
        &db::SyncState {
            object: object.name.clone(),
            incremental_supported: true,
            watermark: Some(next_watermark.clone()),
            last_full_sync: stored.last_full_sync.clone(),
        },
    )?;

    Ok(ObjectSyncResult {
        object: object.name.clone(),
        mode: SyncMode::Incremental,
        incremental_supported: true,
        fetched: records.len(),
        upserted: records.len(),
        deleted: diff.deletes.len(),
        rows: db::object_row_count(conn, &object.name)?,
        watermark: Some(next_watermark),
        last_full_sync: stored.last_full_sync.clone(),
    })
}

async fn fetch_all_records(tenant: &str, object: &str, include_meta: bool) -> Result<Vec<Value>> {
    let tenant = tenant.to_string();
    let object = object.to_string();
    let fields = include_meta.then_some(api::USER_RECORD_FIELDS);
    api::collect_cursor_pages(|cookie| {
        let tenant = tenant.clone();
        let object = object.clone();
        async move { api::list_records_cursor_page(&tenant, &object, cookie.as_deref(), fields).await }
    })
    .await
}

async fn fetch_records_by_id(
    tenant: &str,
    object: &str,
    ids: &BTreeSet<String>,
    include_meta: bool,
) -> Result<Vec<Value>> {
    let tenant = tenant.to_string();
    let object = object.to_string();
    let records = stream::iter(ids.iter().cloned())
        .map(|id| {
            let tenant = tenant.clone();
            let object = object.clone();
            async move { api::get_record(&tenant, &object, &id, include_meta).await }
        })
        .buffer_unordered(api::MAX_CONCURRENCY)
        .try_collect::<Vec<_>>()
        .await?;
    Ok(records.into_iter().flatten().collect())
}

fn ids_from_records(records: &[Value]) -> BTreeSet<String> {
    records
        .iter()
        .filter_map(|record| record.get("_id").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

fn infer_array_overrides(schema: &Value, records: &[Value]) -> db::ArrayColumnOverrides {
    let mut overrides = db::ArrayColumnOverrides::new();
    let Some(properties) = schema
        .pointer("/schema/properties")
        .and_then(Value::as_object)
    else {
        return overrides;
    };

    for (property, definition) in properties {
        if !is_undeclared_array(definition) {
            continue;
        }
        let mut sample = Vec::new();
        for record in records {
            let Some(elements) = record.get(property).and_then(Value::as_array) else {
                continue;
            };
            for element in elements {
                sample.push(element.clone());
                if sample.len() >= 256 {
                    break;
                }
            }
            if sample.len() >= 256 {
                break;
            }
        }
        let columns = db::infer_columns(&sample);
        if !columns.is_empty() {
            overrides.insert(property.clone(), columns);
        }
    }

    overrides
}

fn is_undeclared_array(definition: &Value) -> bool {
    if type_name(definition) != Some("array") {
        return false;
    }
    let items = definition.get("items").unwrap_or(&Value::Null);
    !crate::managed::state::is_relationship_property(definition)
        && items.get("properties").and_then(Value::as_object).is_none()
}

fn type_name(value: &Value) -> Option<&str> {
    match value.get("type") {
        Some(Value::String(kind)) => Some(kind.as_str()),
        Some(Value::Array(kinds)) => kinds
            .iter()
            .filter_map(Value::as_str)
            .find(|kind| *kind != "null"),
        _ => None,
    }
}

fn max_record_watermark(records: &[Value]) -> Option<String> {
    records
        .iter()
        .filter_map(record_meta_changed)
        .max()
        .map(str::to_owned)
}

fn record_meta_changed(record: &Value) -> Option<&str> {
    record
        .pointer("/_meta/lastChanged/date")
        .and_then(Value::as_str)
}

fn advance_watermark(current: &str, changes: &[api::MetaChange], records: &[Value]) -> String {
    let mut max = current.to_string();
    for changed in changes.iter().map(|change| change.changed.as_str()) {
        if changed > max.as_str() {
            max = changed.to_string();
        }
    }
    for changed in records.iter().filter_map(record_meta_changed) {
        if changed > max.as_str() {
            max = changed.to_string();
        }
    }
    max
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn id_diff_reports_creates_and_deletes_deterministically() {
        let local = BTreeSet::from(["a".to_string(), "b".to_string(), "d".to_string()]);
        let remote = BTreeSet::from(["b".to_string(), "c".to_string(), "e".to_string()]);

        assert_eq!(
            diff_ids(&local, &remote),
            IdDiff {
                creates: vec!["c".into(), "e".into()],
                deletes: vec!["a".into(), "d".into()],
            }
        );
    }

    #[test]
    fn watermark_advances_from_sidecar_or_refetched_records() {
        let changes = vec![api::MetaChange {
            meta_id: "meta-1".into(),
            changed: "2026-06-20T02:00:00Z".into(),
        }];
        let records = vec![json!({
            "_id": "user-2",
            "_meta": {
                "lastChanged": { "date": "2026-06-20T03:00:00Z" }
            }
        })];

        assert_eq!(
            advance_watermark("2026-06-20T01:00:00Z", &changes, &records),
            "2026-06-20T03:00:00Z"
        );
    }

    #[test]
    fn syncable_object_names_exclude_sidecars_and_internal_entries() {
        let doc = json!({
            "objects": [
                { "name": "alpha_user" },
                { "name": "bravo_role" },
                { "name": "alpha_usermeta" },
                { "name": "_internal" },
                { "name": "system_task" },
                { "name": "custom_profile" }
            ]
        });

        assert_eq!(
            syncable_object_names(&doc).unwrap(),
            vec!["alpha_user", "bravo_role", "custom_profile"]
        );
    }
}
