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
    /// A `repo.ds` relationship mapping on a property declared as something
    /// other than a relationship — a key reused after a delete.
    MappingOnScalar { object: String, key: String },
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
            Self::MappingOnScalar { object, key } => format!(
                "repo.ds maps {object}.{key} as a relationship, but config/managed declares it \
                 as something else"
            ),
        }
    }
}

/// A relationship pair, or a lone forward end, and what disagrees about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairReport {
    pub forward: End,
    pub reverse: Option<End>,
    pub problems: Vec<Problem>,
    /// Targets outside `config/managed` (`internal/role`, say), whose schema
    /// this check does not read: neither passed nor failed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unchecked: Vec<String>,
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

fn mapping_problems(end: &End, partner_key: Option<&str>, out: &mut Vec<Problem>) {
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
        if partner_key != Some(names.as_str()) {
            out.push(Problem::MappingPartner {
                end: label(end),
                names,
                expected: partner_key.unwrap_or("a declared reverse").to_string(),
            });
        }
    }
}

/// The reports for one relationship end: one per `managed/` target, each
/// with that target's reverse. A multi-target forward names one reverse key
/// that every managed target must hold, so each is checked — checking only
/// one let a second target missing its reverse pass (Astra round 4).
///
/// Problems that belong to the forward end alone — its own mapping — are
/// carried on the first report only, so they count once.
pub fn pairs(managed: &Value, repo_ds: &Value, object: &str, key: &str) -> Vec<PairReport> {
    let Some(forward) = end(managed, repo_ds, object, key) else {
        return Vec::new();
    };
    let Some(property) = properties(managed, object).and_then(|props| props.get(key)) else {
        return Vec::new();
    };
    let reverse_key = declared_reverse_key(property).map(str::to_string);
    let mut forward_problems = Vec::new();
    mapping_problems(&forward, reverse_key.as_deref(), &mut forward_problems);
    let (managed_targets, unchecked): (Vec<&String>, Vec<&String>) = forward
        .targets
        .iter()
        .partition(|path| path.starts_with("managed/"));
    let unchecked: Vec<String> = unchecked.into_iter().cloned().collect();
    let mut reports = Vec::new();
    if let Some(reverse_key) = &reverse_key {
        for path in &managed_targets {
            let target = &path["managed/".len()..];
            let mut problems = Vec::new();
            let reverse = reverse_end(managed, repo_ds, object, key, target, reverse_key);
            match &reverse {
                Err(problem) => problems.push(problem.clone()),
                Ok(reverse) => {
                    mapping_problems(reverse, Some(key), &mut problems);
                    match (&forward.mapping, &reverse.mapping) {
                        (Some(_), None) => problems.push(Problem::Mixed {
                            mapped: label(&forward),
                            unmapped: label(reverse),
                        }),
                        (None, Some(_)) => problems.push(Problem::Mixed {
                            mapped: label(reverse),
                            unmapped: label(&forward),
                        }),
                        (Some(a), Some(b)) if a.kind == b.kind => {
                            problems.push(Problem::MappingKinds {
                                kind: a.kind.clone(),
                            })
                        }
                        _ => {}
                    }
                }
            }
            reports.push(PairReport {
                forward: forward.clone(),
                reverse: reverse.ok(),
                problems,
                unchecked: Vec::new(),
            });
        }
    }
    if reports.is_empty() {
        reports.push(PairReport {
            forward,
            reverse: None,
            problems: Vec::new(),
            unchecked: Vec::new(),
        });
    }
    let first = &mut reports[0];
    forward_problems.append(&mut first.problems);
    first.problems = forward_problems;
    first.unchecked = unchecked;
    reports
}

/// `target.reverse_key` as the reverse of `object.key`, or why it is not one.
fn reverse_end(
    managed: &Value,
    repo_ds: &Value,
    object: &str,
    key: &str,
    target: &str,
    reverse_key: &str,
) -> Result<End, Problem> {
    let missing = || Problem::ReverseMissing {
        object: target.to_string(),
        key: reverse_key.to_string(),
    };
    let property = properties(managed, target)
        .and_then(|props| props.get(reverse_key))
        .ok_or_else(missing)?;
    let back = format!("managed/{object}");
    match end(managed, repo_ds, target, reverse_key) {
        Some(reverse)
            if declared_reverse_key(property).is_none_or(|k| k == key)
                && reverse.targets.contains(&back) =>
        {
            Ok(reverse)
        }
        _ => Err(Problem::ReverseNotReciprocal {
            object: target.to_string(),
            key: reverse_key.to_string(),
        }),
    }
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
            // `_meta` and `_notifications` are IDM's own relationships, mapped
            // on every user object and never declared in `config/managed`
            // (sandbox, 2026-10-26): absent from the schema is their normal
            // state, not a leftover.
            if key.is_some_and(|key| key != mapped_key)
                || mapped_key.starts_with('_')
                || mapping_for(repo_ds, object, mapped_key).is_none()
            {
                continue;
            }
            match props.get(mapped_key) {
                None => stale.push(Problem::StaleMapping {
                    object: object.to_string(),
                    key: mapped_key.clone(),
                }),
                Some(property) if !crate::managed::state::is_relationship_property(property) => {
                    stale.push(Problem::MappingOnScalar {
                        object: object.to_string(),
                        key: mapped_key.clone(),
                    })
                }
                Some(_) => {}
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
                if stale.is_empty() {
                    return Err(format!("'{object}.{key}' is not a relationship"));
                }
                return Ok(Report {
                    pairs: Vec::new(),
                    stale,
                });
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
        for pair in self::pairs(managed, repo_ds, object, key) {
            let mut identity = vec![label(&pair.forward)];
            if let Some(reverse) = &pair.reverse {
                identity.push(label(reverse));
            }
            identity.sort();
            if seen.insert(identity) {
                pairs.push(pair);
            }
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

/// The facts about a property that its `repo.ds` mapping depends on.
#[derive(Debug, PartialEq, Eq)]
struct Shape {
    relationship: bool,
    targets: Vec<String>,
    reverse: Option<String>,
    array: bool,
}

fn shapes(doc: &Value) -> Vec<((String, String), Shape)> {
    let mut out = Vec::new();
    let Ok(objects) = crate::managed::api::objects(doc) else {
        return out;
    };
    for object in objects {
        let Some(name) = object.get("name").and_then(Value::as_str) else {
            continue;
        };
        for (key, property) in crate::managed::state::properties(object)
            .into_iter()
            .flatten()
        {
            let relationship = crate::managed::state::is_relationship_property(property);
            let node = property.get("items").unwrap_or(property);
            out.push((
                (name.to_string(), key.clone()),
                Shape {
                    relationship,
                    targets: node
                        .get("resourceCollection")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|entry| entry.get("path").and_then(Value::as_str))
                        .map(str::to_string)
                        .collect(),
                    reverse: declared_reverse_key(property).map(str::to_string),
                    array: property.get("type").and_then(Value::as_str) == Some("array"),
                },
            ));
        }
    }
    out
}

/// The properties `after` adds, or changes in a way a mapping depends on.
fn changed(before: &Value, after: &Value) -> Vec<(String, String)> {
    let before = shapes(before);
    shapes(after)
        .into_iter()
        .filter(|(id, shape)| !before.iter().any(|(b, old)| b == id && old == shape))
        .map(|(id, _)| id)
        .collect()
}

/// Whether writing `after` over `before` needs `config/repo.ds` consulted:
/// some property was added, or changed its relationship shape. A write that
/// only removes, or edits nothing a mapping depends on (a hook body, a title),
/// does not — so those writes never depend on repo.ds being readable.
pub fn needs_repo_ds(before: &Value, after: &Value) -> bool {
    !changed(before, after).is_empty()
}

fn mapping_problem(problem: &Problem) -> bool {
    matches!(
        problem,
        Problem::Mixed { .. }
            | Problem::MappingKinds { .. }
            | Problem::MappingTarget { .. }
            | Problem::MappingPartner { .. }
    )
}

fn mapping_problems_in(doc: &Value, repo_ds: &Value) -> Vec<Problem> {
    let mut out: Vec<Problem> = Vec::new();
    for ((object, key), shape) in shapes(doc) {
        if !shape.relationship {
            continue;
        }
        for problem in pairs(doc, repo_ds, &object, &key)
            .into_iter()
            .flat_map(|pair| pair.problems)
            .filter(mapping_problem)
        {
            if !out.contains(&problem) {
                out.push(problem);
            }
        }
    }
    out
}

/// Refuses a `config/managed` write that would make the document contradict
/// `config/repo.ds` where it did not before. Run at the write boundary
/// (`api::replace_managed_confirmed`), so it covers every verb — undo, field
/// add, object rename — and not only the ones that remembered to ask (Astra
/// round 4). Contradictions already present are not this write's doing and
/// do not block it; `relationship verify` reports them.
///
/// Two classes: a relationship whose pair would disagree with its mappings
/// (one end mapped and the other not — the measured broken state —, a mapping
/// naming a target the end no longer declares, a `reverseReference` reading
/// through a key that is not its partner's, both ends holding the reference),
/// and a non-relationship property written at a key `repo.ds` maps as one.
pub fn transition_refusal(before: &Value, after: &Value, repo_ds: &Value) -> Result<(), String> {
    let mut reasons: Vec<String> = Vec::new();
    let after_shapes = shapes(after);
    for (object, key) in changed(before, after) {
        let relationship = after_shapes
            .iter()
            .any(|((o, k), shape)| *o == object && *k == key && shape.relationship);
        if !relationship && let Some(mapping) = mapped(repo_ds, &object, &key) {
            reasons.push(format!(
                "{object}.{key} would not be a relationship, but repo.ds maps it as a `{}`{}",
                mapping.kind,
                mapping
                    .resource_path
                    .map(|path| format!(" to {path}"))
                    .unwrap_or_default()
            ));
        }
    }
    let existing = mapping_problems_in(before, repo_ds);
    reasons.extend(
        mapping_problems_in(after, repo_ds)
            .iter()
            .filter(|problem| !existing.contains(problem))
            .map(Problem::message),
    );
    if reasons.is_empty() {
        return Ok(());
    }
    Err(format!(
        "refusing to write config/managed: it would contradict config/repo.ds, which aic does \
         not edit — {}. Use another key, or repair the mapping in the console \
         (docs/api/10-managed-objects.md, \"Relationship storage\")",
        reasons.join("; ")
    ))
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
/// came back as one object or an array. Absent or `null` is no links; any
/// other shape is an error, not an empty list — dropping what did not decode
/// would turn a response nothing understood into "untested" (Sol round 6).
pub fn links_of(record: &Value, key: &str) -> Result<Vec<Link>, String> {
    let from_id = record
        .get("_id")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("a record read for {key} has no string _id: {record}"))?;
    let entries: Vec<&Value> = match record.get(key) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.iter().collect(),
        Some(item @ Value::Object(_)) => vec![item],
        Some(other) => {
            return Err(format!(
                "{from_id}.{key} is not a relationship value: {other}"
            ));
        }
    };
    entries
        .into_iter()
        .map(|entry| {
            let field = |name: &str| {
                entry
                    .get(name)
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| format!("{from_id}.{key} has a link without {name}: {entry}"))
            };
            Ok(Link {
                from_id: from_id.to_string(),
                to_collection: field("_refResourceCollection")?,
                to_id: field("_refResourceId")?,
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
    /// The far end lists the near record, but under another edge id. Two
    /// stores each holding a link is one cause; a link removed and re-added
    /// between the two reads is another, so a re-run is worth one look.
    EdgeDiffers { far_edges: Vec<String> },
    /// The far end does not list the near record.
    Missing,
    /// The far record does not exist.
    FarRecordMissing,
}

/// Judges one sampled link, read from a record of `near_collection`, against
/// what the far end's reverse property returned (`None` when the far record
/// is gone). A link back is one naming the near record by collection **and**
/// id: an edge id agreeing is corroboration, not identity, since nothing
/// measured makes edge ids unique across collections.
pub fn judge(link: &Link, near_collection: &str, far_entries: Option<&[Link]>) -> LinkOutcome {
    let Some(far_entries) = far_entries else {
        return LinkOutcome::FarRecordMissing;
    };
    let back: Vec<&Link> = far_entries
        .iter()
        .filter(|far| far.to_id == link.from_id && far.to_collection == near_collection)
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
        // IDM's system relationships are mapped and never declared.
        repo_ds["resourceMapping"]["genericMapping"]["managed/a"]["properties"]["_meta"] =
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

    /// Sol round 6: a scalar reusing a mapped key is reported, by object and
    /// by key; and only relationship mappings count — a plain attribute
    /// mapping on a scalar is ordinary.
    #[test]
    fn a_relationship_mapping_on_a_scalar_is_reported() {
        let mut doc = managed();
        doc["objects"][0]["schema"]["properties"]["owner"] = json!({"type": "string"});
        let mut repo_ds = repo(Some(reference()), None);
        repo_ds["resourceMapping"]["genericMapping"]["managed/a"]["properties"]["plain"] =
            json!({"type": "string", "ldapAttribute": "fr-attr-istr1"});
        let expected = vec![Problem::MappingOnScalar {
            object: "a".into(),
            key: "owner".into(),
        }];
        assert_eq!(report(&doc, &repo_ds, "a", None).unwrap().stale, expected);
        assert_eq!(
            report(&doc, &repo_ds, "a", Some("owner")).unwrap().stale,
            expected
        );
        assert!(report(&doc, &repo_ds, "a", Some("plain")).is_err());
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
    fn a_delete_names_the_mappings_it_leaves() {
        let forward = repo(Some(reference()), None);
        assert_eq!(
            delete_leftovers(&forward, &[("a", "owner"), ("b", "owned")]),
            vec!["a.owner".to_string()]
        );
        assert!(delete_leftovers(&repo(None, None), &[("a", "owner")]).is_empty());
    }

    /// Astra round 4, finding 1: leftovers describe `a.old <-> b.owned`, and
    /// the pair is recreated as `a.owner <-> b.owned`. Every resourcePath still
    /// matches, so a target-only check passes; `b.owned` still reads through
    /// `old`. The control is the same recreation over leftovers that agree.
    #[test]
    fn recreating_a_pair_over_a_mapping_naming_the_old_partner_is_refused() {
        let before = json!({"_id": "managed", "objects": [
            {"name": "a", "schema": {"properties": {}}},
            {"name": "b", "schema": {"properties": {}}}
        ]});
        let mut stale = reverse_reference();
        stale["propertyName"] = json!("old");
        let error = transition_refusal(&before, &managed(), &repo(Some(reference()), Some(stale)))
            .unwrap_err();
        assert!(
            error.contains("reads through old, expected owner"),
            "{error}"
        );
        assert_eq!(
            transition_refusal(
                &before,
                &managed(),
                &repo(Some(reference()), Some(reverse_reference()))
            ),
            Ok(())
        );
    }

    /// The measured broken state, reached through any verb: a recreated pair
    /// where only one end picks up a leftover mapping.
    #[test]
    fn a_write_that_leaves_one_end_mapped_is_refused() {
        let before = json!({"_id": "managed", "objects": [
            {"name": "a", "schema": {"properties": {}}},
            {"name": "b", "schema": {"properties": {}}}
        ]});
        let error =
            transition_refusal(&before, &managed(), &repo(Some(reference()), None)).unwrap_err();
        assert!(error.contains("a.owner is mapped"), "{error}");
        assert_eq!(transition_refusal(&before, &managed(), &json!({})), Ok(()));
    }

    /// Astra round 4, finding 2: an undo (or any whole-document write) that
    /// retargets a mapped end back is caught at the boundary, and a scalar
    /// written over a leftover relationship mapping is too.
    #[test]
    fn a_retarget_or_a_scalar_over_a_mapping_is_refused_whatever_the_verb() {
        let mapped = repo(Some(reference()), Some(reverse_reference()));
        let mut retargeted = managed();
        retargeted["objects"][0]["schema"]["properties"]["owner"]["resourceCollection"] =
            json!([{"path": "managed/c"}]);
        let error = transition_refusal(&managed(), &retargeted, &mapped).unwrap_err();
        assert!(error.contains("points at managed/b"), "{error}");

        let mut scalar = managed();
        scalar["objects"][0]["schema"]["properties"]["owner"] = json!({"type": "string"});
        let error = transition_refusal(&managed(), &scalar, &mapped).unwrap_err();
        assert!(
            error.contains("a.owner would not be a relationship"),
            "{error}"
        );
        // Unmapped, the same scalar is fine.
        assert_eq!(transition_refusal(&managed(), &scalar, &json!({})), Ok(()));
    }

    /// Sol round 6: renaming `c` to `a` moves `c.owner -> managed/d` onto
    /// `a.owner`'s surviving mapping to managed/b. No mapping names `c`, so
    /// only a check of the resulting document sees it.
    #[test]
    fn a_rename_onto_a_name_with_contradicting_mappings_is_refused() {
        let before = json!({"_id": "managed", "objects": [
            {"name": "c", "schema": {"properties": {
                "owner": {"type": "relationship", "resourceCollection": [{"path": "managed/d"}]}
            }}},
            {"name": "d", "schema": {"properties": {}}}
        ]});
        let mut after = before.clone();
        after["objects"][0]["name"] = json!("a");
        let error =
            transition_refusal(&before, &after, &repo(Some(reference()), None)).unwrap_err();
        assert!(error.contains("points at managed/b"), "{error}");
        assert_eq!(transition_refusal(&before, &after, &json!({})), Ok(()));
    }

    /// What already disagrees is not this write's doing: an unrelated edit to
    /// a document with a pre-existing mixed pair still writes, and needs no
    /// repo.ds at all.
    #[test]
    fn existing_contradictions_and_unrelated_edits_do_not_block() {
        let mixed = repo(Some(reference()), None);
        let mut titled = managed();
        titled["objects"][0]["schema"]["properties"]["owner"]["title"] = json!("Owner");
        titled["objects"][0]["onCreate"] = json!({"type": "text/javascript", "source": "x"});
        assert!(!needs_repo_ds(&managed(), &titled));
        assert_eq!(transition_refusal(&managed(), &titled, &mixed), Ok(()));
        let mut added = managed();
        added["objects"][0]["schema"]["properties"]["note"] = json!({"type": "string"});
        assert!(needs_repo_ds(&managed(), &added));
        assert_eq!(transition_refusal(&managed(), &added, &mixed), Ok(()));
    }

    #[test]
    fn links_read_from_one_object_or_an_array() {
        let one = json!({"_id": "a1", "owner": {"_refResourceCollection": "managed/b",
            "_refResourceId": "b1", "_refProperties": {"_id": "e1"}}});
        let many = json!({"_id": "b1", "owned": [{"_refResourceCollection": "managed/a",
            "_refResourceId": "a1", "_refProperties": {"_id": "e1"}}]});
        assert_eq!(links_of(&one, "owner").unwrap()[0].to_id, "b1");
        assert_eq!(
            links_of(&many, "owned").unwrap()[0].edge_id.as_deref(),
            Some("e1")
        );
        assert!(
            links_of(&json!({"_id": "a2", "owner": null}), "owner")
                .unwrap()
                .is_empty()
        );
        assert!(links_of(&json!({"_id": "a3"}), "owner").unwrap().is_empty());
    }

    /// Sol round 6: what does not decode is an error, never an empty sample.
    #[test]
    fn a_link_that_does_not_decode_is_an_error() {
        let partial = json!({"_id": "a1", "owner": [{"_refResourceId": "b1"}]});
        assert!(
            links_of(&partial, "owner")
                .unwrap_err()
                .contains("without _refResourceCollection")
        );
        assert!(links_of(&json!({"_id": "a1", "owner": "b1"}), "owner").is_err());
        assert!(links_of(&json!({"owner": []}), "owner").is_err());
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
            judge(&near, "managed/x", Some(&[link("b1", "a1", "e1")])),
            LinkOutcome::Resolved
        );
        assert_eq!(
            judge(&near, "managed/x", Some(&[link("b1", "a1", "e2")])),
            LinkOutcome::EdgeDiffers {
                far_edges: vec!["e2".into()]
            }
        );
        assert_eq!(
            judge(&near, "managed/x", Some(&[link("b1", "a9", "e1")])),
            LinkOutcome::Missing
        );
        assert_eq!(
            judge(&near, "managed/x", None),
            LinkOutcome::FarRecordMissing
        );
    }

    /// Astra round 4: same record id, same edge id, but the far link names
    /// another collection — it does not point back at this record.
    #[test]
    fn a_link_back_to_another_collection_does_not_resolve() {
        let near = Link {
            from_id: "123".into(),
            to_collection: "managed/b".into(),
            to_id: "456".into(),
            edge_id: Some("e".into()),
        };
        let far = Link {
            from_id: "456".into(),
            to_collection: "managed/c".into(),
            to_id: "123".into(),
            edge_id: Some("e".into()),
        };
        assert_eq!(
            judge(&near, "managed/a", Some(std::slice::from_ref(&far))),
            LinkOutcome::Missing
        );
        let far = Link {
            to_collection: "managed/a".into(),
            ..far
        };
        assert_eq!(
            judge(&near, "managed/a", Some(&[far])),
            LinkOutcome::Resolved
        );
    }

    /// Astra round 4: every managed target is checked, and a target outside
    /// `config/managed` is neither a pass nor a failure.
    #[test]
    fn every_managed_target_is_checked_and_external_ones_are_unchecked() {
        let mut doc = managed();
        doc["objects"][0]["schema"]["properties"]["owner"]["resourceCollection"] =
            json!([{"path": "managed/b"}, {"path": "managed/c"}, {"path": "internal/role"}]);
        doc["objects"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "c", "schema": {"properties": {}}}));
        let checked = super::report(&doc, &json!({}), "a", Some("owner")).unwrap();
        assert_eq!(checked.pairs.len(), 2);
        assert_eq!(checked.pairs[0].problems, vec![]);
        assert_eq!(
            checked.pairs[0].unchecked,
            vec!["internal/role".to_string()]
        );
        assert_eq!(
            checked.pairs[1].problems,
            vec![Problem::ReverseMissing {
                object: "c".into(),
                key: "owned".into()
            }]
        );
        // A forward whose only target is external has nothing to fail.
        doc["objects"][0]["schema"]["properties"]["owner"]["resourceCollection"] =
            json!([{"path": "internal/role"}]);
        let checked = super::report(&doc, &json!({}), "a", Some("owner")).unwrap();
        assert_eq!(checked.pairs.len(), 1);
        assert_eq!(checked.pairs[0].problems, vec![]);
        assert_eq!(
            checked.pairs[0].unchecked,
            vec!["internal/role".to_string()]
        );
    }
}
