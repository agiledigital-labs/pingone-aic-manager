//! IDM scheduled-job specifics for the script-sync core. Like endpoints these
//! are `/openidm/config/…` objects (tenant-global, no realm, no `_rev`), but
//! the script lives nested at `invokeContext.script.source` and only
//! script-invoking schedules carry one (taskscanner/sync jobs don't). See
//! `docs/api/11-idm-endpoints.md`.

use super::{Kind, RemoteRef, RemoteScript};
use crate::{Error, Result};
use serde_json::Value;
use std::path::PathBuf;

const ID_PREFIX: &str = "schedule/";

fn ref_from_id(id: &str) -> RemoteRef {
    let name = id.strip_prefix(ID_PREFIX).unwrap_or(id).to_string();
    RemoteRef {
        kind: Kind::IdmSchedule,
        id: id.to_string(),
        name,
        context: None,
        is_default: false,
        evaluator_version: None,
    }
}

/// A schedule we can sync: one carrying an inline `invokeContext.script.source`
/// string. Non-script schedules (taskscanner, sync, …) are skipped.
fn has_inline_script(cfg: &Value) -> bool {
    cfg.pointer("/invokeContext/script/source")
        .and_then(|v| v.as_str())
        .is_some()
}

pub async fn list(tenant: &str, _realm: &str) -> Result<Vec<RemoteRef>> {
    // The config list returns full objects, so we can filter for script-bearing
    // schedules without an extra fetch per item.
    let body = crate::aic::api::get(tenant, "/openidm/config?_queryFilter=true").await?;
    let arr = body
        .get("result")
        .and_then(|v| v.as_array())
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected /openidm/config list shape: {body}"),
        })?;
    Ok(arr
        .iter()
        .filter(|v| {
            v.get("_id")
                .and_then(|x| x.as_str())
                .is_some_and(|id| id.starts_with(ID_PREFIX))
                && has_inline_script(v)
        })
        .filter_map(|v| v.get("_id").and_then(|x| x.as_str()))
        .map(ref_from_id)
        .collect())
}

pub async fn fetch(tenant: &str, _realm: &str, id: &str) -> Result<RemoteScript> {
    let raw = crate::aic::api::get(tenant, &format!("/openidm/config/{id}")).await?;
    Ok(RemoteScript {
        reference: ref_from_id(id),
        raw_config: raw,
    })
}

pub async fn write(
    tenant: &str,
    _realm: &str,
    script: &RemoteScript,
    confirmed_prod: bool,
) -> Result<Value> {
    let path = format!("/openidm/config/{}", script.reference.id);
    crate::aic::api::put(tenant, &path, script.raw_config.clone(), confirmed_prod).await
}

pub async fn delete(tenant: &str, _realm: &str, id: &str, confirmed_prod: bool) -> Result<Value> {
    crate::aic::api::delete(tenant, &format!("/openidm/config/{id}"), confirmed_prod).await
}

/// Schedule scripts are plaintext at `invokeContext.script.source`.
pub fn decode_source(raw: &Value) -> Result<Vec<u8>> {
    let s = raw
        .pointer("/invokeContext/script/source")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::Config("schedule has no invokeContext.script.source".into()))?;
    Ok(s.as_bytes().to_vec())
}

/// Write the edited source back into `invokeContext.script.source`, preserving
/// every other field (schedule, globals, enabled, …).
pub fn encode_source(raw: &mut Value, source: &[u8]) -> Result<()> {
    let s = String::from_utf8(source.to_vec())
        .map_err(|e| Error::Config(format!("schedule source is not UTF-8: {e}")))?;
    let script = raw
        .pointer_mut("/invokeContext/script")
        .and_then(|v| v.as_object_mut())
        .ok_or_else(|| Error::Config("schedule has no invokeContext.script object".into()))?;
    script.insert("source".into(), Value::String(s));
    Ok(())
}

pub fn workspace_subpath(r: &RemoteRef) -> PathBuf {
    PathBuf::from("idm")
        .join("schedule")
        .join(format!("{}.cjs", r.name))
}

/// Snapshot config path — kept distinct from endpoints so a schedule and an
/// endpoint of the same name can't collide.
pub fn config_subpath(r: &RemoteRef) -> PathBuf {
    PathBuf::from("idm-schedule").join(format!("{}.schedule.json", r.name))
}

pub fn extra_files(_r: &RemoteRef) -> Vec<(PathBuf, String)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn name_and_paths() {
        let r = ref_from_id("schedule/UpdateReviewList");
        assert_eq!(r.name, "UpdateReviewList");
        assert_eq!(
            workspace_subpath(&r),
            PathBuf::from("idm/schedule/UpdateReviewList.cjs")
        );
        assert_eq!(
            config_subpath(&r),
            PathBuf::from("idm-schedule/UpdateReviewList.schedule.json")
        );
    }

    #[test]
    fn only_script_schedules_are_listed() {
        assert!(has_inline_script(
            &json!({"invokeService":"script","invokeContext":{"script":{"source":"x"}}})
        ));
        assert!(!has_inline_script(
            &json!({"invokeService":"taskscanner","invokeContext":{"scriptProperty":"y"}})
        ));
    }

    #[test]
    fn nested_source_round_trips_preserving_siblings() {
        let body = b"logger.info('hi');";
        let mut raw = json!({
            "enabled": false,
            "schedule": "0 0 * * * ?",
            "invokeContext": {"script": {"type": "text/javascript", "source": "old", "globals": {}}}
        });
        encode_source(&mut raw, body).unwrap();
        assert_eq!(decode_source(&raw).unwrap(), body);
        // siblings preserved
        assert_eq!(raw["schedule"], json!("0 0 * * * ?"));
        assert_eq!(raw["invokeContext"]["script"]["type"], json!("text/javascript"));
        assert!(raw["invokeContext"]["script"].get("globals").is_some());
    }
}
