//! IDM custom-endpoint specifics for the script-sync core. The **only** place
//! AM-vs-IDM differences for IDM live: `/openidm/config/endpoint/{name}` (no
//! realm, no `Accept-API-Version`), plaintext `source` (not base64), and
//! name-derived-from-`_id`. See `docs/api/11-idm-endpoints.md`.

use super::{Kind, RemoteRef, RemoteScript};
use crate::{Error, Result};
use serde_json::Value;
use std::path::PathBuf;

const ID_PREFIX: &str = "endpoint/";

fn ref_from_id(id: &str) -> RemoteRef {
    let name = id.strip_prefix(ID_PREFIX).unwrap_or(id).to_string();
    RemoteRef {
        kind: Kind::IdmEndpoint,
        id: id.to_string(),
        name,
        context: None,
        is_default: false,
    }
}

pub async fn list(tenant: &str, _realm: &str) -> Result<Vec<RemoteRef>> {
    // IDM config is tenant-global; the list is unfiltered, so we filter for
    // `endpoint/` ids client-side (verified — see the doc).
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
        .filter_map(|v| v.get("_id").and_then(|x| x.as_str()))
        .filter(|id| id.starts_with(ID_PREFIX))
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

/// IDM `source` is plaintext. Scripted endpoints may nest it as
/// `{ "source": "…" }`; handle both.
pub fn decode_source(raw: &Value) -> Result<Vec<u8>> {
    let s = match raw.get("source") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(o)) => o
            .get("source")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("IDM endpoint nested `source` has no string".into()))?
            .to_string(),
        _ => return Err(Error::Config("IDM endpoint has no `source` field".into())),
    };
    Ok(s.into_bytes())
}

/// Write the edited source back as a plaintext `source` string (flattening any
/// nested form, matching the verified create shape).
pub fn encode_source(raw: &mut Value, source: &[u8]) -> Result<()> {
    let s = String::from_utf8(source.to_vec())
        .map_err(|e| Error::Config(format!("IDM endpoint source is not UTF-8: {e}")))?;
    let obj = raw
        .as_object_mut()
        .ok_or_else(|| Error::Config("IDM raw config is not an object".into()))?;
    obj.insert("source".into(), Value::String(s));
    Ok(())
}

pub fn workspace_subpath(r: &RemoteRef) -> PathBuf {
    PathBuf::from("idm")
        .join("endpoint")
        .join(format!("{}.cjs", r.name))
}

pub fn config_filename(r: &RemoteRef) -> String {
    format!("{}.idm.json", r.name)
}

pub fn extra_files(_r: &RemoteRef) -> Vec<(PathBuf, String)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn name_derives_from_id_prefix() {
        let r = ref_from_id("endpoint/my-endpoint");
        assert_eq!(r.name, "my-endpoint");
        assert_eq!(r.id, "endpoint/my-endpoint");
        assert_eq!(
            workspace_subpath(&r),
            PathBuf::from("idm/endpoint/my-endpoint.cjs")
        );
        assert_eq!(config_filename(&r), "my-endpoint.idm.json");
    }

    #[test]
    fn plaintext_source_round_trips() {
        let body = b"(function(){ return {}; })();";
        let mut raw = json!({"_id": "endpoint/x", "type": "text/javascript", "source": "old"});
        encode_source(&mut raw, body).unwrap();
        assert_eq!(raw["source"], json!("(function(){ return {}; })();"));
        assert_eq!(decode_source(&raw).unwrap(), body);
        // unrelated fields are preserved for round-trip
        assert_eq!(raw["type"], json!("text/javascript"));
    }

    #[test]
    fn nested_source_object_decodes() {
        let raw = json!({"source": {"source": "var x = 1;", "type": "text/javascript"}});
        assert_eq!(decode_source(&raw).unwrap(), b"var x = 1;");
    }
}
