//! IDM managed-object **hook** specifics for the script-sync core. Hooks are
//! scripts embedded in the single tenant-global `managed` config document
//! (`/openidm/config/managed`, no `_rev`) at `objects[i].<hookKey>.source` —
//! the same scripts-inside-a-config-doc shape as schedules, except every hook
//! shares ONE wire document. See `docs/api/10-managed-objects.md`.
//!
//! Consequences of the shared document (all verified 2026-06-13):
//! - A hook is addressed as `managed/<object>.<hookKey>`
//!   (e.g. `managed/alpha_user.onCreate`); the wire id keeps that full form.
//! - [`fetch`] narrows the document to the one hook object
//!   (`{"type": "text/javascript", "source": "…"}`), so snapshots and the
//!   engine's content-based conflict check cover exactly the unit a user owns.
//! - [`write`] is a fresh **read-modify-write**: re-GET the live document,
//!   graft only our hook's `source` into it, PUT the whole thing back. This
//!   keeps concurrent edits to other objects/hooks intact; pushing a stale
//!   full document would silently revert them.
//! - The config PUT returns 200 **before** the hook registry applies the
//!   change, so [`write`] re-reads until the new source is live (or times out).
//! - Hooks are detected by VALUE SHAPE (`type` containing "javascript" +
//!   string `source`), not by a hardcoded event-key list. File-backed hooks
//!   (`{"type": …, "file": "…"}`) reference server-side files the config API
//!   cannot read or write — they are skipped, never converted or dropped.

use super::{Kind, RemoteRef, RemoteScript};
use crate::{Error, Result};
use serde_json::Value;
use std::path::PathBuf;

const ID_PREFIX: &str = "managed/";

/// How long to wait for a 200'd config PUT to actually reach the hook
/// registry (observed ~5s on the sandbox).
const APPLY_RETRIES: u32 = 6;
const APPLY_DELAY_MS: u64 = 2_000;

/// Split a hook name (`<object>.<hookKey>`) on the LAST dot — managed object
/// names (`alpha_user`) never contain dots today, but hook keys definitely
/// don't, so last-dot is the safe direction.
fn parse_name(name: &str) -> Result<(&str, &str)> {
    name.rsplit_once('.').ok_or_else(|| {
        Error::Config(format!(
            "managed hook name '{name}' must be <object>.<hookKey> (e.g. alpha_user.onCreate)"
        ))
    })
}

fn ref_from_name(name: &str) -> RemoteRef {
    RemoteRef {
        kind: Kind::IdmManagedHook,
        id: format!("{ID_PREFIX}{name}"),
        name: name.to_string(),
        context: None,
        is_default: false,
        evaluator_version: None,
    }
}

fn name_from_id(id: &str) -> &str {
    id.strip_prefix(ID_PREFIX).unwrap_or(id)
}

/// An editable hook: an object-valued property with a string `source` and a
/// `type` mentioning javascript. File-backed hooks (`file` instead of
/// `source`) fail this test by design.
fn is_inline_hook(v: &Value) -> bool {
    v.is_object()
        && v.get("source").is_some_and(Value::is_string)
        && v.get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| t.contains("javascript"))
}

async fn fetch_managed_doc(tenant: &str) -> Result<Value> {
    crate::aic::api::get(tenant, "/openidm/config/managed").await
}

fn objects_of(doc: &Value) -> Result<&Vec<Value>> {
    doc.get("objects")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected /openidm/config/managed shape: {doc}"),
        })
}

/// Locate `objects[name == object]`'s `<hook>` value in a mutable document.
fn hook_slot<'a>(doc: &'a mut Value, object: &str, hook: &str) -> Result<&'a mut Value> {
    let objects = doc
        .get_mut("objects")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| Error::Config("managed config has no objects array".into()))?;
    let entry = objects
        .iter_mut()
        .find(|o| o.get("name").and_then(Value::as_str) == Some(object))
        .ok_or_else(|| {
            Error::Config(format!(
                "managed object '{object}' no longer exists on the tenant — re-pull"
            ))
        })?;
    entry.get_mut(hook).ok_or_else(|| {
        Error::Config(format!(
            "hook '{hook}' no longer exists on managed object '{object}' — re-pull"
        ))
    })
}

pub async fn list(tenant: &str, _realm: &str) -> Result<Vec<RemoteRef>> {
    let doc = fetch_managed_doc(tenant).await?;
    let mut out = Vec::new();
    for obj in objects_of(&doc)? {
        let Some(obj_name) = obj.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(map) = obj.as_object() else { continue };
        // Top-level keys only: hooks live beside `name`/`schema`, never under
        // them, and `schema.properties` can contain arbitrary user keys.
        for (key, value) in map {
            if key == "schema" || !is_inline_hook(value) {
                continue;
            }
            out.push(ref_from_name(&format!("{obj_name}.{key}")));
        }
    }
    // The wire document order is object-definition order; sort for stable
    // list/picker output.
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub async fn fetch(tenant: &str, _realm: &str, id: &str) -> Result<RemoteScript> {
    let name = name_from_id(id);
    let (object, hook) = parse_name(name)?;
    let mut doc = fetch_managed_doc(tenant).await?;
    let slot = hook_slot(&mut doc, object, hook)?;
    if !is_inline_hook(slot) {
        return Err(Error::Config(format!(
            "hook '{name}' is not an inline-source hook (file-backed hooks are read-only)"
        )));
    }
    Ok(RemoteScript {
        reference: ref_from_name(name),
        // Narrowed: just the hook object. The snapshot + conflict check then
        // cover exactly this hook, and drift elsewhere in the shared document
        // is (correctly) someone else's business.
        raw_config: slot.clone(),
    })
}

pub async fn write(
    tenant: &str,
    _realm: &str,
    script: &RemoteScript,
    confirmed_prod: bool,
) -> Result<Value> {
    let name = name_from_id(&script.reference.id);
    let (object, hook) = parse_name(name)?;
    let new_source = decode_source(&script.raw_config)?;
    let new_source_str = String::from_utf8(new_source.clone())
        .map_err(|e| Error::Config(format!("hook source is not UTF-8: {e}")))?;

    // Fresh read-modify-write against the LIVE document (not the pulled
    // snapshot) so concurrent edits to other objects/hooks survive our push.
    let mut doc = fetch_managed_doc(tenant).await?;
    {
        let slot = hook_slot(&mut doc, object, hook)?;
        if !is_inline_hook(slot) {
            return Err(Error::Config(format!(
                "hook '{name}' changed to a non-inline form on the tenant — re-pull and re-check"
            )));
        }
        slot.as_object_mut()
            .expect("is_inline_hook guarantees an object")
            .insert("source".into(), Value::String(new_source_str.clone()));
    }
    let resp = crate::aic::api::put(tenant, "/openidm/config/managed", doc, confirmed_prod).await?;

    // The PUT 200s before the hook registry applies the change (verified —
    // see docs/api/10). Confirm the new source is actually live so callers
    // (and `watch`) can trust a returned Ok.
    for attempt in 0..APPLY_RETRIES {
        let mut live = fetch_managed_doc(tenant).await?;
        if let Ok(slot) = hook_slot(&mut live, object, hook) {
            if slot.get("source").and_then(Value::as_str) == Some(new_source_str.as_str()) {
                return Ok(resp);
            }
        }
        if attempt + 1 < APPLY_RETRIES {
            tokio::time::sleep(std::time::Duration::from_millis(APPLY_DELAY_MS)).await;
        }
    }
    Err(Error::Config(format!(
        "pushed hook '{name}' but the managed config did not reflect it within \
         {}s — verify the tenant state before retrying",
        (APPLY_RETRIES as u64 * APPLY_DELAY_MS) / 1_000
    )))
}

/// Deleting a hook = removing its key from the object, via the same RMW.
pub async fn delete(tenant: &str, _realm: &str, id: &str, confirmed_prod: bool) -> Result<Value> {
    let name = name_from_id(id);
    let (object, hook) = parse_name(name)?;
    let mut doc = fetch_managed_doc(tenant).await?;
    // Borrow-check friendly: locate the entry directly rather than via hook_slot.
    let objects = doc
        .get_mut("objects")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| Error::Config("managed config has no objects array".into()))?;
    let entry = objects
        .iter_mut()
        .find(|o| o.get("name").and_then(Value::as_str) == Some(object))
        .and_then(|o| o.as_object_mut())
        .ok_or_else(|| Error::Config(format!("managed object '{object}' not found")))?;
    if entry.remove(hook).is_none() {
        return Err(Error::Config(format!(
            "hook '{hook}' not present on managed object '{object}'"
        )));
    }
    crate::aic::api::put(tenant, "/openidm/config/managed", doc, confirmed_prod).await
}

/// Hook source is plaintext at `.source` of the (narrowed) hook object.
pub fn decode_source(raw: &Value) -> Result<Vec<u8>> {
    let s = raw
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Config("managed hook has no source".into()))?;
    Ok(s.as_bytes().to_vec())
}

/// Write the edited source back into the narrowed hook object, preserving
/// siblings (`type`, `globals`, …).
pub fn encode_source(raw: &mut Value, source: &[u8]) -> Result<()> {
    let s = String::from_utf8(source.to_vec())
        .map_err(|e| Error::Config(format!("managed hook source is not UTF-8: {e}")))?;
    let map = raw
        .as_object_mut()
        .ok_or_else(|| Error::Config("managed hook config is not an object".into()))?;
    map.insert("source".into(), Value::String(s));
    Ok(())
}

/// `idm/managed/<object>/<hookKey>.cjs` — one folder per managed object.
pub fn workspace_subpath(r: &RemoteRef) -> PathBuf {
    let (object, hook) = parse_name(&r.name).unwrap_or((r.name.as_str(), "hook"));
    PathBuf::from("idm")
        .join("managed")
        .join(object)
        .join(format!("{hook}.cjs"))
}

/// Snapshot path — kind-distinct so no name can collide with endpoints/schedules.
pub fn config_subpath(r: &RemoteRef) -> PathBuf {
    PathBuf::from("idm-managed").join(format!("{}.hook.json", r.name))
}

pub fn extra_files(_r: &RemoteRef) -> Vec<(PathBuf, String)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names_parse_on_last_dot() {
        assert_eq!(
            parse_name("alpha_user.onCreate").unwrap(),
            ("alpha_user", "onCreate")
        );
        assert!(parse_name("nodot").is_err());
    }

    #[test]
    fn paths() {
        let r = ref_from_name("alpha_user.onCreate");
        assert_eq!(r.id, "managed/alpha_user.onCreate");
        assert_eq!(
            workspace_subpath(&r),
            PathBuf::from("idm/managed/alpha_user/onCreate.cjs")
        );
        assert_eq!(
            config_subpath(&r),
            PathBuf::from("idm-managed/alpha_user.onCreate.hook.json")
        );
    }

    #[test]
    fn hook_detection_is_value_shaped() {
        assert!(is_inline_hook(
            &json!({"type": "text/javascript", "source": "x"})
        ));
        // File-backed: read-only, must not be detected as editable.
        assert!(!is_inline_hook(
            &json!({"type": "text/javascript", "file": "roles/onDelete-roles.js"})
        ));
        assert!(!is_inline_hook(&json!({"source": "x"})));
        assert!(!is_inline_hook(&json!("text")));
    }

    #[test]
    fn source_round_trips_preserving_siblings() {
        let mut raw = json!({"type": "text/javascript", "source": "old", "globals": {"a": 1}});
        encode_source(&mut raw, b"new body").unwrap();
        assert_eq!(decode_source(&raw).unwrap(), b"new body");
        assert_eq!(raw["type"], json!("text/javascript"));
        assert_eq!(raw["globals"]["a"], json!(1));
    }

    #[test]
    fn hook_slot_errors_name_the_problem() {
        let mut doc = json!({"objects": [{"name": "alpha_user", "onCreate": {"type": "text/javascript", "source": "x"}}]});
        assert!(hook_slot(&mut doc, "alpha_user", "onCreate").is_ok());
        assert!(
            hook_slot(&mut doc, "alpha_user", "onUpdate")
                .unwrap_err()
                .to_string()
                .contains("onUpdate")
        );
        assert!(
            hook_slot(&mut doc, "ghost", "onCreate")
                .unwrap_err()
                .to_string()
                .contains("ghost")
        );
    }
}
