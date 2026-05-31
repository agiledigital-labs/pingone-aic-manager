//! AM-script specifics for the script-sync core. The **only** place AM-vs-IDM
//! differences for AM live: realm-scoped `/am/json…/scripts`, the
//! `protocol=2.0,resource=1.0` header, base64 `script` body, and context→dir
//! routing. See `docs/api/04-scripts.md`.

use super::{Kind, RemoteRef, RemoteScript};
use crate::{Error, Result};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde_json::Value;
use std::path::PathBuf;

/// AM scripts require the protocol-versioned header (the client default of
/// `resource=1.0` 400s on the scripts endpoint).
const API_VERSION: &str = "protocol=2.0,resource=1.0";

fn realm_path(realm: &str) -> String {
    format!("/am/json/realms/root/realms/{realm}")
}

fn ref_from_config(raw: &Value) -> RemoteRef {
    RemoteRef {
        kind: Kind::Am,
        id: str_field(raw, "_id"),
        name: str_field(raw, "name"),
        context: raw
            .get("context")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        is_default: raw.get("default").and_then(|v| v.as_bool()).unwrap_or(false),
    }
}

fn str_field(raw: &Value, key: &str) -> String {
    raw.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

pub async fn list(tenant: &str, realm: &str) -> Result<Vec<RemoteRef>> {
    // The scripts endpoint is paged; loop on `_pagedResultsCookie` so we don't
    // miss anything past the first page (docs/api/04-scripts.md).
    let mut refs = Vec::new();
    let mut cookie: Option<String> = None;
    loop {
        let mut path = format!("{}/scripts?_queryFilter=true&_pageSize=100", realm_path(realm));
        if let Some(c) = &cookie {
            path.push_str("&_pagedResultsCookie=");
            path.push_str(&encode_query(c));
        }
        let body = crate::aic::api::get_versioned(tenant, &path, API_VERSION).await?;
        let arr = body
            .get("result")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::Api {
                status: 0,
                body: format!("unexpected scripts list shape: {body}"),
            })?;
        refs.extend(arr.iter().map(ref_from_config));
        match body.get("pagedResultsCookie").and_then(|v| v.as_str()) {
            Some(c) if !c.is_empty() => cookie = Some(c.to_string()),
            _ => break,
        }
    }
    Ok(refs)
}

/// Percent-encode a query-string value. The paged-results cookie can contain
/// `+`, `/`, `=`; reqwest passes our query through verbatim, so we encode here.
fn encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub async fn fetch(tenant: &str, realm: &str, id: &str) -> Result<RemoteScript> {
    let path = format!("{}/scripts/{id}", realm_path(realm));
    let raw = crate::aic::api::get_versioned(tenant, &path, API_VERSION).await?;
    Ok(RemoteScript {
        reference: ref_from_config(&raw),
        raw_config: raw,
    })
}

pub async fn write(
    tenant: &str,
    realm: &str,
    script: &RemoteScript,
    confirmed_prod: bool,
) -> Result<Value> {
    let path = format!("{}/scripts/{}", realm_path(realm), script.reference.id);
    crate::aic::api::put_versioned(tenant, &path, script.raw_config.clone(), confirmed_prod, API_VERSION)
        .await
}

pub async fn delete(tenant: &str, realm: &str, id: &str, confirmed_prod: bool) -> Result<Value> {
    let path = format!("{}/scripts/{id}", realm_path(realm));
    crate::aic::api::delete_versioned(tenant, &path, confirmed_prod, API_VERSION).await
}

/// AM `script` is base64 on the wire. Some legacy scripts store it as a JSON
/// array of lines; handle both, preferring the string form the live API uses.
pub fn decode_source(raw: &Value) -> Result<Vec<u8>> {
    match raw.get("script") {
        Some(Value::String(s)) => B64
            .decode(s.trim())
            .map_err(|e| Error::Config(format!("decode AM script base64: {e}"))),
        Some(Value::Array(lines)) => {
            let joined: Vec<String> = lines
                .iter()
                .map(|l| l.as_str().unwrap_or_default().to_string())
                .collect();
            Ok(format!("{}\n", joined.join("\n")).into_bytes())
        }
        _ => Err(Error::Config("AM script has no `script` field".into())),
    }
}

pub fn encode_source(raw: &mut Value, source: &[u8]) -> Result<()> {
    let obj = raw
        .as_object_mut()
        .ok_or_else(|| Error::Config("AM raw config is not an object".into()))?;
    obj.insert("script".into(), Value::String(B64.encode(source)));
    Ok(())
}

/// `LIBRARY`→lib, `OIDC_CLAIMS`→oidc, everything else→src (verified mapping
/// from p1-sync; see `docs/api/04-scripts.md`).
fn dir_for_context(context: Option<&str>) -> &'static str {
    match context {
        Some("LIBRARY") => "lib",
        Some("OIDC_CLAIMS") => "oidc",
        _ => "src",
    }
}

pub fn workspace_subpath(r: &RemoteRef) -> PathBuf {
    PathBuf::from("am")
        .join(dir_for_context(r.context.as_deref()))
        .join(format!("{}.cjs", r.name))
}

pub fn config_filename(r: &RemoteRef) -> String {
    format!("{}.script.json", r.name)
}

/// `LIBRARY` scripts get an ES-module wrapper alongside the `.cjs` so other
/// scripts can `import`/`require` them with types (matches p1-sync).
pub fn extra_files(r: &RemoteRef) -> Vec<(PathBuf, String)> {
    if r.context.as_deref() == Some("LIBRARY") {
        let path = PathBuf::from("am").join("lib").join(format!("{}.js", r.name));
        vec![(path, format!("export * from \"./{}.cjs\";\n", r.name))]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rref(context: Option<&str>) -> RemoteRef {
        RemoteRef {
            kind: Kind::Am,
            id: "uuid-1".into(),
            name: "MyScript".into(),
            context: context.map(|s| s.to_string()),
            is_default: false,
        }
    }

    #[test]
    fn source_round_trips_through_base64() {
        let body = b"function foo() { return 1; }\n";
        let mut raw = json!({"name": "MyScript", "script": ""});
        encode_source(&mut raw, body).unwrap();
        // wire value is base64
        assert_eq!(raw["script"], json!(B64.encode(body)));
        // and decodes back to the same bytes
        assert_eq!(decode_source(&raw).unwrap(), body);
    }

    #[test]
    fn legacy_array_script_decodes_as_lines() {
        let raw = json!({"script": ["line1", "line2"]});
        assert_eq!(decode_source(&raw).unwrap(), b"line1\nline2\n");
    }

    #[test]
    fn context_routes_to_directory() {
        assert_eq!(workspace_subpath(&rref(None)), PathBuf::from("am/src/MyScript.cjs"));
        assert_eq!(
            workspace_subpath(&rref(Some("AUTHENTICATION_TREE_DECISION_NODE"))),
            PathBuf::from("am/src/MyScript.cjs")
        );
        assert_eq!(
            workspace_subpath(&rref(Some("LIBRARY"))),
            PathBuf::from("am/lib/MyScript.cjs")
        );
        assert_eq!(
            workspace_subpath(&rref(Some("OIDC_CLAIMS"))),
            PathBuf::from("am/oidc/MyScript.cjs")
        );
    }

    #[test]
    fn encode_query_escapes_cookie_specials() {
        // A typical paged-results cookie has +, /, = which must be escaped.
        assert_eq!(encode_query("aB9-_.~"), "aB9-_.~");
        assert_eq!(encode_query("a+b/c=d"), "a%2Bb%2Fc%3Dd");
    }

    #[test]
    fn library_gets_es_wrapper_only() {
        assert!(extra_files(&rref(None)).is_empty());
        let extra = extra_files(&rref(Some("LIBRARY")));
        assert_eq!(extra.len(), 1);
        assert_eq!(extra[0].0, PathBuf::from("am/lib/MyScript.js"));
        assert!(extra[0].1.contains("export * from \"./MyScript.cjs\""));
    }
}
