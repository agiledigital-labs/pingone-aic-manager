//! IDM sync-mapping script specifics for the script-sync core. Sync mappings
//! are scripts embedded in the single tenant-global `sync` config document
//! (`/openidm/config/sync`, no `_rev`) at either mapping-level script keys
//! (`mappings[i].onUpdate.source`, etc.) or per-property
//! `transform`/`condition` slots. See `docs/api/16-sync-mappings.md`.
//!
//! Consequences of the shared document (verified 2026-06-18):
//! - A script is addressed as `sync/<mapping>.<slotpath>`, where `<slotpath>`
//!   is a mapping event (`onCreate`, `correlationScript`, `result`, ...), or
//!   `transform.<targetAttr>` / `condition.<targetAttr>`.
//! - [`fetch`] narrows the document to the one script object
//!   (`{"type": "text/javascript", "source": "..."}`), so snapshots and the
//!   engine's content-based conflict check cover exactly the script a user
//!   owns.
//! - [`write`] is a fresh **read-modify-write**: re-GET the live sync document,
//!   graft only our slot's `source` into it, PUT the whole thing back, then
//!   poll until the applied document shows the new source.
//! - Whole-mapping script slots come from a known key list
//!   ([`WHOLE_MAPPING_SLOTS`]) so non-script JS-shaped config (e.g.
//!   `correlationQuery`) is never mistaken for a script; per-property
//!   `transform`/`condition` are then confirmed by VALUE SHAPE (`type`
//!   containing "javascript" + string `source`). File-backed scripts
//!   (`{"type": ..., "file": "..."}`) reference server-side files the config
//!   API cannot read or write, so they are skipped and treated as read-only.

use super::{Kind, RemoteRef, RemoteScript};
use crate::{Error, Result};
use serde_json::Value;
use std::path::PathBuf;

const ID_PREFIX: &str = "sync/";
const SYNC_PATH: &str = "/openidm/config/sync";

const WHOLE_MAPPING_SLOTS: &[&str] = &[
    "onCreate",
    "onUpdate",
    "onDelete",
    "onLink",
    "onUnlink",
    "onSync",
    "validSource",
    "validTarget",
    "correlationScript",
    "result",
];

/// How long to wait for a 200'd config PUT to actually apply to the sync
/// mapping registry. Same cadence as managed hooks.
const APPLY_RETRIES: u32 = 6;
const APPLY_DELAY_MS: u64 = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotKind<'a> {
    Whole(&'a str),
    Transform(&'a str),
    Condition(&'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotCategory {
    Behaviour,
    Result,
    Transform,
    Condition,
}

impl SlotCategory {
    fn as_str(self) -> &'static str {
        match self {
            SlotCategory::Behaviour => "behaviour",
            SlotCategory::Result => "result",
            SlotCategory::Transform => "transform",
            SlotCategory::Condition => "condition",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParsedName<'a> {
    mapping: &'a str,
    slotpath: &'a str,
    slot: SlotKind<'a>,
}

/// Split `sync/<mapping>.<slotpath>` (or just `<mapping>.<slotpath>`) on the
/// FIRST dot. IDM mapping names contain no dots; target attribute names can
/// contain dots/slashes, so the remainder is kept verbatim.
fn parse_name(name_or_id: &str) -> Result<ParsedName<'_>> {
    let name = name_or_id.strip_prefix(ID_PREFIX).unwrap_or(name_or_id);
    let (mapping, slotpath) = name.split_once('.').ok_or_else(|| {
        Error::Config(format!(
            "sync mapping script name '{name}' must be <mapping>.<slotpath> \
             (e.g. managedUser_systemLdapAccounts.onUpdate)"
        ))
    })?;
    if mapping.is_empty() || slotpath.is_empty() {
        return Err(Error::Config(format!(
            "sync mapping script name '{name}' must include both mapping and slot"
        )));
    }

    let slot = if let Some(attr) = slotpath.strip_prefix("transform.") {
        if attr.is_empty() {
            return Err(Error::Config(format!(
                "sync mapping transform name '{name}' must include a target attribute"
            )));
        }
        SlotKind::Transform(attr)
    } else if let Some(attr) = slotpath.strip_prefix("condition.") {
        if attr.is_empty() {
            return Err(Error::Config(format!(
                "sync mapping condition name '{name}' must include a target attribute"
            )));
        }
        SlotKind::Condition(attr)
    } else {
        SlotKind::Whole(slotpath)
    };

    Ok(ParsedName {
        mapping,
        slotpath,
        slot,
    })
}

fn ref_from_name(name: &str) -> RemoteRef {
    RemoteRef {
        kind: Kind::IdmSyncMapping,
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

/// An editable sync-mapping script: an object-valued property with a string
/// `source` and a `type` mentioning javascript. File-backed scripts (`file`
/// instead of `source`) fail this test by design.
fn is_inline_script(v: &Value) -> bool {
    v.is_object()
        && v.get("source").is_some_and(Value::is_string)
        && v.get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| t.contains("javascript"))
}

async fn get_sync_doc(tenant: &str) -> Result<Value> {
    crate::aic::api::get(tenant, SYNC_PATH).await
}

async fn replace_sync_doc(tenant: &str, doc: Value, confirmed_prod: bool) -> Result<Value> {
    crate::aic::api::put(tenant, SYNC_PATH, doc, confirmed_prod).await
}

fn mappings_of(doc: &Value) -> Result<&Vec<Value>> {
    doc.get("mappings")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected /openidm/config/sync shape: {doc}"),
        })
}

fn mappings_mut(doc: &mut Value) -> Result<&mut Vec<Value>> {
    if doc.get("mappings").and_then(Value::as_array).is_none() {
        return Err(Error::Api {
            status: 0,
            body: format!("unexpected /openidm/config/sync shape: {doc}"),
        });
    }
    Ok(doc
        .get_mut("mappings")
        .and_then(Value::as_array_mut)
        .expect("checked mappings array above"))
}

fn mapping_mut<'a>(doc: &'a mut Value, mapping: &str) -> Result<&'a mut Value> {
    mappings_mut(doc)?
        .iter_mut()
        .find(|m| m.get("name").and_then(Value::as_str) == Some(mapping))
        .ok_or_else(|| {
            Error::Config(format!(
                "sync mapping '{mapping}' no longer exists on the tenant -- re-pull"
            ))
        })
}

fn property_mut<'a>(
    mapping_value: &'a mut Value,
    mapping: &str,
    attr: &str,
) -> Result<&'a mut Value> {
    let properties = mapping_value
        .get_mut("properties")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            Error::Config(format!(
                "sync mapping '{mapping}' has no properties array -- re-pull"
            ))
        })?;
    properties
        .iter_mut()
        .find(|p| p.get("target").and_then(Value::as_str) == Some(attr))
        .ok_or_else(|| {
            Error::Config(format!(
                "property '{attr}' no longer exists on sync mapping '{mapping}' -- re-pull"
            ))
        })
}

fn missing_script_error(parsed: &ParsedName<'_>) -> Error {
    Error::Config(format!(
        "sync mapping script '{}.{}' no longer exists -- re-pull",
        parsed.mapping, parsed.slotpath
    ))
}

/// Locate the parsed slot in a mutable sync document.
fn slot_value_mut<'a>(doc: &'a mut Value, parsed: &ParsedName<'_>) -> Result<&'a mut Value> {
    let mapping_value = mapping_mut(doc, parsed.mapping)?;
    match parsed.slot {
        SlotKind::Whole(slot) => mapping_value
            .get_mut(slot)
            .ok_or_else(|| missing_script_error(parsed)),
        SlotKind::Transform(attr) => property_mut(mapping_value, parsed.mapping, attr)?
            .get_mut("transform")
            .ok_or_else(|| missing_script_error(parsed)),
        SlotKind::Condition(attr) => property_mut(mapping_value, parsed.mapping, attr)?
            .get_mut("condition")
            .ok_or_else(|| missing_script_error(parsed)),
    }
}

fn remove_slot(doc: &mut Value, parsed: &ParsedName<'_>) -> Result<()> {
    let mapping_value = mapping_mut(doc, parsed.mapping)?;
    match parsed.slot {
        SlotKind::Whole(slot) => {
            let obj = mapping_value.as_object_mut().ok_or_else(|| {
                Error::Config(format!(
                    "sync mapping '{}' is not an object -- re-pull",
                    parsed.mapping
                ))
            })?;
            let existing = obj.get(slot).ok_or_else(|| missing_script_error(parsed))?;
            if !is_inline_script(existing) {
                return Err(read_only_error(parsed));
            }
            obj.remove(slot);
        }
        SlotKind::Transform(attr) | SlotKind::Condition(attr) => {
            let key = match parsed.slot {
                SlotKind::Transform(_) => "transform",
                SlotKind::Condition(_) => "condition",
                SlotKind::Whole(_) => unreachable!(),
            };
            let property = property_mut(mapping_value, parsed.mapping, attr)?;
            let obj = property.as_object_mut().ok_or_else(|| {
                Error::Config(format!(
                    "property '{attr}' on sync mapping '{}' is not an object -- re-pull",
                    parsed.mapping
                ))
            })?;
            let existing = obj.get(key).ok_or_else(|| missing_script_error(parsed))?;
            if !is_inline_script(existing) {
                return Err(read_only_error(parsed));
            }
            obj.remove(key);
        }
    }
    Ok(())
}

fn read_only_error(parsed: &ParsedName<'_>) -> Error {
    Error::Config(format!(
        "sync mapping script '{}.{}' is not an inline-source script \
         (file-backed scripts are read-only)",
        parsed.mapping, parsed.slotpath
    ))
}

fn refs_from_doc(doc: &Value) -> Result<Vec<RemoteRef>> {
    let mut out = Vec::new();
    for mapping in mappings_of(doc)? {
        let Some(mapping_name) = mapping.get("name").and_then(Value::as_str) else {
            continue;
        };

        for slot in WHOLE_MAPPING_SLOTS {
            if mapping.get(*slot).is_some_and(is_inline_script) {
                out.push(ref_from_name(&format!("{mapping_name}.{slot}")));
            }
        }

        let Some(properties) = mapping.get("properties").and_then(Value::as_array) else {
            continue;
        };
        for property in properties {
            let Some(target) = property.get("target").and_then(Value::as_str) else {
                continue;
            };
            if target.is_empty() {
                continue;
            }
            if property.get("transform").is_some_and(is_inline_script) {
                out.push(ref_from_name(&format!("{mapping_name}.transform.{target}")));
            }
            if property.get("condition").is_some_and(is_inline_script) {
                out.push(ref_from_name(&format!("{mapping_name}.condition.{target}")));
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub async fn list(tenant: &str, _realm: &str) -> Result<Vec<RemoteRef>> {
    let doc = get_sync_doc(tenant).await?;
    refs_from_doc(&doc)
}

pub async fn fetch(tenant: &str, _realm: &str, id: &str) -> Result<RemoteScript> {
    let name = name_from_id(id);
    let parsed = parse_name(name)?;
    let mut doc = get_sync_doc(tenant).await?;
    let slot = slot_value_mut(&mut doc, &parsed)?;
    if !is_inline_script(slot) {
        return Err(read_only_error(&parsed));
    }
    Ok(RemoteScript {
        reference: ref_from_name(name),
        // Narrowed: just the script slot object. The snapshot + conflict check
        // then cover exactly this script, and drift elsewhere in the shared
        // document is someone else's business.
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
    let parsed = parse_name(name)?;
    let new_source = decode_source(&script.raw_config)?;
    let new_source_str = String::from_utf8(new_source.clone())
        .map_err(|e| Error::Config(format!("sync mapping script source is not UTF-8: {e}")))?;

    // Fresh read-modify-write against the LIVE document (not the pulled
    // snapshot) so concurrent edits to other mappings/slots survive our push.
    let mut doc = get_sync_doc(tenant).await?;
    {
        let slot = slot_value_mut(&mut doc, &parsed)?;
        if !is_inline_script(slot) {
            return Err(Error::Config(format!(
                "sync mapping script '{}.{}' changed to a non-inline form on \
                 the tenant -- re-pull and re-check",
                parsed.mapping, parsed.slotpath
            )));
        }
        slot.as_object_mut()
            .expect("is_inline_script guarantees an object")
            .insert("source".into(), Value::String(new_source_str.clone()));
    }
    let resp = replace_sync_doc(tenant, doc, confirmed_prod).await?;

    // The PUT 200s before the sync registry applies the change (verified --
    // see docs/api/16). Confirm the new source is actually live so callers
    // can trust a returned Ok.
    for attempt in 0..APPLY_RETRIES {
        let mut live = get_sync_doc(tenant).await?;
        if let Ok(slot) = slot_value_mut(&mut live, &parsed) {
            if slot.get("source").and_then(Value::as_str) == Some(new_source_str.as_str()) {
                return Ok(resp);
            }
        }
        if attempt + 1 < APPLY_RETRIES {
            tokio::time::sleep(std::time::Duration::from_millis(APPLY_DELAY_MS)).await;
        }
    }
    Err(Error::Config(format!(
        "pushed sync mapping script '{}.{}' but the sync config did not reflect \
         it within {}s -- verify the tenant state before retrying",
        parsed.mapping,
        parsed.slotpath,
        (APPLY_RETRIES as u64 * APPLY_DELAY_MS) / 1_000
    )))
}

/// Deleting a sync-mapping script = removing its slot key, via the same RMW.
/// Per-property deletes remove only `transform` or `condition`, never the
/// whole property.
pub async fn delete(tenant: &str, _realm: &str, id: &str, confirmed_prod: bool) -> Result<Value> {
    let name = name_from_id(id);
    let parsed = parse_name(name)?;
    let mut doc = get_sync_doc(tenant).await?;
    remove_slot(&mut doc, &parsed)?;
    replace_sync_doc(tenant, doc, confirmed_prod).await
}

/// Sync-mapping source is plaintext at `.source` of the narrowed script object.
pub fn decode_source(raw: &Value) -> Result<Vec<u8>> {
    let s = raw
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Config("sync mapping script has no source".into()))?;
    Ok(s.as_bytes().to_vec())
}

/// Write the edited source back into the narrowed script object, preserving
/// siblings (`type`, `globals`, ...).
pub fn encode_source(raw: &mut Value, source: &[u8]) -> Result<()> {
    let s = String::from_utf8(source.to_vec())
        .map_err(|e| Error::Config(format!("sync mapping script source is not UTF-8: {e}")))?;
    let map = raw
        .as_object_mut()
        .ok_or_else(|| Error::Config("sync mapping script config is not an object".into()))?;
    map.insert("source".into(), Value::String(s));
    Ok(())
}

fn attr_file_slug(attr: &str) -> String {
    attr.replace(['/', '.'], "_")
}

fn safe_filename(name: &str) -> String {
    name.replace(['/', '\\'], "_")
}

fn slot_category(parsed: &ParsedName<'_>) -> SlotCategory {
    match parsed.slot {
        SlotKind::Whole("result") => SlotCategory::Result,
        SlotKind::Whole(_) => SlotCategory::Behaviour,
        SlotKind::Transform(_) => SlotCategory::Transform,
        SlotKind::Condition(_) => SlotCategory::Condition,
    }
}

fn slot_filename(parsed: &ParsedName<'_>) -> String {
    match parsed.slot {
        SlotKind::Whole(slot) => slot.to_string(),
        SlotKind::Transform(attr) | SlotKind::Condition(attr) => attr_file_slug(attr),
    }
}

/// `idm/sync/<mapping>/<category>/<slotfile>.cjs` -- one folder per binding
/// category so each TypeScript project sees only non-conflicting globals. The
/// workspace filename slugifies target attrs, but the RemoteRef name keeps the
/// exact attr.
pub fn workspace_subpath(r: &RemoteRef) -> PathBuf {
    match parse_name(&r.name) {
        Ok(parsed) => PathBuf::from("idm")
            .join("sync")
            .join(parsed.mapping)
            .join(slot_category(&parsed).as_str())
            .join(format!("{}.cjs", slot_filename(&parsed))),
        Err(_) => PathBuf::from("idm")
            .join("sync")
            .join(safe_filename(&r.name))
            .join("script.cjs"),
    }
}

/// Snapshot path -- kind-distinct so no name can collide with endpoints,
/// schedules, or managed hooks.
pub fn config_subpath(r: &RemoteRef) -> PathBuf {
    PathBuf::from("idm-sync").join(format!("{}.json", safe_filename(&r.name)))
}

/// Per-mapping, per-category TypeScript project. Each category has a generated
/// binding file because slots disagree about globals like `source` and
/// `target`.
pub fn leaf_tsconfig(mapping: &str, category: &str) -> String {
    let mapping_file = safe_filename(mapping);
    format!(
        "{{\n  \"extends\": \"../../../tsconfig.json\",\n  \"include\": [\n    \"./**/*\",\n    \"../../../types/rhino-1.7.14.d.ts\",\n    \"../../../types/common.d.ts\",\n    \"../../../types/managed/*.d.ts\",\n    \"../../../types/sync/_shared.d.ts\",\n    \"../../../types/sync/{mapping_file}.{category}.d.ts\"\n  ]\n}}\n"
    )
}

pub fn extra_files(r: &RemoteRef) -> Vec<(PathBuf, String)> {
    let Ok(parsed) = parse_name(&r.name) else {
        return Vec::new();
    };
    let category = slot_category(&parsed).as_str();
    vec![(
        PathBuf::from("idm")
            .join("sync")
            .join(parsed.mapping)
            .join(category)
            .join("tsconfig.json"),
        leaf_tsconfig(parsed.mapping, category),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names_parse_on_first_dot_and_keep_attrs_verbatim() {
        assert_eq!(
            parse_name("managedUser_systemLdapAccounts.onUpdate").unwrap(),
            ParsedName {
                mapping: "managedUser_systemLdapAccounts",
                slotpath: "onUpdate",
                slot: SlotKind::Whole("onUpdate"),
            }
        );
        assert_eq!(
            parse_name("sync/map.transform.profile/name.given").unwrap(),
            ParsedName {
                mapping: "map",
                slotpath: "transform.profile/name.given",
                slot: SlotKind::Transform("profile/name.given"),
            }
        );
        assert_eq!(
            parse_name("map.condition.manager.ref").unwrap(),
            ParsedName {
                mapping: "map",
                slotpath: "condition.manager.ref",
                slot: SlotKind::Condition("manager.ref"),
            }
        );
        assert!(parse_name("nodot").is_err());
        assert!(parse_name("map.transform.").is_err());
    }

    #[test]
    fn inline_detection_is_value_shaped() {
        assert!(is_inline_script(
            &json!({"type": "text/javascript", "source": "x"})
        ));
        assert!(is_inline_script(
            &json!({"type": "application/javascript", "source": "x", "globals": {}})
        ));
        // File-backed: read-only, must not be detected as editable.
        assert!(!is_inline_script(
            &json!({"type": "text/javascript", "file": "sync/onUpdate.js"})
        ));
        assert!(!is_inline_script(&json!({"source": "x"})));
        assert!(!is_inline_script(&json!("text")));
    }

    #[test]
    fn refs_from_doc_lists_inline_slots_only_and_sorts() {
        let doc = json!({
            "_id": "sync",
            "mappings": [{
                "name": "b_map",
                "correlationQuery": [{"type": "text/javascript", "source": "not a script slot"}],
                "onUpdate": {"type": "text/javascript", "source": "update"},
                "onDelete": {"type": "text/javascript", "file": "sync/onDelete.js"},
                "properties": [
                    {
                        "target": "name",
                        "source": "cn",
                        "transform": {"type": "text/javascript", "source": "return source;"}
                    },
                    {
                        "target": "manager.ref",
                        "condition": {"type": "text/javascript", "source": "true"}
                    }
                ]
            }, {
                "name": "a_map",
                "validSource": {"type": "text/javascript", "source": "true"}
            }]
        });
        let refs = refs_from_doc(&doc).unwrap();
        let names: Vec<_> = refs.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "a_map.validSource",
                "b_map.condition.manager.ref",
                "b_map.onUpdate",
                "b_map.transform.name",
            ]
        );
        assert!(refs.iter().all(|r| r.kind == Kind::IdmSyncMapping));
        assert!(refs.iter().all(|r| r.id.starts_with("sync/")));
    }

    #[test]
    fn paths_slugify_attr_filename_but_keep_wire_name() {
        let r = ref_from_name("map.transform.profile/name.given");
        assert_eq!(r.id, "sync/map.transform.profile/name.given");
        assert_eq!(r.name, "map.transform.profile/name.given");
        assert_eq!(
            workspace_subpath(&r),
            PathBuf::from("idm/sync/map/transform/profile_name_given.cjs")
        );
        assert_eq!(
            config_subpath(&r),
            PathBuf::from("idm-sync/map.transform.profile_name.given.json")
        );

        let event = ref_from_name("map.onCreate");
        assert_eq!(
            workspace_subpath(&event),
            PathBuf::from("idm/sync/map/behaviour/onCreate.cjs")
        );

        let result = ref_from_name("map.result");
        assert_eq!(
            workspace_subpath(&result),
            PathBuf::from("idm/sync/map/result/result.cjs")
        );
    }

    #[test]
    fn extra_files_emit_per_category_leaf_tsconfig() {
        let files = extra_files(&ref_from_name("map.onCreate"));
        assert_eq!(files.len(), 1);
        assert_eq!(
            files[0].0,
            PathBuf::from("idm/sync/map/behaviour/tsconfig.json")
        );
        assert!(files[0].1.contains("../../../types/rhino-1.7.14.d.ts"));
        assert!(files[0].1.contains("../../../types/common.d.ts"));
        assert!(files[0].1.contains("../../../types/managed/*.d.ts"));
        assert!(files[0].1.contains("../../../types/sync/_shared.d.ts"));
        assert!(
            files[0]
                .1
                .contains("../../../types/sync/map.behaviour.d.ts")
        );
        assert!(!files[0].1.contains("sync-mapping.d.ts"));
        assert!(!files[0].1.contains("managed-hook.d.ts"));

        let transform = extra_files(&ref_from_name("map.transform.profile/name"));
        assert_eq!(
            transform[0].0,
            PathBuf::from("idm/sync/map/transform/tsconfig.json")
        );
        assert!(
            transform[0]
                .1
                .contains("../../../types/sync/map.transform.d.ts")
        );
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
    fn slot_lookup_errors_name_mapping_slot_and_property_problems() {
        let mut doc = json!({"mappings": [{
            "name": "map",
            "onCreate": {"type": "text/javascript", "source": "x"},
            "properties": [{"target": "name", "source": "cn",
                "transform": {"type": "text/javascript", "source": "return source;"}
            }]
        }]});

        let event = parse_name("map.onCreate").unwrap();
        assert!(slot_value_mut(&mut doc, &event).is_ok());

        let missing_slot = parse_name("map.onUpdate").unwrap();
        assert!(
            slot_value_mut(&mut doc, &missing_slot)
                .unwrap_err()
                .to_string()
                .contains("map.onUpdate")
        );

        let missing_property = parse_name("map.transform.sn").unwrap();
        let error = slot_value_mut(&mut doc, &missing_property)
            .unwrap_err()
            .to_string();
        assert!(error.contains("sn"));
        assert!(error.contains("map"));

        let missing_mapping = parse_name("ghost.onCreate").unwrap();
        assert!(
            slot_value_mut(&mut doc, &missing_mapping)
                .unwrap_err()
                .to_string()
                .contains("ghost")
        );
    }

    #[test]
    fn remove_slot_drops_only_the_named_script_key() {
        let mut doc = json!({"mappings": [{
            "name": "map",
            "onCreate": {"type": "text/javascript", "source": "x"},
            "properties": [{"target": "name", "source": "cn",
                "transform": {"type": "text/javascript", "source": "return source;"},
                "condition": {"type": "text/javascript", "source": "true"}
            }]
        }]});

        remove_slot(&mut doc, &parse_name("map.transform.name").unwrap()).unwrap();
        let property = &doc["mappings"][0]["properties"][0];
        assert!(property.get("transform").is_none());
        assert!(property.get("condition").is_some());
        assert_eq!(property["target"], json!("name"));
        assert_eq!(property["source"], json!("cn"));

        remove_slot(&mut doc, &parse_name("map.onCreate").unwrap()).unwrap();
        assert!(doc["mappings"][0].get("onCreate").is_none());
    }

    #[test]
    fn remove_slot_refuses_file_backed_scripts() {
        let mut doc = json!({"mappings": [{
            "name": "map",
            "onDelete": {"type": "text/javascript", "file": "sync/onDelete.js"}
        }]});
        let error = remove_slot(&mut doc, &parse_name("map.onDelete").unwrap())
            .unwrap_err()
            .to_string();
        assert!(error.contains("file-backed"));
        assert!(doc["mappings"][0].get("onDelete").is_some());
    }
}
