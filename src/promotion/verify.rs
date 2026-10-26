//! `aic promotion verify`, tenant-free half: which live document a report
//! item names, how two copies are compared, and what the comparison means.
//!
//! The provisional report is the promotion service's claim, not a diff — on
//! 2026-10-26 it listed 150 items that were byte-identical on both tenants
//! (`docs/api/23-promotion.md`). So every item is fetched from **both**
//! tenants and judged on content, whatever the report says its change kind is:
//! a "modified" item that is identical is in sync, and a "deleted" item absent
//! from both is too.
//!
//! An item this module cannot read is `Unchecked`, never `Same`: a verifier
//! that counted what it could not see as matching would recreate the defect it
//! exists to expose.

use serde_json::{Map, Value};

/// One entry of a report, flattened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub section: String,
    pub item: String,
    pub change: String,
    pub name: String,
    pub realm: Option<String>,
    pub uid: Option<String>,
}

/// Flatten a report. Shape errors are errors, as in `spec::summarise`.
pub fn entries(report: &Value) -> Result<Vec<Entry>, String> {
    let sections = report
        .get("report")
        .and_then(Value::as_object)
        .ok_or_else(|| "the report has no `report` object".to_string())?;
    let mut out = Vec::new();
    for (section, items) in sections {
        let items = items
            .as_array()
            .ok_or_else(|| format!("report section {section} is not an array"))?;
        for item in items {
            let name = str_field(item, "configItem")?;
            let changes = item
                .get("configChange")
                .and_then(Value::as_object)
                .ok_or_else(|| format!("{section} / {name} has no `configChange` object"))?;
            for (change, list) in changes {
                let list = list
                    .as_array()
                    .ok_or_else(|| format!("{section} / {name} / {change} is not an array"))?;
                for entry in list {
                    out.push(Entry {
                        section: section.clone(),
                        item: name.clone(),
                        change: change.clone(),
                        name: str_field(entry, "name")?,
                        realm: opt_str(entry, "realm")?,
                        uid: opt_str(entry, "uid")?,
                    });
                }
            }
        }
    }
    Ok(out)
}

fn str_field(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("no string `{key}` in {value}"))
}

fn opt_str(value: &Value, key: &str) -> Result<Option<String>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(format!("`{key}` is not a string: {other}")),
    }
}

/// The live document a report entry names. Each variant's path was measured
/// 2026-10-26 (`docs/api/23-promotion.md`, "Verifying a report").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The tree **and every node it reaches**, page-node children included.
    Journey {
        realm: String,
        name: String,
    },
    Script {
        realm: String,
        id: String,
    },
    /// A script the report lists with no `uid` (seen 2026-10-26), found by
    /// an exact `name eq` query.
    ScriptByName {
        realm: String,
        name: String,
    },
    SecretMapping {
        realm: String,
        secret_id: String,
    },
    /// A realm service, addressed by its display name (`AM Services >
    /// <name>`); the id comes from the realm's service list.
    RealmService {
        realm: String,
        service: String,
    },
    AuthenticationSettings {
        realm: String,
    },
    /// A CORS configuration. The report lowercases its id (`client-a`
    /// for `Client-A`, measured), so it is matched without case
    /// against the list.
    Cors {
        name: String,
    },
    /// A realm's **whole** theme list in `ui/themerealm`. The report's theme
    /// name does not match the stored one ("Starter" for "Starter Theme",
    /// measured), so rather than guess which theme it means, the realm's
    /// list is compared entire — equal lists cover every theme in them.
    Themes {
        realm: String,
    },
    Unchecked(String),
}

/// AM reports realms as `/alpha`, IDM as `alpha`. Only a plain sub-realm name
/// is addressable here; the root realm and nested paths are not.
fn realm(entry: &Entry) -> Result<String, String> {
    let raw = entry
        .realm
        .as_deref()
        .ok_or_else(|| "the report gives no realm".to_string())?;
    let name = raw.strip_prefix('/').unwrap_or(raw);
    if name.is_empty() || name.contains('/') {
        return Err(format!("realm {raw:?} is not a sub-realm this can address"));
    }
    Ok(name.to_string())
}

/// `ou=<uuid>,ou=scriptConfigurations,…` → `<uuid>`.
fn script_id(uid: &str) -> Option<String> {
    let first = uid.split(',').next()?.strip_prefix("ou=")?;
    let ok = first.len() == 36 && first.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    ok.then(|| first.to_string())
}

pub fn target(entry: &Entry) -> Target {
    let realm_of = || realm(entry);
    let unchecked = |why: String| Target::Unchecked(why);
    match entry.item.as_str() {
        "Journeys" => match realm_of() {
            Ok(realm) => Target::Journey {
                realm,
                name: entry.name.clone(),
            },
            Err(why) => unchecked(why),
        },
        "Scripts" => match (realm_of(), entry.uid.as_deref()) {
            (Err(why), _) => unchecked(why),
            (Ok(realm), None) => Target::ScriptByName {
                realm,
                name: entry.name.clone(),
            },
            (Ok(realm), Some(uid)) => match script_id(uid) {
                Some(id) => Target::Script { realm, id },
                None => unchecked(format!("no script id in the report's uid {uid:?}")),
            },
        },
        "ESV Secrets Store Mappings" => match realm_of() {
            Ok(realm) => Target::SecretMapping {
                realm,
                secret_id: entry.name.clone(),
            },
            Err(why) => unchecked(why),
        },
        "AM Authentication > Settings" => match realm_of() {
            Ok(realm) => Target::AuthenticationSettings { realm },
            Err(why) => unchecked(why),
        },
        "CORS" => Target::Cors {
            name: entry.name.clone(),
        },
        "IDM Themes By Realm" => match realm_of() {
            Ok(realm) => Target::Themes { realm },
            Err(why) => unchecked(why),
        },
        item => match item.strip_prefix("AM Services > ") {
            Some(service) => match realm_of() {
                Ok(realm) => Target::RealmService {
                    realm,
                    service: service.to_string(),
                },
                Err(why) => unchecked(why),
            },
            None => unchecked(format!("no reader for {item:?}")),
        },
    }
}

/// Fields that describe a copy rather than the configuration: AM's `_rev`
/// anywhere, and a script's audit stamps. Everything else is compared.
const VOLATILE: &[&str] = &[
    "_rev",
    "createdBy",
    "creationDate",
    "lastModifiedBy",
    "lastModifiedDate",
];

pub fn normalise(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| !VOLATILE.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), normalise(v)))
                .collect::<Map<_, _>>(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(normalise).collect()),
        other => other.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Same,
    /// Up to `MAX_PATHS` JSON paths where the copies differ.
    Differs(Vec<String>),
    OnlyLower,
    OnlyUpper,
    /// Absent from both — in sync, e.g. a "deleted" item already gone.
    Neither,
    Unchecked(String),
}

impl Verdict {
    pub fn in_sync(&self) -> bool {
        matches!(self, Verdict::Same | Verdict::Neither)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Verdict::Same => "same",
            Verdict::Neither => "absent-both",
            Verdict::Differs(_) => "DIFFERS",
            Verdict::OnlyLower => "ONLY-LOWER",
            Verdict::OnlyUpper => "ONLY-UPPER",
            Verdict::Unchecked(_) => "unchecked",
        }
    }
}

pub const MAX_PATHS: usize = 5;

/// `None` is "the tenant says it does not exist" (a measured 404), never "could
/// not read" — that is an error before this is reached.
pub fn judge(lower: Option<&Value>, upper: Option<&Value>) -> Verdict {
    match (lower, upper) {
        (None, None) => Verdict::Neither,
        (Some(_), None) => Verdict::OnlyLower,
        (None, Some(_)) => Verdict::OnlyUpper,
        (Some(l), Some(u)) => {
            let mut paths = Vec::new();
            diff("", &normalise(l), &normalise(u), &mut paths);
            if paths.is_empty() {
                Verdict::Same
            } else {
                Verdict::Differs(paths)
            }
        }
    }
}

fn diff(at: &str, l: &Value, u: &Value, out: &mut Vec<String>) {
    if out.len() >= MAX_PATHS || l == u {
        return;
    }
    match (l, u) {
        (Value::Object(a), Value::Object(b)) => {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                let here = format!("{at}/{key}");
                match (a.get(key), b.get(key)) {
                    (Some(x), Some(y)) => diff(&here, x, y, out),
                    (Some(_), None) => out.push(format!("{here} (lower only)")),
                    (None, Some(_)) => out.push(format!("{here} (upper only)")),
                    (None, None) => {}
                }
                if out.len() >= MAX_PATHS {
                    return;
                }
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                diff(&format!("{at}/{i}"), x, y, out);
            }
        }
        _ => out.push(if at.is_empty() { "/".into() } else { at.into() }),
    }
}

/// Whether two configured base URLs name one tenant: compared without case
/// or a trailing slash, so `verify` cannot pass by comparing a tenant with
/// itself under two context names.
pub fn same_tenant(a: &str, b: &str) -> bool {
    a.trim_end_matches('/')
        .eq_ignore_ascii_case(b.trim_end_matches('/'))
}

/// A realm's theme list out of `ui/themerealm`. Absent realm → `Ok(None)`.
pub fn themes<'a>(themerealm: &'a Value, realm: &str) -> Result<Option<&'a Value>, String> {
    match themerealm.get("realm").and_then(|r| r.get(realm)) {
        None | Some(Value::Null) => Ok(None),
        Some(list @ Value::Array(_)) => Ok(Some(list)),
        Some(other) => Err(format!("themerealm.realm.{realm} is not an array: {other}")),
    }
}

/// The one entry of a list whose `field` equals `want` ignoring ASCII case.
/// None → `Ok(None)`; several → an error, never a pick.
pub fn one_by<'a>(list: &'a Value, field: &str, want: &str) -> Result<Option<&'a Value>, String> {
    let result = list
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("list has no `result` array: {list}"))?;
    let hits: Vec<&Value> = result
        .iter()
        .filter(|v| {
            v.get(field)
                .and_then(Value::as_str)
                .is_some_and(|s| s.eq_ignore_ascii_case(want))
        })
        .collect();
    match hits.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(one)),
        _ => Err(format!("{} entries match {field} {want:?}", hits.len())),
    }
}

/// A CREST `name eq "…"` filter, with `\\` and `"` escaped.
pub fn name_filter(name: &str) -> String {
    format!(
        "name eq \"{}\"",
        name.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

/// Absent from both tenants is in sync only when the report calls the item
/// deleted. For an item it calls added or modified, absence everywhere says
/// the lookup did not find what the report meant — the failure that once
/// passed three CORS entries and a theme as in sync — so it is unchecked.
pub fn settle(change: &str, verdict: Verdict) -> Verdict {
    match verdict {
        Verdict::Neither if change != "deleted" => Verdict::Unchecked(format!(
            "reported {change} but found on neither tenant; the lookup may not match the report's name"
        )),
        other => other,
    }
}

/// The realm-service id for the report's display name, from the realm's
/// service list (`{result: [{_id, name}]}`). The report says "Session
/// Service" where the list says "Session", and "Validation Service" where it
/// says "Validation Service" (measured 2026-10-26), so the name is tried as
/// given and without a trailing " Service". Two different matches are an
/// error, not a guess. No match → `Ok(None)`: not configured in that realm.
pub fn service_id(list: &Value, service: &str) -> Result<Option<String>, String> {
    let result = list
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("realm service list has no `result` array: {list}"))?;
    let mut names = vec![service];
    if let Some(short) = service.strip_suffix(" Service") {
        names.push(short);
    }
    let mut ids: Vec<&str> = result
        .iter()
        .filter(|s| {
            s.get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| names.contains(&n))
        })
        .map(|s| {
            s.get("_id")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("realm service without a string _id: {s}"))
        })
        .collect::<Result<_, _>>()?;
    ids.sort_unstable();
    ids.dedup();
    match ids.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some((*one).to_string())),
        _ => Err(format!("{service:?} names several realm services: {ids:?}")),
    }
}

/// Every `(nodeType, _id)` a tree or a page node lists. A tree carries them
/// as an object keyed by id; a page node as an array of `{_id, nodeType}`.
pub fn node_refs(doc: &Value) -> Result<Vec<(String, String)>, String> {
    match doc.get("nodes") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Object(map)) => map
            .iter()
            .map(|(id, meta)| {
                let ty = meta
                    .get("nodeType")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("node {id} has no string nodeType"))?;
                Ok((ty.to_string(), id.clone()))
            })
            .collect(),
        Some(Value::Array(list)) => list
            .iter()
            .map(|child| {
                let id = child.get("_id").and_then(Value::as_str);
                let ty = child.get("nodeType").and_then(Value::as_str);
                match (ty, id) {
                    (Some(ty), Some(id)) => Ok((ty.to_string(), id.to_string())),
                    _ => Err(format!("page-node child without _id/nodeType: {child}")),
                }
            })
            .collect(),
        Some(other) => Err(format!(
            "`nodes` is neither an object nor an array: {other}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(item: &str, name: &str, realm: Option<&str>, uid: Option<&str>) -> Entry {
        Entry {
            section: "AMConfig".into(),
            item: item.into(),
            change: "modified".into(),
            name: name.into(),
            realm: realm.map(str::to_string),
            uid: uid.map(str::to_string),
        }
    }

    #[test]
    fn report_items_map_to_their_measured_targets() {
        let uid = "ou=00000000-0000-4000-8000-000000000001,ou=scriptConfigurations,ou=default";
        assert_eq!(
            target(&entry("Scripts", "S", Some("/alpha"), Some(uid))),
            Target::Script {
                realm: "alpha".into(),
                id: "00000000-0000-4000-8000-000000000001".into()
            }
        );
        assert_eq!(
            target(&entry(
                "AM Services > Session Service",
                "opendj",
                Some("/bravo"),
                None
            )),
            Target::RealmService {
                realm: "bravo".into(),
                service: "Session Service".into()
            }
        );
        assert_eq!(
            target(&entry("CORS", "web", None, None)),
            Target::Cors { name: "web".into() }
        );
        assert_eq!(
            target(&entry("Scripts", "S", Some("/bravo"), None)),
            Target::ScriptByName {
                realm: "bravo".into(),
                name: "S".into()
            }
        );
    }

    /// What cannot be read is never counted as matching.
    #[test]
    fn unknown_items_and_unaddressable_realms_are_unchecked() {
        assert!(matches!(
            target(&entry("Global Scripts", "T", None, None)),
            Target::Unchecked(_)
        ));
        assert!(matches!(
            target(&entry("Journeys", "J", Some("/"), None)),
            Target::Unchecked(_)
        ));
        assert!(matches!(
            target(&entry("Journeys", "J", None, None)),
            Target::Unchecked(_)
        ));
        assert!(matches!(
            target(&entry(
                "Scripts",
                "S",
                Some("/alpha"),
                Some("ou=not-a-uuid,ou=x")
            )),
            Target::Unchecked(_)
        ));
        assert!(!Verdict::Unchecked("x".into()).in_sync());
    }

    #[test]
    fn judge_ignores_copy_metadata_and_names_the_differing_paths() {
        let lower = json!({"_rev": "1", "script": "a", "lastModifiedDate": 1, "nodes": {"n": {"x": 1, "_rev": "9"}}});
        let same = json!({"_rev": "2", "script": "a", "lastModifiedDate": 2, "nodes": {"n": {"x": 1, "_rev": "8"}}});
        assert_eq!(judge(Some(&lower), Some(&same)), Verdict::Same);
        let other = json!({"script": "b", "nodes": {"n": {"x": 2}}, "extra": true});
        assert_eq!(
            judge(Some(&lower), Some(&other)),
            Verdict::Differs(vec![
                "/extra (upper only)".into(),
                "/nodes/n/x".into(),
                "/script".into()
            ])
        );
    }

    #[test]
    fn presence_is_judged_whatever_the_report_called_the_change() {
        let v = json!({"a": 1});
        assert_eq!(judge(None, None), Verdict::Neither);
        assert!(Verdict::Neither.in_sync());
        assert_eq!(judge(Some(&v), None), Verdict::OnlyLower);
        assert_eq!(judge(None, Some(&v)), Verdict::OnlyUpper);
    }

    #[test]
    fn themes_are_compared_by_whole_realm_list() {
        let doc = json!({"realm": {"alpha": [{"name": "Starter Theme"}]}});
        assert_eq!(
            target(&entry(
                "IDM Themes By Realm",
                "Starter",
                Some("alpha"),
                None
            )),
            Target::Themes {
                realm: "alpha".into()
            }
        );
        assert_eq!(
            themes(&doc, "alpha").unwrap(),
            Some(&json!([{"name": "Starter Theme"}]))
        );
        assert_eq!(themes(&doc, "bravo").unwrap(), None);
        assert!(themes(&json!({"realm": {"alpha": {}}}), "alpha").is_err());
    }

    #[test]
    fn one_by_matches_without_case_and_refuses_ambiguity() {
        let list = json!({"result": [{"_id": "Client-A"}, {"_id": "Other"}]});
        assert_eq!(
            one_by(&list, "_id", "client-a").unwrap(),
            Some(&json!({"_id": "Client-A"}))
        );
        assert_eq!(one_by(&list, "_id", "missing").unwrap(), None);
        let twice = json!({"result": [{"_id": "A"}, {"_id": "a"}]});
        assert!(one_by(&twice, "_id", "a").is_err());
        assert!(one_by(&json!({}), "_id", "a").is_err());
    }

    #[test]
    fn name_filter_escapes_quotes() {
        assert_eq!(name_filter(r#"a"b\c"#), r#"name eq "a\"b\\c""#);
    }

    /// The discriminating case: absent everywhere for a reported modify is
    /// not in sync.
    #[test]
    fn absent_from_both_is_in_sync_only_for_a_reported_delete() {
        assert_eq!(settle("deleted", Verdict::Neither), Verdict::Neither);
        assert!(!settle("modified", Verdict::Neither).in_sync());
        assert!(!settle("added", Verdict::Neither).in_sync());
        assert_eq!(settle("modified", Verdict::Same), Verdict::Same);
    }

    #[test]
    fn service_ids_resolve_from_the_measured_list() {
        let list = json!({"result": [
            {"_id": "pushNotification", "name": "Push Notification Service"},
            {"_id": "session", "name": "Session"}
        ]});
        assert_eq!(
            service_id(&list, "Push Notification Service").unwrap(),
            Some("pushNotification".into())
        );
        assert_eq!(
            service_id(&list, "Session Service").unwrap(),
            Some("session".into())
        );
        assert_eq!(service_id(&list, "Validation Service").unwrap(), None);
        assert!(service_id(&json!({}), "x").is_err());
        let both =
            json!({"result": [{"_id": "a", "name": "X Service"}, {"_id": "b", "name": "X"}]});
        assert!(service_id(&both, "X Service").is_err());
    }

    #[test]
    fn node_refs_read_trees_and_page_nodes() {
        let tree = json!({"nodes": {"id1": {"nodeType": "PageNode"}}});
        assert_eq!(
            node_refs(&tree).unwrap(),
            vec![("PageNode".into(), "id1".into())]
        );
        let page = json!({"nodes": [{"_id": "c1", "nodeType": "ScriptedDecisionNode"}]});
        assert_eq!(
            node_refs(&page).unwrap(),
            vec![("ScriptedDecisionNode".into(), "c1".into())]
        );
        assert_eq!(node_refs(&json!({"x": 1})).unwrap(), vec![]);
        assert!(node_refs(&json!({"nodes": [{"_id": "c1"}]})).is_err());
    }

    #[test]
    fn same_tenant_ignores_case_and_a_trailing_slash() {
        assert!(same_tenant(
            "https://tenant.example.com/",
            "https://TENANT.example.com"
        ));
        assert!(!same_tenant(
            "https://tenant.example.com",
            "https://other.example.com"
        ));
    }

    #[test]
    fn entries_flatten_and_refuse_a_bad_shape() {
        let report = json!({"report": {"AMConfig": [{"configItem": "Journeys", "configChange": {
            "added": [{"name": "A", "realm": "/bravo"}], "deleted": [{"name": "B", "realm": "/alpha"}]}}]}});
        let got = entries(&report).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[1].change, "deleted");
        assert!(entries(&json!({"report": {"AMConfig": [{"configItem": "J", "configChange": {"added": [{"realm": 1, "name": "x"}]}}]}})).is_err());
    }
}
