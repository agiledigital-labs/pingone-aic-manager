//! `aic managed relationship verify` — the tenant-free half.
//!
//! A relationship's two ends are declared in `config/managed`, and may also be
//! mapped in `config/repo.ds` (`resourceMapping.genericMapping."managed/<obj>"
//! .properties.<key>`). Both modes were measured to work; what was measured to
//! break is a pair whose ends disagree — one end mapped, the other not, after a
//! rename through `config/managed` (`docs/api/10-managed-objects.md`,
//! "Relationship storage"). So this module reports disagreement, never mode
//! alone, and judges sampled links by the one property both modes share: the
//! forward and reverse reads of a link carry the same `_refProperties._id`.
//!
//! Nothing here performs I/O; `cli.rs` reads the two documents and the sampled
//! records and hands them in.

use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;

use crate::managed::ops::declared_reverse_key;

/// One end of a relationship as `config/managed` and `repo.ds` describe it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct End {
    pub object: String,
    pub key: String,
    /// `managed/<obj>` paths from `resourceCollection` (through `items`).
    pub targets: Vec<String>,
    pub array: bool,
    pub mapping: Option<Mapping>,
}

/// A `repo.ds` relationship mapping entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Mapping {
    /// `reference` or `reverseReference`.
    pub kind: String,
    pub resource_path: Option<String>,
    /// Set on a `reverseReference`: the partner key it reads through.
    pub property_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "problem", rename_all = "snake_case")]
pub enum Problem {
    /// The forward end names a reverse that its target does not declare.
    ReverseMissing { object: String, key: String },
    /// The named reverse exists but is not a relationship back to the forward.
    ReverseNotReciprocal { object: String, key: String },
    /// Exactly one end is mapped: the measured broken state.
    Mixed { mapped: String, unmapped: String },
    /// Both ends mapped with the same kind; one must hold the reference and
    /// the other read it back.
    MappingKinds { kind: String },
    /// A mapping's `resourcePath` is not among the end's declared targets.
    MappingTarget {
        end: String,
        mapped: String,
        declared: Vec<String>,
    },
    /// A `reverseReference` reads through a key other than its partner's.
    MappingPartner {
        end: String,
        names: String,
        expected: String,
    },
    /// A `repo.ds` entry for a property `config/managed` does not declare.
    StaleMapping { object: String, key: String },
}

impl Problem {
    pub fn message(&self) -> String {
        match self {
            Self::ReverseMissing { object, key } => {
                format!("reverse {object}.{key} is not declared")
            }
            Self::ReverseNotReciprocal { object, key } => {
                format!("reverse {object}.{key} is not a relationship back to this end")
            }
            Self::Mixed { mapped, unmapped } => format!(
                "{mapped} is mapped in repo.ds but {unmapped} is not; reads and writes on the \
                 two ends can go to different stores"
            ),
            Self::MappingKinds { kind } => {
                format!("both ends are mapped as `{kind}`; one should hold the reference")
            }
            Self::MappingTarget {
                end,
                mapped,
                declared,
            } => format!(
                "{end}'s repo.ds mapping points at {mapped}, but it is declared to target {}",
                declared.join(", ")
            ),
            Self::MappingPartner {
                end,
                names,
                expected,
            } => format!("{end}'s repo.ds mapping reads through {names}, expected {expected}"),
            Self::StaleMapping { object, key } => {
                format!("repo.ds maps {object}.{key}, which config/managed does not declare")
            }
        }
    }
}

/// A relationship pair, or a lone forward end, and what disagrees about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairReport {
    pub forward: End,
    pub reverse: Option<End>,
    pub problems: Vec<Problem>,
}

impl PairReport {
    /// Both ends exist, point at each other, and are worth sampling.
    pub fn sampleable(&self) -> bool {
        self.reverse.is_some()
            && !self.problems.iter().any(|problem| {
                matches!(
                    problem,
                    Problem::ReverseMissing { .. } | Problem::ReverseNotReciprocal { .. }
                )
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub pairs: Vec<PairReport>,
    pub stale: Vec<Problem>,
}

fn mapping_for(repo_ds: &Value, object: &str, key: &str) -> Option<Mapping> {
    let entry = mappings_of(repo_ds, object)?.get(key)?;
    let kind = entry.get("type").and_then(Value::as_str)?;
    if kind != "reference" && kind != "reverseReference" {
        return None;
    }
    Some(Mapping {
        kind: kind.to_string(),
        resource_path: entry
            .get("resourcePath")
            .and_then(Value::as_str)
            .map(str::to_string),
        property_name: entry
            .get("propertyName")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn mappings_of<'a>(repo_ds: &'a Value, object: &str) -> Option<&'a serde_json::Map<String, Value>> {
    repo_ds
        .get("resourceMapping")?
        .get("genericMapping")?
        .get(format!("managed/{object}"))?
        .get("properties")?
        .as_object()
}

fn properties<'a>(managed: &'a Value, object: &str) -> Option<&'a serde_json::Map<String, Value>> {
    let def = crate::managed::api::object_named(managed, object).ok()?;
    crate::managed::state::properties(def)
}

fn end(managed: &Value, repo_ds: &Value, object: &str, key: &str) -> Option<End> {
    let property = properties(managed, object)?.get(key)?;
    if !crate::managed::state::is_relationship_property(property) {
        return None;
    }
    let node = property.get("items").unwrap_or(property);
    let targets = node
        .get("resourceCollection")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("path").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Some(End {
        object: object.to_string(),
        key: key.to_string(),
        targets,
        array: property.get("type").and_then(Value::as_str) == Some("array"),
        mapping: mapping_for(repo_ds, object, key),
    })
}

fn label(end: &End) -> String {
    format!("{}.{}", end.object, end.key)
}

fn mapping_problems(end: &End, partner: Option<&End>, out: &mut Vec<Problem>) {
    let Some(mapping) = &end.mapping else {
        return;
    };
    if let Some(path) = &mapping.resource_path
        && !end.targets.iter().any(|target| target == path)
    {
        out.push(Problem::MappingTarget {
            end: label(end),
            mapped: path.clone(),
            declared: end.targets.clone(),
        });
    }
    if mapping.kind == "reverseReference" {
        let names = mapping.property_name.clone().unwrap_or_default();
        let expected = partner.map(|partner| partner.key.clone());
        if expected.as_deref() != Some(names.as_str()) {
            out.push(Problem::MappingPartner {
                end: label(end),
                names,
                expected: expected.unwrap_or_else(|| "a declared reverse".into()),
            });
        }
    }
}

/// Builds the report for one relationship end and its declared reverse.
pub fn pair(managed: &Value, repo_ds: &Value, object: &str, key: &str) -> Option<PairReport> {
    let forward = end(managed, repo_ds, object, key)?;
    let property = properties(managed, object)?.get(key)?;
    let reverse_key = declared_reverse_key(property).map(str::to_string);
    let mut problems = Vec::new();
    let mut reverse = None;
    if let Some(reverse_key) = reverse_key {
        // A multi-target forward names one key that every target would have
        // to hold; this checks the first managed one, whatever its position.
        let target_object = forward
            .targets
            .iter()
            .find_map(|path| path.strip_prefix("managed/"))
            .map(str::to_string);
        match target_object
            .as_deref()
            .and_then(|target| properties(managed, target).map(|props| (target, props)))
            .and_then(|(target, props)| props.get(&reverse_key).map(|prop| (target, prop)))
        {
            None => problems.push(Problem::ReverseMissing {
                object: target_object.unwrap_or_default(),
                key: reverse_key,
            }),
            Some((target, prop)) => {
                let back = format!("managed/{object}");
                let reciprocal = declared_reverse_key(prop).is_none_or(|k| k == key)
                    && end(managed, repo_ds, target, &reverse_key)
                        .is_some_and(|end| end.targets.contains(&back));
                if reciprocal {
                    reverse = end(managed, repo_ds, target, &reverse_key);
                } else {
                    problems.push(Problem::ReverseNotReciprocal {
                        object: target.to_string(),
                        key: reverse_key,
                    });
                }
            }
        }
    }
    mapping_problems(&forward, reverse.as_ref(), &mut problems);
    if let Some(reverse) = &reverse {
        mapping_problems(reverse, Some(&forward), &mut problems);
        match (&forward.mapping, &reverse.mapping) {
            (Some(_), None) => problems.push(Problem::Mixed {
                mapped: label(&forward),
                unmapped: label(reverse),
            }),
            (None, Some(_)) => problems.push(Problem::Mixed {
                mapped: label(reverse),
                unmapped: label(&forward),
            }),
            (Some(a), Some(b)) if a.kind == b.kind => problems.push(Problem::MappingKinds {
                kind: a.kind.clone(),
            }),
            _ => {}
        }
    }
    Some(PairReport {
        forward,
        reverse,
        problems,
    })
}

/// Every relationship on `object` (or just `key`), each pair once, plus
/// `repo.ds` entries on `object` that name no declared property.
pub fn report(
    managed: &Value,
    repo_ds: &Value,
    object: &str,
    key: Option<&str>,
) -> Result<Report, String> {
    let props = properties(managed, object)
        .ok_or_else(|| format!("managed object '{object}' not found"))?;
    let mut stale = Vec::new();
    if let Some(mapped) = mappings_of(repo_ds, object) {
        for mapped_key in mapped.keys() {
            if key.is_none_or(|key| key == mapped_key) && !props.contains_key(mapped_key) {
                stale.push(Problem::StaleMapping {
                    object: object.to_string(),
                    key: mapped_key.clone(),
                });
            }
        }
    }
    let keys: Vec<&str> = match key {
        Some(key) => {
            let Some(property) = props.get(key) else {
                if stale.is_empty() {
                    return Err(format!("'{object}.{key}' is not declared"));
                }
                return Ok(Report {
                    pairs: Vec::new(),
                    stale,
                });
            };
            if !crate::managed::state::is_relationship_property(property) {
                return Err(format!("'{object}.{key}' is not a relationship"));
            }
            vec![key]
        }
        None => props
            .iter()
            .filter(|(_, property)| crate::managed::state::is_relationship_property(property))
            .map(|(key, _)| key.as_str())
            .collect(),
    };
    let mut seen = BTreeSet::new();
    let mut pairs = Vec::new();
    for key in keys {
        let Some(pair) = pair(managed, repo_ds, object, key) else {
            continue;
        };
        let mut identity = vec![label(&pair.forward)];
        if let Some(reverse) = &pair.reverse {
            identity.push(label(reverse));
        }
        identity.sort();
        if seen.insert(identity) {
            pairs.push(pair);
        }
    }
    Ok(Report { pairs, stale })
}

/// `object.key`'s `repo.ds` mapping, if it has one.
pub fn mapped(repo_ds: &Value, object: &str, key: &str) -> Option<Mapping> {
    mapping_for(repo_ds, object, key)
}

/// Refuses moving a mapped relationship end to another target.
///
/// Measured 2026-10-26: after repointing a mapped end through
/// `config/managed`, a link to a record of the new target was `400 … does not
/// exist` — it resolved against the mapping's old `resourcePath` — and that
/// failed write still removed the end's existing link. `repo.ds` is not
/// something `aic` writes, so there is no way to move the mapping with it.
pub fn repoint_refusal(
    repo_ds: &Value,
    source_object: &str,
    key: &str,
    old_target: &str,
    old_reverse_key: Option<&str>,
    new_target: &str,
) -> Result<(), String> {
    if old_target == new_target {
        return Ok(());
    }
    let mut ends = Vec::new();
    if mapped(repo_ds, source_object, key).is_some() {
        ends.push(format!("{source_object}.{key}"));
    }
    if let Some(reverse_key) = old_reverse_key
        && mapped(repo_ds, old_target, reverse_key).is_some()
    {
        ends.push(format!("{old_target}.{reverse_key}"));
    }
    if ends.is_empty() {
        return Ok(());
    }
    Err(format!(
        "refusing to retarget {source_object}.{key} from {old_target} to {new_target}: {} \
         mapped in config/repo.ds, which still names managed/{old_target}. A mapped end that \
         was retargeted rejected links to the new target and lost its existing link \
         (docs/api/10-managed-objects.md, \"Relationship storage\"). Create a new relationship \
         instead",
        describe_ends(&ends)
    ))
}

/// Refuses writing a relationship end over a `repo.ds` mapping that names a
/// different target — a mapping a delete left behind. Recreating with the
/// same target reused it (measured); a different target is the state a
/// retarget was measured to break.
pub fn leftover_refusal(repo_ds: &Value, ends: &[(&str, &str, &str)]) -> Result<(), String> {
    let conflicts: Vec<String> = ends
        .iter()
        .filter_map(|(object, key, target)| {
            let path = mapped(repo_ds, object, key)?.resource_path?;
            (path != format!("managed/{target}"))
                .then(|| format!("{object}.{key} (mapped to {path}, now to managed/{target})"))
        })
        .collect();
    if conflicts.is_empty() {
        return Ok(());
    }
    Err(format!(
        "refusing: config/repo.ds already maps {} to another target, and aic cannot change \
         it. Use another key, or repair the mapping in the console",
        conflicts.join(", ")
    ))
}

/// Refuses renaming an object that a `repo.ds` relationship mapping names,
/// as its own `managed/<object>` entry or as another mapping's
/// `resourcePath`. Inferred, not measured: the mapping would go on naming the
/// old path, which is the state a retarget was measured to break.
pub fn rename_refusal(repo_ds: &Value, object: &str) -> Result<(), String> {
    let path = format!("managed/{object}");
    let mut ends: Vec<String> = mappings_of(repo_ds, object)
        .into_iter()
        .flat_map(|mappings| mappings.keys())
        .filter(|key| mapped(repo_ds, object, key).is_some())
        .map(|key| format!("{object}.{key}"))
        .collect();
    if let Some(generic) = repo_ds
        .pointer("/resourceMapping/genericMapping")
        .and_then(Value::as_object)
    {
        for (resource, entry) in generic {
            let Some(other) = resource.strip_prefix("managed/") else {
                continue;
            };
            if other == object {
                continue;
            }
            for (key, _) in entry
                .get("properties")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                if mapped(repo_ds, other, key)
                    .is_some_and(|mapping| mapping.resource_path.as_deref() == Some(&path))
                {
                    ends.push(format!("{other}.{key}"));
                }
            }
        }
    }
    if ends.is_empty() {
        return Ok(());
    }
    Err(format!(
        "refusing to rename {object}: config/repo.ds maps relationships by its name ({}), and \
         aic cannot move those mappings. Renaming would leave them naming managed/{object}",
        ends.join(", ")
    ))
}

/// What a relationship delete leaves behind in `repo.ds`: measured
/// 2026-10-26, the mapping of a deleted mapped end stays, and recreating the
/// same key with the same target picked it up again.
pub fn delete_leftovers(repo_ds: &Value, ends: &[(&str, &str)]) -> Vec<String> {
    ends.iter()
        .filter(|(object, key)| mapped(repo_ds, object, key).is_some())
        .map(|(object, key)| format!("{object}.{key}"))
        .collect()
}

fn describe_ends(ends: &[String]) -> String {
    match ends {
        [one] => format!("{one} is"),
        many => format!("{} are", many.join(" and ")),
    }
}

/// One link read from a record's relationship property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Link {
    pub from_id: String,
    pub to_collection: String,
    pub to_id: String,
    pub edge_id: Option<String>,
}

/// The links a record's `_fields=<key>` read returned, whether the property
/// came back as one object or an array.
pub fn links_of(record: &Value, key: &str) -> Vec<Link> {
    let Some(from_id) = record.get("_id").and_then(Value::as_str) else {
        return Vec::new();
    };
    let entries: Vec<&Value> = match record.get(key) {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(item @ Value::Object(_)) => vec![item],
        _ => Vec::new(),
    };
    entries
        .into_iter()
        .filter_map(|entry| {
            Some(Link {
                from_id: from_id.to_string(),
                to_collection: entry.get("_refResourceCollection")?.as_str()?.to_string(),
                to_id: entry.get("_refResourceId")?.as_str()?.to_string(),
                edge_id: entry
                    .pointer("/_refProperties/_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum LinkOutcome {
    /// The far end lists the near record with the same edge id.
    Resolved,
    /// The far end lists the near record, but under another edge id: two
    /// stores each hold a link.
    EdgeDiffers { far_edges: Vec<String> },
    /// The far end does not list the near record.
    Missing,
    /// The far record does not exist.
    FarRecordMissing,
}

/// Judges one sampled link against what the far end's reverse property
/// returned (`None` when the far record is gone).
pub fn judge(link: &Link, far_entries: Option<&[Link]>) -> LinkOutcome {
    let Some(far_entries) = far_entries else {
        return LinkOutcome::FarRecordMissing;
    };
    let back: Vec<&Link> = far_entries
        .iter()
        .filter(|far| far.to_id == link.from_id)
        .collect();
    if back.is_empty() {
        return LinkOutcome::Missing;
    }
    if back
        .iter()
        .any(|far| far.edge_id.is_some() && far.edge_id == link.edge_id)
    {
        return LinkOutcome::Resolved;
    }
    LinkOutcome::EdgeDiffers {
        far_edges: back.iter().filter_map(|far| far.edge_id.clone()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn managed() -> Value {
        json!({"_id": "managed", "objects": [
            {"name": "a", "schema": {"properties": {
                "owner": {"type": "relationship", "reverseRelationship": true,
                    "reversePropertyName": "owned",
                    "resourceCollection": [{"path": "managed/b"}]},
                "plain": {"type": "string"}
            }}},
            {"name": "b", "schema": {"properties": {
                "owned": {"type": "array", "items": {"type": "relationship",
                    "reverseRelationship": true, "reversePropertyName": "owner",
                    "resourceCollection": [{"path": "managed/a"}]}}
            }}}
        ]})
    }

    fn repo(a: Option<Value>, b: Option<Value>) -> Value {
        let mut generic = serde_json::Map::new();
        if let Some(a) = a {
            generic.insert("managed/a".into(), json!({"properties": {"owner": a}}));
        }
        if let Some(b) = b {
            generic.insert("managed/b".into(), json!({"properties": {"owned": b}}));
        }
        json!({"resourceMapping": {"genericMapping": generic}})
    }

    fn reference() -> Value {
        json!({"type": "reference", "ldapAttribute": "fr-idm-reference-1",
            "resourcePath": "managed/b", "primaryKey": "uid"})
    }

    fn reverse_reference() -> Value {
        json!({"type": "reverseReference", "propertyName": "owner",
            "resourcePath": "managed/a", "isMultiValued": true})
    }

    fn problems(repo_ds: &Value) -> Vec<Problem> {
        report(&managed(), repo_ds, "a", Some("owner"))
            .unwrap()
            .pairs
            .remove(0)
            .problems
    }

    #[test]
    fn generic_and_mapped_pairs_both_pass() {
        assert_eq!(problems(&repo(None, None)), vec![]);
        assert_eq!(
            problems(&repo(Some(reference()), Some(reverse_reference()))),
            vec![]
        );
    }

    #[test]
    fn one_mapped_end_is_mixed_whichever_end_it_is() {
        assert_eq!(
            problems(&repo(Some(reference()), None)),
            vec![Problem::Mixed {
                mapped: "a.owner".into(),
                unmapped: "b.owned".into()
            }]
        );
        assert_eq!(
            problems(&repo(None, Some(reverse_reference()))),
            vec![Problem::Mixed {
                mapped: "b.owned".into(),
                unmapped: "a.owner".into()
            }]
        );
    }

    #[test]
    fn a_reverse_reference_through_another_key_is_reported() {
        let mut stale = reverse_reference();
        stale["propertyName"] = json!("ownerOld");
        assert_eq!(
            problems(&repo(Some(reference()), Some(stale))),
            vec![Problem::MappingPartner {
                end: "b.owned".into(),
                names: "ownerOld".into(),
                expected: "owner".into()
            }]
        );
    }

    #[test]
    fn a_mapping_to_an_undeclared_target_is_reported() {
        let mut moved = reference();
        moved["resourcePath"] = json!("managed/c");
        assert_eq!(
            problems(&repo(Some(moved), Some(reverse_reference()))),
            vec![Problem::MappingTarget {
                end: "a.owner".into(),
                mapped: "managed/c".into(),
                declared: vec!["managed/b".into()]
            }]
        );
    }

    #[test]
    fn two_references_are_reported() {
        let mut both = reference();
        both["resourcePath"] = json!("managed/a");
        assert_eq!(
            problems(&repo(Some(reference()), Some(both))),
            vec![Problem::MappingKinds {
                kind: "reference".into()
            }]
        );
    }

    #[test]
    fn a_mapping_without_a_property_is_stale_and_a_scalar_is_not_a_reverse() {
        let mut doc = managed();
        doc["objects"][1]["schema"]["properties"]["owned"] = json!({"type": "string"});
        let mut repo_ds = repo(Some(reference()), None);
        repo_ds["resourceMapping"]["genericMapping"]["managed/a"]["properties"]["gone"] =
            reference();
        let report = report(&doc, &repo_ds, "a", None).unwrap();
        assert_eq!(
            report.stale,
            vec![Problem::StaleMapping {
                object: "a".into(),
                key: "gone".into()
            }]
        );
        assert_eq!(
            report.pairs[0].problems,
            vec![Problem::ReverseNotReciprocal {
                object: "b".into(),
                key: "owned".into()
            }]
        );
        assert!(!report.pairs[0].sampleable());
    }

    #[test]
    fn a_reverse_naming_this_key_but_another_target_is_not_reciprocal() {
        let mut doc = managed();
        doc["objects"][1]["schema"]["properties"]["owned"]["items"]["resourceCollection"] =
            json!([{"path": "managed/c"}]);
        let report = report(&doc, &json!({}), "a", Some("owner")).unwrap();
        assert_eq!(
            report.pairs[0].problems,
            vec![Problem::ReverseNotReciprocal {
                object: "b".into(),
                key: "owned".into()
            }]
        );
    }

    #[test]
    fn a_forward_listing_internal_role_first_still_finds_its_reverse() {
        let mut doc = managed();
        doc["objects"][0]["schema"]["properties"]["owner"]["resourceCollection"] =
            json!([{"path": "internal/role"}, {"path": "managed/b"}]);
        let report = report(&doc, &json!({}), "a", Some("owner")).unwrap();
        assert_eq!(report.pairs[0].reverse.as_ref().unwrap().key, "owned");
        assert_eq!(report.pairs[0].problems, vec![]);
    }

    #[test]
    fn a_self_referential_pair_is_reported_once() {
        let doc = json!({"_id": "managed", "objects": [{"name": "u", "schema": {"properties": {
            "manager": {"type": "relationship", "reverseRelationship": true,
                "reversePropertyName": "reports", "resourceCollection": [{"path": "managed/u"}]},
            "reports": {"type": "array", "items": {"type": "relationship",
                "reverseRelationship": true, "reversePropertyName": "manager",
                "resourceCollection": [{"path": "managed/u"}]}}
        }}}]});
        let report = report(&doc, &json!({}), "u", None).unwrap();
        assert_eq!(report.pairs.len(), 1);
    }

    #[test]
    fn a_retarget_is_refused_when_either_old_end_is_mapped() {
        let both = repo(Some(reference()), Some(reverse_reference()));
        let forward = repo(Some(reference()), None);
        let reverse = repo(None, Some(reverse_reference()));
        for repo_ds in [&both, &forward, &reverse] {
            let error =
                repoint_refusal(repo_ds, "a", "owner", "b", Some("owned"), "c").unwrap_err();
            assert!(error.contains("from b to c"), "{error}");
        }
        assert!(error_names(&reverse).contains("b.owned is mapped"));
        // Not a retarget, or nothing mapped: allowed.
        assert_eq!(
            repoint_refusal(&both, "a", "owner", "b", Some("owned"), "b"),
            Ok(())
        );
        assert_eq!(
            repoint_refusal(&repo(None, None), "a", "owner", "b", Some("owned"), "c"),
            Ok(())
        );
        // A reverse that was never declared is not consulted.
        assert_eq!(
            repoint_refusal(&reverse, "a", "owner", "b", None, "c"),
            Ok(())
        );
    }

    fn error_names(repo_ds: &Value) -> String {
        repoint_refusal(repo_ds, "a", "owner", "b", Some("owned"), "c").unwrap_err()
    }

    #[test]
    fn a_rename_is_refused_for_an_object_a_mapping_names_either_way() {
        let both = repo(Some(reference()), Some(reverse_reference()));
        // `a` holds a mapping, and `b.owned` points back at managed/a.
        let error = rename_refusal(&both, "a").unwrap_err();
        assert!(
            error.contains("a.owner") && error.contains("b.owned"),
            "{error}"
        );
        // Only an inbound mapping: `a` holds none of its own.
        let error = rename_refusal(&repo(None, Some(reverse_reference())), "a").unwrap_err();
        assert!(error.contains("b.owned"), "{error}");
        assert_eq!(rename_refusal(&repo(None, None), "a"), Ok(()));
        // A mapping elsewhere that names another object does not block.
        assert_eq!(
            rename_refusal(&repo(None, Some(reverse_reference())), "c"),
            Ok(())
        );
    }

    #[test]
    fn a_new_end_over_a_mapping_to_another_target_is_refused() {
        let forward = repo(Some(reference()), None);
        // Same target as the leftover mapping: reused, allowed.
        assert_eq!(leftover_refusal(&forward, &[("a", "owner", "b")]), Ok(()));
        let error = leftover_refusal(&forward, &[("a", "owner", "c")]).unwrap_err();
        assert!(
            error.contains("a.owner (mapped to managed/b, now to managed/c)"),
            "{error}"
        );
        assert_eq!(
            leftover_refusal(&repo(None, None), &[("a", "owner", "c")]),
            Ok(())
        );
    }

    #[test]
    fn a_delete_names_the_mappings_it_leaves() {
        let forward = repo(Some(reference()), None);
        assert_eq!(
            delete_leftovers(&forward, &[("a", "owner"), ("b", "owned")]),
            vec!["a.owner".to_string()]
        );
        assert!(delete_leftovers(&repo(None, None), &[("a", "owner")]).is_empty());
    }

    #[test]
    fn links_read_from_one_object_or_an_array() {
        let one = json!({"_id": "a1", "owner": {"_refResourceCollection": "managed/b",
            "_refResourceId": "b1", "_refProperties": {"_id": "e1"}}});
        let many = json!({"_id": "b1", "owned": [{"_refResourceCollection": "managed/a",
            "_refResourceId": "a1", "_refProperties": {"_id": "e1"}}]});
        assert_eq!(links_of(&one, "owner")[0].to_id, "b1");
        assert_eq!(links_of(&many, "owned")[0].edge_id.as_deref(), Some("e1"));
        assert!(links_of(&json!({"_id": "a2", "owner": null}), "owner").is_empty());
    }

    #[test]
    fn a_link_resolves_only_with_the_same_edge_id() {
        let link = |from: &str, to: &str, edge: &str| Link {
            from_id: from.into(),
            to_collection: "managed/x".into(),
            to_id: to.into(),
            edge_id: Some(edge.into()),
        };
        let near = link("a1", "b1", "e1");
        assert_eq!(
            judge(&near, Some(&[link("b1", "a1", "e1")])),
            LinkOutcome::Resolved
        );
        assert_eq!(
            judge(&near, Some(&[link("b1", "a1", "e2")])),
            LinkOutcome::EdgeDiffers {
                far_edges: vec!["e2".into()]
            }
        );
        assert_eq!(
            judge(&near, Some(&[link("b1", "a9", "e1")])),
            LinkOutcome::Missing
        );
        assert_eq!(judge(&near, None), LinkOutcome::FarRecordMissing);
    }
}
