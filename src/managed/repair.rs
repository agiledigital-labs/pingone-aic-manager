//! The single `config/repo.ds` write workflow: stale-mapping removal for
//! `relationship repair` (and, with `--empty-entry`, removal of a deleted type's
//! empty per-type entry), atomic pair addition for `map` / new `set`, and
//! explicit in-place conversion rollback (both ends removed in one PUT), and
//! its preliminary unchanged-document write check before records are touched.
//! `conversion_records` plans full rollback eligibility before any unlink;
//! this writer independently replans against fresh documents at the final PUT.
//! Addition decisions and allocation live in `storage`; both use the same
//! recheck, backup, whole-document PUT and read-back machinery below.
//!
//! A relationship delete leaves its `repo.ds` mappings behind, and a leftover
//! makes the next `relationship set` that reuses the key, or reads through it,
//! contradict `repo.ds` — so `aic` refuses it, and before this verb there was
//! no way out short of hand-editing the document.
//!
//! What was measured, on the sandbox's test objects (2026-10-31,
//! `docs/api/10-managed-objects.md`, "Relationship storage"):
//!
//! - A JSON `PATCH` `remove` of one mapping is `200` and changes nothing: the
//!   response and every re-read still hold the entry. So this verb never
//!   patches.
//! - A whole-document `PUT` of the read document minus the entry is `200`,
//!   and re-reads equal the document sent. Mapped pairs that were left alone
//!   kept resolving, and new links on them still went to the mapped store.
//! - Removing a stale `reference`, a stale `reverseReference` whose forward
//!   was also stale, and a stale `reverseReference` whose forward is still
//!   declared and mapped each unblocked the refused `set` (where there was
//!   one) and broke nothing that was read afterwards.
//!
//! What was **not** measured, and is therefore refused: removing a mapping
//! that another mapping reads through, and anything on an object
//! `config/managed` does not declare. Removing an object's last mapped
//! property is allowed and keeps its per-type entry with `properties: {}`,
//! the measured state of a freshly added entry. That entry is removed only
//! by [`plan_remove_entry`] (`--empty-entry`), for a deleted or an existing type.
//!
//! Additions use the measured 2026-11-01 shape and waitForCompletion=true.
//! A WritePermit proves consent outside dry-run; callers also gate production.
//!
//! Nothing here performs I/O except through the closures [`apply_with`] is
//! handed, so a test drives the real order of reads and writes.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::managed::verify::{self, Mapping};

#[derive(Debug, Clone, PartialEq)]
enum Operation {
    Remove,
    Add(Box<super::storage::Addition>),
    Rollback(super::conversion::Pair),
    Confirm(super::conversion::Pair),
    /// Remove an empty per-type `genericMapping."managed/<object>"` entry for
    /// a type `config/managed` no longer declares.
    RemoveEntry,
}

/// A removal or atomic addition computed from one read of each document.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Plan {
    pub object: String,
    pub key: String,
    pub mapping: Mapping,
    /// What the operator should know before consenting; never a refusal.
    pub notes: Vec<String>,
    /// The `repo.ds` this plan was computed from. The write is refused if
    /// the tenant no longer holds exactly this.
    #[serde(skip)]
    before: Value,
    #[serde(skip)]
    operation: Operation,
    /// The whole transformed document that is `PUT`.
    #[serde(skip)]
    after: Value,
}

impl Plan {
    pub fn addition(&self) -> Option<&super::storage::Addition> {
        match &self.operation {
            Operation::Add(addition) => Some(addition),
            _ => None,
        }
    }

    pub fn before(&self) -> &Value {
        &self.before
    }

    pub fn after(&self) -> &Value {
        &self.after
    }

    pub fn label(&self) -> String {
        match self.operation {
            Operation::RemoveEntry => format!("managed/{}", self.object),
            _ => format!("{}.{}", self.object, self.key),
        }
    }
}

/// Proof that the operator consented to a non-dry-run mapping write. The field is
/// private, so [`authorize`] is the only place one is made, and
/// [`apply_with`] requires one: a dry run cannot reach the write.
pub struct WritePermit(());

pub fn authorize(dry_run: bool, consented: bool) -> Option<WritePermit> {
    (!dry_run && consented).then_some(WritePermit(()))
}

/// The plan for removing `object.key`'s mapping, or why it is refused.
pub fn plan(managed: &Value, repo_ds: &Value, object: &str, key: &str) -> Result<Plan, String> {
    let label = format!("{object}.{key}");
    if verify::SYSTEM_RELATIONSHIPS.contains(&key) {
        return Err(format!(
            "refusing to repair {label}: {key} is one of IDM's own relationships, mapped on \
             every user object and never declared in config/managed"
        ));
    }
    let Some(props) = verify::properties(managed, object) else {
        return Err(format!(
            "refusing to repair {label}: managed object '{object}' is not declared in \
             config/managed, and removing a mapping from an undeclared object is unmeasured"
        ));
    };
    let sections = verify::sections_mapping(repo_ds, object, key);
    match sections.as_slice() {
        [] => {
            return Err(format!(
                "config/repo.ds has no mapping for {label}; nothing to repair"
            ));
        }
        [only] if only == "genericMapping" => {}
        [_] => {
            return Err(format!(
                "refusing to repair {label}: its config/repo.ds mapping is outside \
                 genericMapping (in {}), and removing one there is unmeasured",
                sections[0]
            ));
        }
        many => {
            // Which copy IDM honours is unknown, and removing one would not
            // make the key unmapped (Sol/Astra round 9).
            return Err(format!(
                "refusing to repair {label}: config/repo.ds maps it in more than one section \
                 ({}), and which one IDM uses is unmeasured",
                many.join(", ")
            ));
        }
    }
    let Some(entries) = verify::generic_mappings_of(repo_ds, object) else {
        return Err(format!(
            "internal: managed/{object} vanished from genericMapping"
        ));
    };
    // Validate the exact entry the write removes — never a merged view of it.
    let Some(mapping) = verify::relationship_mapping(&entries[key]) else {
        let kind = entries[key]
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("untyped");
        return Err(format!(
            "refusing to repair {label}: its config/repo.ds mapping is `{kind}`, not a \
             relationship mapping; repair removes only `reference` and `reverseReference`"
        ));
    };
    if let Some(property) = props.get(key) {
        let what = if crate::managed::state::is_relationship_property(property) {
            "a relationship"
        } else {
            "something other than a relationship"
        };
        return Err(format!(
            "refusing to repair {label}: config/managed declares it, as {what}. Repair removes \
             only mappings that name no declared property"
        ));
    }
    let dependents = dependents(managed, repo_ds, object, key);
    if !dependents.is_empty() {
        let names: Vec<String> = dependents
            .iter()
            .map(|dependent| match dependent.owner {
                Owner::Declared => format!("{} (declared)", dependent.label),
                Owner::Stale => format!("{} (also stale: repair it first)", dependent.label),
                Owner::StaleBlocked(why) => format!(
                    "{} (also stale, but repair refuses it too: {why})",
                    dependent.label
                ),
                Owner::Outside => format!(
                    "{} (not a managed property; repair does not edit it)",
                    dependent.label
                ),
            })
            .collect();
        return Err(format!(
            "refusing to repair {label}: config/repo.ds reads through it from {}. Removing a \
             mapping another one reads through is unmeasured",
            names.join(", ")
        ));
    }
    let mut after = repo_ds.clone();
    let removed = after
        .pointer_mut(&format!(
            "/resourceMapping/genericMapping/{}/properties",
            pointer_token(&format!("managed/{object}"))
        ))
        .and_then(Value::as_object_mut)
        .and_then(|properties| properties.remove(key));
    if removed.is_none() {
        // `mappings_of` found it above, so this is the same lookup disagreeing
        // with itself — a defect here, not a tenant state.
        return Err(format!(
            "internal: could not locate {label}'s mapping to remove"
        ));
    }

    let mut notes = Vec::new();
    if entries.len() == 1 {
        // The entry stays, with `properties: {}`: the state a new per-type
        // entry starts in, measured to keep records readable and creatable
        // (docs/api/10). Removing the entry itself would move where the
        // object's records are looked up, which is not measured.
        notes.push(format!(
            "{label} is the last mapped property of managed/{object}; its per-type entry \
             stays, with no properties — the state a new per-type entry starts in"
        ));
    }
    if mapping.kind == "reverseReference"
        && let (Some(partner_object), Some(partner_key)) = (
            mapping
                .resource_path
                .as_deref()
                .and_then(|path| path.strip_prefix("managed/")),
            mapping.property_name.as_deref(),
        )
        && verify::properties(managed, partner_object)
            .and_then(|props| props.get(partner_key))
            .is_some_and(crate::managed::state::is_relationship_property)
        && verify::sections_mapping(repo_ds, partner_object, partner_key).len() == 1
        && verify::mapped(repo_ds, partner_object, partner_key)
            .is_some_and(|partner| partner.kind == "reference")
    {
        // Only the measured state: a declared relationship that is itself
        // mapped as the `reference` (round 7 — declaration alone says nothing
        // about storage).
        notes.push(format!(
            "{partner_object}.{partner_key} is a declared relationship mapped as a reference, \
             and keeps that mapping; when this was measured its links still resolved and new \
             links still wrote afterwards"
        ));
    }
    notes.push(format!(
        "a relationship later declared as {label} will be stored generically; links written \
         while it was mapped are not expected to reappear through it (inferred, not measured)"
    ));
    Ok(Plan {
        object: object.to_string(),
        key: key.to_string(),
        mapping,
        notes,
        operation: Operation::Remove,
        before: repo_ds.clone(),
        after,
    })
}

/// The plan for removing `object`'s empty per-type `repo.ds` entry, or why it
/// is refused.
///
/// Measured on the sandbox (2026-11-10, `docs/api/10-managed-objects.md`):
/// for a type that **still exists**, with records and links written while it
/// was mapped, removing its empty entry by a backed-up whole-document `PUT`
/// left every record readable by id and by query and new records creatable,
/// immediately and after ~65 s. For a type `config/managed` no longer declares
/// the same removal was measured on 2026-11-09. So both are allowed, but only
/// for the entry `aic` itself generates for the type
/// (`storage::generated_entry`); any other value or field is refused. Also
/// refused: an entry that still maps a property, and one another mapping names
/// as its `resourcePath` (unmeasured).
pub fn plan_remove_entry(managed: &Value, repo_ds: &Value, object: &str) -> Result<Plan, String> {
    let label = format!("managed/{object}");
    let key = format!("managed/{object}");
    let Some(entry) = repo_ds.pointer(&format!(
        "/resourceMapping/genericMapping/{}",
        pointer_token(&key)
    )) else {
        return Err(format!(
            "config/repo.ds has no genericMapping entry for {label}; nothing to remove"
        ));
    };
    if let Some(properties) = entry.get("properties").and_then(Value::as_object)
        && !properties.is_empty()
    {
        let names: Vec<&str> = properties.keys().map(String::as_str).collect();
        return Err(format!(
            "refusing to remove {label}'s per-type entry: it still maps {}. Remove those \
             mappings first (`relationship repair <object>.<key>`)",
            names.join(", ")
        ));
    }
    if entry.get("properties").is_some_and(|p| !p.is_object()) {
        return Err(format!(
            "refusing to remove {label}'s per-type entry: its `properties` is not an object"
        ));
    }
    let readers = readers_of(repo_ds, object);
    if !readers.is_empty() {
        return Err(format!(
            "refusing to remove {label}'s per-type entry: config/repo.ds maps {} to \
             managed/{object}, and removing the entry under a mapping that names it is unmeasured",
            readers.join(", ")
        ));
    }
    let mut after = repo_ds.clone();
    let removed = after
        .pointer_mut("/resourceMapping/genericMapping")
        .and_then(Value::as_object_mut)
        .and_then(|entries| entries.remove(&key));
    if removed.is_none() {
        return Err(format!("internal: could not locate {label} to remove"));
    }
    // Only the layout `aic` itself generates for this type is a measured one
    // (docs/api/10). Any other value decides where records are stored, so
    // removing it is refused rather than noted.
    let generated = super::storage::generated_entry(object);
    let mut differing: Vec<String> = Vec::new();
    if let (Some(actual), Some(expected)) = (entry.as_object(), generated.as_object()) {
        for (name, value) in expected {
            if name != "properties" && actual.get(name) != Some(value) {
                differing.push(name.clone());
            }
        }
        for name in actual.keys() {
            if !expected.contains_key(name) {
                differing.push(name.clone());
            }
        }
    } else {
        differing.push("(the entry is not an object)".into());
    }
    if !differing.is_empty() {
        return Err(format!(
            "refusing to remove {label}'s per-type entry: it is not the layout aic generates \
             (differs in {}), and removing a storage layout that was not measured is refused",
            differing.join(", ")
        ));
    }
    let mut notes = Vec::new();
    if crate::managed::api::object_named(managed, object).is_ok() {
        notes.push(format!(
            "{object} is still declared in config/managed; its records were measured to stay \
             readable and creatable after its empty entry was removed (docs/api/10)"
        ));
    }
    Ok(Plan {
        object: object.to_string(),
        key: String::new(),
        mapping: Mapping {
            kind: "per-type entry".into(),
            resource_path: Some(key),
            property_name: None,
        },
        notes,
        operation: Operation::RemoveEntry,
        before: repo_ds.clone(),
        after,
    })
}

/// `object.key` labels of every `repo.ds` mapping, in any section, whose
/// `resourcePath` is `managed/<object>`.
fn readers_of(repo_ds: &Value, object: &str) -> Vec<String> {
    let target = format!("managed/{object}");
    let mut found = Vec::new();
    let Some(sections) = repo_ds.get("resourceMapping").and_then(Value::as_object) else {
        return found;
    };
    for (section, entries) in sections {
        let Some(entries) = entries.as_object() else {
            continue;
        };
        for (resource, entry) in entries {
            let Some(properties) = entry.get("properties").and_then(Value::as_object) else {
                continue;
            };
            for (key, mapping) in properties {
                if mapping.get("resourcePath").and_then(Value::as_str) == Some(target.as_str()) {
                    found.push(format!("{resource}.{key} ({section})"));
                }
            }
        }
    }
    found
}

/// Plan an atomic addition; the writer treats this like a removal except for
/// its pure recheck. Link emptiness is checked by the caller before applying.
pub fn plan_add(managed: &Value, repo: &Value, object: &str, key: &str) -> Result<Plan, String> {
    let addition = super::storage::plan(managed, repo, object, key)?;
    let mapping = verify::mapped(&addition.after, object, key)
        .ok_or("internal: addition did not produce a reference")?;
    Ok(Plan {
        object: object.into(),
        key: key.into(),
        mapping,
        notes: Vec::new(),
        before: repo.clone(),
        after: addition.after.clone(),
        operation: Operation::Add(Box::new(addition)),
    })
}

/// Explicit rollback removes both captured mapping entries atomically. It
/// keeps per-type metadata: removing that entry could hide newer records.
/// The caller must first prove the mapped link store empty.
pub fn plan_rollback(
    managed: &Value,
    repo: &Value,
    pair: &super::conversion::Pair,
) -> Result<Plan, String> {
    super::conversion::guard(pair, managed, repo, super::storage::Mode::Mapped)
        .map_err(super::conversion::config_detail)?;
    let mut after = repo.clone();
    for entry in &pair.mappings {
        // Another mapping reading the forward must not be stranded. The
        // pair's own reverse is removed in this same document.
        for dependent in verify::all_relationship_mappings(repo) {
            if dependent.mapping.kind == "reverseReference"
                && dependent.mapping.resource_path.as_deref()
                    == Some(format!("managed/{}", entry.object).as_str())
                && dependent.mapping.property_name.as_deref() == Some(entry.key.as_str())
                && !pair.mappings.iter().any(|own| {
                    dependent.resource == format!("managed/{}", own.object)
                        && dependent.section == own.section
                        && dependent.key == own.key
                })
            {
                return Err(format!(
                    "rollback refused: {} also reads through {}.{}",
                    dependent.label(),
                    entry.object,
                    entry.key
                ));
            }
        }
        after["resourceMapping"][&entry.section][format!("managed/{}", entry.object)]["properties"]
            .as_object_mut()
            .ok_or("mapping properties vanished")?
            .remove(&entry.key);
    }
    let mapping = verify::mapped(repo, &pair.forward.object, &pair.forward.key)
        .ok_or("reference vanished")?;
    Ok(Plan {
        object: pair.forward.object.clone(),
        key: pair.forward.key.clone(),
        mapping,
        notes: Vec::new(),
        before: repo.clone(),
        after,
        operation: Operation::Rollback(pair.clone()),
    })
}

/// Submit the unchanged document before rollback removes records. A write
/// denial stops the operation before any unlink; this does not lock access
/// against changes during the later record work.
pub fn plan_confirm(
    managed: &Value,
    repo: &Value,
    pair: &super::conversion::Pair,
) -> Result<Plan, String> {
    super::conversion::guard(pair, managed, repo, super::storage::Mode::Mapped)
        .map_err(super::conversion::config_detail)?;
    let mapping = verify::mapped(repo, &pair.forward.object, &pair.forward.key)
        .ok_or("reference vanished")?;
    Ok(Plan {
        object: pair.forward.object.clone(),
        key: pair.forward.key.clone(),
        mapping,
        notes: Vec::new(),
        before: repo.clone(),
        after: repo.clone(),
        operation: Operation::Confirm(pair.clone()),
    })
}

/// A `reverseReference` elsewhere in `repo.ds` that reads through
/// `object.key` — in any section and on any resource, because the sandbox's
/// own `internal/role` and `internal/notification` entries read through
/// `managed/alpha_user` (Sol/Astra round 7).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Dependent {
    label: String,
    owner: Owner,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Owner {
    /// A property `config/managed` declares on a managed object.
    Declared,
    /// A declared object's mapping naming no declared property, in
    /// `genericMapping`, beside other mappings: what repair can remove.
    Stale,
    /// Stale, but a repair of it would itself be refused for the reason
    /// carried — so "repair it first" would be a dead end (Astra round 8).
    StaleBlocked(&'static str),
    /// Outside `genericMapping."managed/…"`, which this verb does not edit.
    Outside,
}

fn dependents(managed: &Value, repo_ds: &Value, object: &str, key: &str) -> Vec<Dependent> {
    let path = format!("managed/{object}");
    verify::all_relationship_mappings(repo_ds)
        .into_iter()
        .filter(|located| {
            !(located.generic_object() == Some(object) && located.key == key)
                && located.mapping.kind == "reverseReference"
                && located.mapping.resource_path.as_deref() == Some(path.as_str())
                && located.mapping.property_name.as_deref() == Some(key)
        })
        .map(|located| Dependent {
            label: located.label(),
            owner: match located.generic_object() {
                None => Owner::Outside,
                Some(other) => match verify::properties(managed, other) {
                    None => Owner::StaleBlocked("its object is not declared"),
                    Some(props) if props.contains_key(&located.key) => Owner::Declared,
                    Some(_) if verify::sections_mapping(repo_ds, other, &located.key).len() > 1 => {
                        Owner::StaleBlocked("it is mapped in more than one section")
                    }
                    Some(_) if verify::SYSTEM_RELATIONSHIPS.contains(&located.key.as_str()) => {
                        Owner::StaleBlocked("it is one of IDM's own relationships")
                    }
                    Some(_) => Owner::Stale,
                },
            },
        })
        .collect()
}

/// RFC 6901 escaping for one pointer token.
fn pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// Plans removing every mapping in `ends` from `repo_ds`, against a
/// `config/managed` that no longer declares them (`managed` is the document
/// *after* the delete), in an order where each removal is one [`plan`] allows
/// on its own: a `reverseReference` that reads through a `reference` comes out
/// first. Each plan's `before` is the previous plan's `after`, so they apply
/// in sequence.
///
/// All or nothing: when no remaining end can be removed next, the reasons are
/// returned and nothing should be written — a delete that leaves some of its
/// mappings behind is the state this exists to avoid.
pub fn plan_sequence(
    managed: &Value,
    repo_ds: &Value,
    ends: &[(String, String)],
) -> Result<Vec<Plan>, Vec<String>> {
    let mut remaining: Vec<&(String, String)> = ends.iter().collect();
    let mut current = repo_ds.clone();
    let mut plans = Vec::new();
    while !remaining.is_empty() {
        let mut refusals = Vec::new();
        let next = remaining
            .iter()
            .enumerate()
            .find_map(
                |(index, (object, key))| match plan(managed, &current, object, key) {
                    Ok(plan) => Some((index, plan)),
                    Err(reason) => {
                        refusals.push(reason);
                        None
                    }
                },
            );
        let Some((index, plan)) = next else {
            return Err(refusals);
        };
        remaining.remove(index);
        current = plan.after.clone();
        plans.push(plan);
    }
    Ok(plans)
}

/// The pre-write check against fresh reads of both documents: `repo.ds` must
/// be exactly what the plan was computed from, and the plan must still hold
/// against the fresh `config/managed` (a property declared since would make
/// the mapping live again).
pub fn recheck(plan: &Plan, managed: &Value, repo_ds: &Value) -> Result<(), String> {
    if repo_ds != &plan.before {
        return Err(format!(
            "config/repo.ds changed since it was read for {}; nothing was written. Run {} \
             again to plan against the current document",
            plan.label(),
            match plan.operation {
                Operation::Rollback(_) | Operation::Confirm(_) => "rollback-map",
                Operation::Add(_) => "map",
                Operation::Remove | Operation::RemoveEntry => "repair",
            }
        ));
    }
    let fresh = match &plan.operation {
        Operation::Rollback(pair) => plan_rollback(managed, repo_ds, pair)?,
        Operation::Confirm(pair) => plan_confirm(managed, repo_ds, pair)?,
        Operation::Add(_) => plan_add(managed, repo_ds, &plan.object, &plan.key)?,
        Operation::Remove => self::plan(managed, repo_ds, &plan.object, &plan.key)?,
        Operation::RemoveEntry => plan_remove_entry(managed, repo_ds, &plan.object)?,
    };
    if fresh.after != plan.after {
        return Err(
            "relationship schema changed since planning; nothing written. Plan again".into(),
        );
    }
    Ok(())
}

/// How the read-back left things.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Readback {
    /// A read returned exactly the document sent.
    Confirmed,
    /// Every read succeeded and returned the pre-write document: the write
    /// did not take effect, at least not yet.
    Unchanged,
    /// Some read was neither, and none confirmed: someone else wrote, or the
    /// tenant normalised what was sent. Kept even if later reads returned the
    /// old document, because that read is evidence of another writer.
    Different,
    /// No read confirmed or differed, and at least one failed — so "every
    /// read was unchanged" cannot be claimed. Carries the last failure.
    Unread { error: String },
}

/// Classify one read-back.
pub fn judge(plan: &Plan, read: &Value) -> Readback {
    if read == &plan.after {
        Readback::Confirmed
    } else if read == &plan.before {
        Readback::Unchanged
    } else {
        Readback::Different
    }
}

/// Where a repair stopped, and whether anything reached the tenant.
#[derive(Debug)]
pub enum Failure {
    /// Refused or failed before the `PUT` was sent: nothing was written.
    NotWritten(String),
    /// The `PUT` failed. Whether it landed is unknown — a lost response looks
    /// the same as a refusal from here.
    SendFailed { error: String, backup: PathBuf },
}

#[derive(Debug, Serialize)]
pub struct Applied {
    pub backup: PathBuf,
    pub readback: Readback,
}

/// Re-read, recheck, back up, write, and confirm by re-read — in that order,
/// which is what a test pins.
///
/// `delays` are the pauses before each read-back after the first; the read
/// stops at the first one that confirms. Config writes were measured to read
/// back at once on this document, but the poll costs nothing when they do.
#[allow(clippy::too_many_arguments)]
pub async fn apply_with(
    plan: &Plan,
    _permit: &WritePermit,
    mut read_managed: impl AsyncFnMut() -> crate::Result<Value>,
    mut read_repo_ds: impl AsyncFnMut() -> crate::Result<Value>,
    backup: impl FnOnce(&Value) -> crate::Result<PathBuf>,
    mut send: impl AsyncFnMut(&Value) -> crate::Result<()>,
    delays: &[Duration],
) -> Result<Applied, Failure> {
    let not_written =
        |error: crate::Error| Failure::NotWritten(super::conversion::config_detail(error));
    let managed = read_managed().await.map_err(not_written)?;
    let repo_ds = read_repo_ds().await.map_err(not_written)?;
    recheck(plan, &managed, &repo_ds).map_err(Failure::NotWritten)?;
    let backup = backup(&repo_ds).map_err(not_written)?;

    if let Err(error) = send(&plan.after).await {
        return Err(Failure::SendFailed {
            error: error.to_string(),
            backup,
        });
    }

    // The outcome is over every read, not the last one (round 7): a read
    // showing a third document must not be forgotten when a later read
    // returns the old one, and one failed read means "every read was
    // unchanged" was not observed.
    let mut differed = false;
    let mut failed: Option<String> = None;
    for pause in std::iter::once(None).chain(delays.iter().map(Some)) {
        if let Some(pause) = pause {
            tokio::time::sleep(*pause).await;
        }
        match read_repo_ds().await.map(|read| judge(plan, &read)) {
            Ok(Readback::Confirmed) => {
                return Ok(Applied {
                    backup,
                    readback: Readback::Confirmed,
                });
            }
            Ok(Readback::Different) => differed = true,
            Ok(_) => {}
            Err(error) => failed = Some(error.to_string()),
        }
    }
    let readback = match (differed, failed) {
        (true, _) => Readback::Different,
        (false, Some(error)) => Readback::Unread { error },
        (false, None) => Readback::Unchanged,
    };
    Ok(Applied { backup, readback })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    fn managed() -> Value {
        json!({"objects": [
            {"name": "a", "schema": {"properties": {
                "name": {"type": "string"},
                "live": {"type": "relationship", "reverseRelationship": true,
                         "reversePropertyName": "back",
                         "resourceCollection": [{"path": "managed/b"}]},
                "scalar": {"type": "string"},
            }}},
            {"name": "b", "schema": {"properties": {
                "back": {"type": "relationship", "reverseRelationship": true,
                         "reversePropertyName": "live",
                         "resourceCollection": [{"path": "managed/a"}]},
            }}},
        ]})
    }

    fn reference(object: &str) -> Value {
        json!({"type": "reference", "resourcePath": format!("managed/{object}")})
    }

    fn reverse(object: &str, through: &str) -> Value {
        json!({"type": "reverseReference", "resourcePath": format!("managed/{object}"),
               "propertyName": through})
    }

    /// `a.live` <-> `b.back` mapped and healthy, plus whatever `a_extra` and
    /// `b_extra` add.
    fn repo(a_extra: Value, b_extra: Value) -> Value {
        let mut a = json!({"live": reference("b")});
        let mut b = json!({"back": reverse("a", "live")});
        a.as_object_mut()
            .unwrap()
            .extend(a_extra.as_object().unwrap().clone());
        b.as_object_mut()
            .unwrap()
            .extend(b_extra.as_object().unwrap().clone());
        json!({"dbType": "DS", "resourceMapping": {"genericMapping": {
            "managed/a": {"objectClasses": ["x"], "properties": a},
            "managed/b": {"properties": b},
        }}})
    }

    /// `a.live` <-> `b.back` after both ends were deleted from
    /// `config/managed`: the same pair's mappings, now naming nothing.
    fn managed_without_the_pair() -> Value {
        json!({"objects": [
            {"name": "a", "schema": {"properties": {
                "name": {"type": "string"},
                "kept": {"type": "relationship",
                         "resourceCollection": [{"path": "managed/b"}]},
            }}},
            {"name": "b", "schema": {"properties": {"x": {"type": "string"}}}},
        ]})
    }

    /// The order is the point: `b.back` reads through `a.live`, so `a.live`
    /// alone is refused and must come out second, whichever order the ends
    /// were named in. Unrelated mappings survive.
    #[test]
    fn a_sequence_removes_the_reader_first_and_leaves_the_rest() {
        let before = repo(
            json!({"kept": reference("b")}),
            json!({"other": reverse("a", "kept")}),
        );
        let ends = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(o, k)| (o.to_string(), k.to_string()))
                .collect()
        };
        assert!(plan(&managed_without_the_pair(), &before, "a", "live").is_err());
        for order in [
            [("a", "live"), ("b", "back")],
            [("b", "back"), ("a", "live")],
        ] {
            let plans = plan_sequence(&managed_without_the_pair(), &before, &ends(&order)).unwrap();
            let labels: Vec<_> = plans.iter().map(Plan::label).collect();
            assert_eq!(labels, ["b.back", "a.live"]);
            assert_eq!(plans[0].before(), &before);
            assert_eq!(plans[1].before(), plans[0].after());
            let expected = json!({"dbType": "DS", "resourceMapping": {"genericMapping": {
                "managed/a": {"objectClasses": ["x"], "properties": {"kept": reference("b")}},
                "managed/b": {"properties": {"other": reverse("a", "kept")}},
            }}});
            assert_eq!(plans[1].after(), &expected);
        }
    }

    /// The client case: each end is the only mapping on its type. Both come
    /// out, reader first, and both per-type entries stay with no properties.
    #[test]
    fn a_sequence_empties_a_pair_whose_ends_are_each_types_last_mapping() {
        let before = repo(json!({}), json!({}));
        let plans = plan_sequence(
            &managed_without_the_pair(),
            &before,
            &[("a".into(), "live".into()), ("b".into(), "back".into())],
        )
        .unwrap();
        let labels: Vec<_> = plans.iter().map(Plan::label).collect();
        assert_eq!(labels, ["b.back", "a.live"]);
        let expected = json!({"dbType": "DS", "resourceMapping": {"genericMapping": {
            "managed/a": {"objectClasses": ["x"], "properties": {}},
            "managed/b": {"properties": {}},
        }}});
        assert_eq!(plans[1].after(), &expected);
    }

    /// All or nothing: if one end cannot come out, no plan is returned — not
    /// even for the end that could.
    #[test]
    fn a_sequence_with_a_blocked_end_plans_nothing() {
        // `b.other` is declared and reads through `a.live`, so `a.live` is
        // refused; `b.back` on its own would be removable.
        let mut managed = managed_without_the_pair();
        managed["objects"][1]["schema"]["properties"]["other"] = json!({
            "type": "relationship", "reverseRelationship": true,
            "reversePropertyName": "live", "resourceCollection": [{"path": "managed/a"}]});
        let before = repo(json!({}), json!({"other": reverse("a", "live")}));
        assert!(plan(&managed, &before, "b", "back").is_ok());
        let error = plan_sequence(
            &managed,
            &before,
            &[("a".into(), "live".into()), ("b".into(), "back".into())],
        )
        .unwrap_err();
        assert!(
            error.iter().any(|reason| reason.contains("a.live")),
            "{error:?}"
        );
    }

    #[test]
    fn a_stale_reference_is_removed_and_nothing_else_changes() {
        let before = repo(json!({"gone": reference("b")}), json!({}));
        let plan = plan(&managed(), &before, "a", "gone").unwrap();
        assert_eq!(plan.mapping.kind, "reference");
        // The discriminating comparison: the whole document, so dropping a
        // sibling, the object entry or a top-level key fails it.
        assert_eq!(plan.after(), &repo(json!({}), json!({})));
        assert_eq!(plan.before(), &before);
    }

    #[test]
    fn a_stale_reverse_whose_forward_is_declared_says_the_forward_is_kept() {
        let before = repo(json!({}), json!({"gone": reverse("a", "live")}));
        let plan = plan(&managed(), &before, "b", "gone").unwrap();
        assert_eq!(plan.after(), &repo(json!({}), json!({})));
        assert!(
            plan.notes
                .iter()
                .any(|note| note
                    .starts_with("a.live is a declared relationship mapped as a reference")),
            "{:?}",
            plan.notes
        );

        let both_stale = repo(
            json!({"old": reference("b")}),
            json!({"gone": reverse("a", "old")}),
        );
        let plan = self::plan(&managed(), &both_stale, "b", "gone").unwrap();
        assert!(
            !plan
                .notes
                .iter()
                .any(|note| note.contains("keeps that mapping")),
            "{:?}",
            plan.notes
        );
    }

    #[test]
    fn a_declared_property_is_refused_whatever_it_is_declared_as() {
        let before = repo(json!({"scalar": reference("b")}), json!({}));
        let error = plan(&managed(), &before, "a", "live").unwrap_err();
        assert!(error.contains("declares it, as a relationship"), "{error}");
        let error = plan(&managed(), &before, "a", "scalar").unwrap_err();
        assert!(
            error.contains("as something other than a relationship"),
            "{error}"
        );
    }

    #[test]
    fn a_mapping_another_reads_through_is_refused_and_named() {
        // Declared dependent: removing `a.old` would cut `b.back2` off.
        let managed = {
            let mut doc = managed();
            doc["objects"][1]["schema"]["properties"]["back2"] =
                json!({"type": "relationship", "resourceCollection": [{"path": "managed/a"}]});
            doc
        };
        let before = repo(
            json!({"old": reference("b")}),
            json!({"back2": reverse("a", "old")}),
        );
        let error = plan(&managed, &before, "a", "old").unwrap_err();
        assert!(error.contains("b.back2 (declared)"), "{error}");

        // Stale dependent: named, with the order to repair in.
        let before = repo(
            json!({"old": reference("b")}),
            json!({"gone": reverse("a", "old")}),
        );
        let error = plan(&self::managed(), &before, "a", "old").unwrap_err();
        assert!(
            error.contains("b.gone (also stale: repair it first)"),
            "{error}"
        );

        // Discriminating controls: a reverse naming the same key on another
        // object, or another key on this object, does not read through it.
        let before = repo(
            json!({"old": reference("b")}),
            json!({"x": reverse("b", "old"), "y": reverse("a", "live")}),
        );
        assert!(plan(&self::managed(), &before, "a", "old").is_ok());
    }

    #[test]
    fn a_stale_dependent_repair_would_refuse_is_not_offered_as_the_way_out() {
        // Its object is undeclared.
        let mut before = repo(json!({"old": reference("b")}), json!({}));
        before["resourceMapping"]["genericMapping"]["managed/gone_obj"] =
            json!({"properties": {"owner": reverse("a", "old"), "x": reference("b")}});
        let error = plan(&managed(), &before, "a", "old").unwrap_err();
        assert!(
            error.contains("gone_obj.owner (also stale, but repair refuses it too: its object"),
            "{error}"
        );
        // It is mapped in more than one section, which repair refuses
        // (Sol round 10).
        let mut before = repo(
            json!({"old": reference("b")}),
            json!({"gone": reverse("a", "old")}),
        );
        before["resourceMapping"]["explicitMapping"] =
            json!({"managed/b": {"properties": {"gone": {"type": "string"}}}});
        let error = plan(&managed(), &before, "a", "old").unwrap_err();
        assert!(error.contains("b.gone (also stale, but repair refuses it too: it is mapped in more than one section)"), "{error}");
        // Its object's last mapping is an ordinary stale dependent now:
        // repair it first, which plan_sequence does.
        let mut before = repo(json!({"old": reference("b")}), json!({}));
        before["resourceMapping"]["genericMapping"]["managed/b"]["properties"] =
            json!({"gone": reverse("a", "old")});
        let error = plan(&managed(), &before, "a", "old").unwrap_err();
        assert!(
            error.contains("b.gone (also stale: repair it first)"),
            "{error}"
        );
    }

    #[test]
    fn an_explicit_mapping_is_found_but_not_edited() {
        let mut before = repo(json!({}), json!({}));
        before["resourceMapping"]["explicitMapping"] =
            json!({"managed/a": {"properties": {"gone": reference("b"), "y": reference("b")}}});
        let error = plan(&managed(), &before, "a", "gone").unwrap_err();
        assert!(error.contains("outside genericMapping"), "{error}");
        let error = plan(&managed(), &before, "a", "absent").unwrap_err();
        assert!(error.contains("no mapping for a.absent"), "{error}");
    }

    #[test]
    fn a_key_mapped_in_two_sections_is_refused_whichever_copy_is_a_relationship() {
        // Sol/Astra round 9: a merged view validated the explicit reference
        // and the write removed the generic scalar.
        for (generic, explicit) in [
            (
                json!({"type": "string", "ldapAttribute": "x"}),
                reference("b"),
            ),
            (
                reference("b"),
                json!({"type": "string", "ldapAttribute": "x"}),
            ),
            (reference("b"), reference("b")),
        ] {
            let mut before = repo(json!({"gone": generic}), json!({}));
            before["resourceMapping"]["explicitMapping"] =
                json!({"managed/a": {"properties": {"gone": explicit}}});
            let error = plan(&managed(), &before, "a", "gone").unwrap_err();
            assert!(error.contains("more than one section"), "{error}");
        }
    }

    #[test]
    fn the_unmeasured_and_the_absent_are_refused() {
        let before = repo(json!({"gone": reference("b")}), json!({}));
        for (object, key, expect) in [
            ("a", "_meta", "IDM's own"),
            ("a", "nothing", "no mapping for a.nothing"),
            ("c", "gone", "'c' is not declared"),
        ] {
            let error = plan(&managed(), &before, object, key).unwrap_err();
            assert!(error.contains(expect), "{object}.{key}: {error}");
        }

        let column = repo(
            json!({"gone": {"type": "string", "ldapAttribute": "x"}}),
            json!({}),
        );
        let error = plan(&managed(), &column, "a", "gone").unwrap_err();
        assert!(error.contains("is `string`"), "{error}");

        // Last mapping on the object: `b` holds only the stale one. Allowed;
        // the per-type entry stays, emptied, with its other members intact.
        let mut last = repo(json!({}), json!({}));
        last["resourceMapping"]["genericMapping"]["managed/b"]["properties"] =
            json!({"gone": reverse("a", "live")});
        let emptied = plan(&managed(), &last, "b", "gone").unwrap();
        let mut expected = last.clone();
        expected["resourceMapping"]["genericMapping"]["managed/b"]["properties"] = json!({});
        assert_eq!(emptied.after(), &expected);
        assert!(
            emptied
                .notes
                .iter()
                .any(|note| note.contains("last mapped property")),
            "{:?}",
            emptied.notes
        );
    }

    #[test]
    fn recheck_refuses_a_changed_document_or_a_newly_declared_key() {
        let before = repo(json!({"gone": reference("b")}), json!({}));
        let plan = plan(&managed(), &before, "a", "gone").unwrap();
        assert!(recheck(&plan, &managed(), &before).is_ok());

        let mut moved = before.clone();
        moved["resourceMapping"]["genericMapping"]["managed/a"]["objectClasses"] = json!(["y"]);
        assert!(
            recheck(&plan, &managed(), &moved)
                .unwrap_err()
                .contains("changed since")
        );

        let mut declared = managed();
        declared["objects"][0]["schema"]["properties"]["gone"] =
            json!({"type": "relationship", "resourceCollection": [{"path": "managed/b"}]});
        assert!(
            recheck(&plan, &declared, &before)
                .unwrap_err()
                .contains("declares it")
        );
    }

    #[test]
    fn a_dry_run_or_a_refusal_gets_no_permit() {
        assert!(authorize(true, true).is_none());
        assert!(authorize(false, false).is_none());
        assert!(authorize(false, true).is_some());
    }

    #[test]
    fn judge_tells_the_sent_document_from_the_old_one_and_from_a_third() {
        let before = repo(json!({"gone": reference("b")}), json!({}));
        let plan = plan(&managed(), &before, "a", "gone").unwrap();
        assert_eq!(judge(&plan, plan.after()), Readback::Confirmed);
        assert_eq!(judge(&plan, &before), Readback::Unchanged);
        assert_eq!(judge(&plan, &json!({})), Readback::Different);
        // The near miss: the removal landed, and something unrelated changed
        // too. A judge that checked only for the entry's absence would
        // confirm this.
        let mut near = plan.after().clone();
        near["resourceMapping"]["genericMapping"]["managed/b"]["objectClasses"] = json!(["z"]);
        assert_eq!(judge(&plan, &near), Readback::Different);
    }

    #[test]
    fn a_reader_outside_managed_generic_mappings_still_blocks() {
        // The sandbox's real shapes: an `explicitMapping` `internal/role` and
        // a `genericMapping` `internal/notification` reverse reading through a
        // managed object. Each alone must refuse.
        for (section, resource) in [
            ("explicitMapping", "internal/role"),
            ("genericMapping", "internal/notification"),
        ] {
            let mut before = repo(json!({"gone": reference("b")}), json!({}));
            before["resourceMapping"][section][resource] =
                json!({"properties": {"members": reverse("a", "gone")}});
            let error = plan(&managed(), &before, "a", "gone").unwrap_err();
            assert!(
                error.contains(&format!(
                    "{resource}.members (in {section}) (not a managed property"
                )),
                "{section}: {error}"
            );
        }
    }

    #[test]
    fn the_partner_note_needs_the_partner_mapped_as_the_reference() {
        // Declared but unmapped: the measured reassurance does not apply.
        let mut before = repo(json!({}), json!({"gone": reverse("a", "live")}));
        before["resourceMapping"]["genericMapping"]["managed/a"]["properties"] =
            json!({"other": reference("b")});
        let plan = plan(&managed(), &before, "b", "gone").unwrap();
        assert!(
            !plan
                .notes
                .iter()
                .any(|note| note.contains("keeps that mapping")),
            "{:?}",
            plan.notes
        );
        // A declared relationship mapped as the *reverse*: not the measured
        // state either (round 8). `b.back` reads through `a.live`, not
        // through the repaired key.
        let mut with_rev = managed();
        with_rev["objects"][0]["schema"]["properties"]["rev"] = json!({"type": "relationship",
            "resourceCollection": [{"path": "managed/b"}]});
        let before = repo(
            json!({"rev": reverse("b", "back")}),
            json!({"gone": reverse("a", "rev")}),
        );
        let plan = self::plan(&with_rev, &before, "b", "gone").unwrap();
        assert!(
            !plan
                .notes
                .iter()
                .any(|note| note.contains("keeps that mapping")),
            "{:?}",
            plan.notes
        );
        // Mapped, but declared as a scalar: not a relationship.
        let before = repo(
            json!({"scalar": reference("b")}),
            json!({"gone": reverse("a", "scalar")}),
        );
        let plan = self::plan(&managed(), &before, "b", "gone").unwrap();
        assert!(
            !plan
                .notes
                .iter()
                .any(|note| note.contains("keeps that mapping")),
            "{:?}",
            plan.notes
        );
    }

    /// Drives [`apply_with`] against a scripted tenant and records the order
    /// of every call.
    struct Tenant {
        managed: Value,
        repo_reads: RefCell<Vec<crate::Result<Value>>>,
        send: RefCell<Option<crate::Result<()>>>,
        backup_fails: bool,
        log: RefCell<Vec<String>>,
    }

    impl Tenant {
        fn new(
            managed: Value,
            repo_reads: Vec<crate::Result<Value>>,
            send: crate::Result<()>,
        ) -> Self {
            Self {
                managed,
                repo_reads: RefCell::new(repo_reads.into_iter().rev().collect()),
                send: RefCell::new(Some(send)),
                backup_fails: false,
                log: RefCell::new(Vec::new()),
            }
        }

        async fn run(&self, plan: &Plan, delays: usize) -> Result<Applied, Failure> {
            let permit = authorize(false, true).unwrap();
            let delays = vec![Duration::ZERO; delays];
            apply_with(
                plan,
                &permit,
                async || {
                    self.log.borrow_mut().push("read managed".into());
                    Ok(self.managed.clone())
                },
                async || {
                    self.log.borrow_mut().push("read repo.ds".into());
                    self.repo_reads
                        .borrow_mut()
                        .pop()
                        .unwrap_or_else(|| Err(crate::Error::Config("no more reads".into())))
                },
                |document| {
                    self.log.borrow_mut().push("backup".into());
                    assert_eq!(
                        document,
                        plan.before(),
                        "the backup is the pre-write document"
                    );
                    if self.backup_fails {
                        return Err(crate::Error::Config("disk full".into()));
                    }
                    Ok(PathBuf::from("/backup.json"))
                },
                async |document| {
                    self.log.borrow_mut().push("send".into());
                    assert_eq!(
                        document,
                        plan.after(),
                        "the whole intended document is sent"
                    );
                    self.send.borrow_mut().take().unwrap()
                },
                &delays,
            )
            .await
        }
    }

    fn fixture() -> (Value, Plan) {
        let before = repo(json!({"gone": reference("b")}), json!({}));
        let plan = plan(&managed(), &before, "a", "gone").unwrap();
        (before, plan)
    }

    #[tokio::test]
    async fn the_write_comes_after_the_recheck_and_the_backup_and_is_confirmed_by_reading() {
        let (before, plan) = fixture();
        let tenant = Tenant::new(
            managed(),
            vec![Ok(before.clone()), Ok(before), Ok(plan.after().clone())],
            Ok(()),
        );
        let applied = tenant.run(&plan, 3).await.unwrap();
        assert_eq!(applied.readback, Readback::Confirmed);
        assert_eq!(
            *tenant.log.borrow(),
            [
                "read managed",
                "read repo.ds",
                "backup",
                "send",
                "read repo.ds",
                "read repo.ds"
            ],
            "stops at the first confirming read"
        );
    }

    #[tokio::test]
    async fn a_document_changed_since_the_plan_writes_nothing_and_backs_up_nothing() {
        let (mut before, plan) = fixture();
        before["dbType"] = json!("other");
        let tenant = Tenant::new(managed(), vec![Ok(before)], Ok(()));
        let failure = tenant.run(&plan, 0).await.unwrap_err();
        assert!(matches!(failure, Failure::NotWritten(ref m) if m.contains("changed since")));
        assert_eq!(*tenant.log.borrow(), ["read managed", "read repo.ds"]);
    }

    #[tokio::test]
    async fn a_write_that_reads_back_unchanged_is_not_reported_confirmed() {
        // The measured PATCH behaviour: 200, and the document never moves.
        let (before, plan) = fixture();
        let tenant = Tenant::new(
            managed(),
            vec![
                Ok(before.clone()),
                Ok(before.clone()),
                Ok(before.clone()),
                Ok(before),
            ],
            Ok(()),
        );
        let applied = tenant.run(&plan, 2).await.unwrap();
        assert_eq!(applied.readback, Readback::Unchanged);
    }

    #[tokio::test]
    async fn a_failed_send_reports_unknown_with_the_backup() {
        let (before, plan) = fixture();
        let tenant = Tenant::new(
            managed(),
            vec![Ok(before)],
            Err(crate::Error::Config("connection reset".into())),
        );
        match tenant.run(&plan, 0).await.unwrap_err() {
            Failure::SendFailed { error, backup } => {
                assert!(error.contains("connection reset"));
                assert_eq!(backup, PathBuf::from("/backup.json"));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            *tenant.log.borrow(),
            ["read managed", "read repo.ds", "backup", "send"]
        );
    }

    #[tokio::test]
    async fn a_failed_last_read_is_unread_not_confirmed() {
        let (before, plan) = fixture();
        let tenant = Tenant::new(managed(), vec![Ok(before)], Ok(()));
        let applied = tenant.run(&plan, 1).await.unwrap();
        assert!(matches!(applied.readback, Readback::Unread { .. }));
    }

    #[tokio::test]
    async fn a_third_document_seen_once_is_not_forgotten() {
        // Recheck read, then a third document, then the old one twice: the
        // outcome is over every read, not the last.
        let (before, plan) = fixture();
        let mut third = before.clone();
        third["dbType"] = json!("other");
        let tenant = Tenant::new(
            managed(),
            vec![
                Ok(before.clone()),
                Ok(third),
                Ok(before.clone()),
                Ok(before),
            ],
            Ok(()),
        );
        assert_eq!(
            tenant.run(&plan, 2).await.unwrap().readback,
            Readback::Different
        );
    }

    #[tokio::test]
    async fn a_third_document_outranks_a_later_failed_read() {
        // Evidence of another writer is the stronger report; a failed read
        // after it must not replace it with "could not read".
        let (before, plan) = fixture();
        let mut third = before.clone();
        third["dbType"] = json!("other");
        let tenant = Tenant::new(
            managed(),
            vec![
                Ok(before),
                Ok(third),
                Err(crate::Error::Config("timeout".into())),
            ],
            Ok(()),
        );
        assert_eq!(
            tenant.run(&plan, 1).await.unwrap().readback,
            Readback::Different
        );
    }

    #[tokio::test]
    async fn one_failed_read_among_unchanged_ones_is_not_unchanged() {
        let (before, plan) = fixture();
        let tenant = Tenant::new(
            managed(),
            vec![
                Ok(before.clone()),
                Err(crate::Error::Config("timeout".into())),
                Ok(before),
            ],
            Ok(()),
        );
        match tenant.run(&plan, 1).await.unwrap().readback {
            Readback::Unread { error } => assert!(error.contains("timeout"), "{error}"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_key_declared_since_the_plan_writes_nothing() {
        let (before, plan) = fixture();
        let mut declared = managed();
        declared["objects"][0]["schema"]["properties"]["gone"] =
            json!({"type": "relationship", "resourceCollection": [{"path": "managed/b"}]});
        let tenant = Tenant::new(declared, vec![Ok(before)], Ok(()));
        let failure = tenant.run(&plan, 0).await.unwrap_err();
        assert!(matches!(failure, Failure::NotWritten(ref m) if m.contains("declares it")));
        assert_eq!(*tenant.log.borrow(), ["read managed", "read repo.ds"]);
    }

    #[tokio::test]
    async fn a_failed_backup_sends_nothing() {
        let (before, plan) = fixture();
        let mut tenant = Tenant::new(managed(), vec![Ok(before)], Ok(()));
        tenant.backup_fails = true;
        let failure = tenant.run(&plan, 0).await.unwrap_err();
        assert!(matches!(failure, Failure::NotWritten(ref m) if m.contains("disk full")));
        assert_eq!(
            *tenant.log.borrow(),
            ["read managed", "read repo.ds", "backup"]
        );
    }
    #[tokio::test]
    async fn an_addition_uses_the_same_recheck_backup_send_readback_sequence() {
        let before = json!({"resourceMapping": {"genericMapping": {}}});
        let plan = plan_add(&managed(), &before, "a", "live").unwrap();
        let tenant = Tenant::new(
            managed(),
            vec![Ok(before), Ok(plan.after().clone())],
            Ok(()),
        );
        assert_eq!(
            tenant.run(&plan, 0).await.unwrap().readback,
            Readback::Confirmed
        );
        assert_eq!(
            *tenant.log.borrow(),
            [
                "read managed",
                "read repo.ds",
                "backup",
                "send",
                "read repo.ds"
            ]
        );
        assert!(verify::mapped(plan.after(), "a", "live").is_some());
        assert!(verify::mapped(plan.after(), "b", "back").is_some());
    }

    #[tokio::test]
    async fn addition_schema_or_repo_drift_cannot_send_a_partial_pair() {
        let before = json!({"resourceMapping": {"genericMapping": {}}});
        let plan = plan_add(&managed(), &before, "a", "live").unwrap();
        let mut changed = managed();
        let live = changed["objects"][0]["schema"]["properties"]["live"].take();
        changed["objects"][0]["schema"]["properties"]["live"] =
            json!({"type": "array", "items": live});
        let tenant = Tenant::new(changed, vec![Ok(before.clone())], Ok(()));
        assert!(matches!(
            tenant.run(&plan, 0).await,
            Err(Failure::NotWritten(_))
        ));
        assert_eq!(*tenant.log.borrow(), ["read managed", "read repo.ds"]);
        let mut mixed = before;
        mixed["resourceMapping"]["genericMapping"]["managed/a"] =
            json!({"properties": {"live": reference("b")}});
        let tenant = Tenant::new(managed(), vec![Ok(mixed)], Ok(()));
        assert!(matches!(
            tenant.run(&plan, 0).await,
            Err(Failure::NotWritten(_))
        ));
        assert_eq!(*tenant.log.borrow(), ["read managed", "read repo.ds"]);
    }

    #[test]
    fn addition_planning_never_authorizes_a_dry_run() {
        let before = json!({"resourceMapping": {"genericMapping": {}}});
        let plan = plan_add(&managed(), &before, "a", "live").unwrap();
        assert!(plan.addition().is_some());
        assert!(authorize(true, true).is_none());
        assert!(authorize(true, false).is_none());
        // apply_with and the HTTP wrapper both require the opaque permit.
    }
    /// `gone` is not declared in `config/managed`; its per-type entry is what
    /// a deleted type leaves behind.
    fn repo_with_empty_entry_for_gone() -> Value {
        let mut doc = repo(json!({}), json!({}));
        doc["resourceMapping"]["genericMapping"]["managed/gone"] =
            super::super::storage::generated_entry("gone");
        doc
    }

    #[test]
    fn an_empty_entry_of_a_deleted_type_is_planned_for_removal_and_nothing_else_moves() {
        let before = repo_with_empty_entry_for_gone();
        let plan = plan_remove_entry(&managed(), &before, "gone").unwrap();
        assert_eq!(plan.label(), "managed/gone");
        assert!(plan.notes.is_empty());
        let mut expected = before.clone();
        expected["resourceMapping"]["genericMapping"]
            .as_object_mut()
            .unwrap()
            .remove("managed/gone");
        assert_eq!(plan.after(), &expected);
        // The mapped pair on the live types is untouched.
        assert!(verify::mapped(plan.after(), "a", "live").is_some());
        // An entry with no `properties` member at all is just as empty.
        let mut bare = before;
        bare["resourceMapping"]["genericMapping"]["managed/gone"]
            .as_object_mut()
            .unwrap()
            .remove("properties");
        assert!(plan_remove_entry(&managed(), &bare, "gone").is_ok());
    }

    /// The discriminating cases: each is an entry a looser rule ("remove any
    /// per-type entry that looks empty") would take.
    #[test]
    fn an_entry_is_refused_while_it_maps_a_property_or_is_named_by_a_mapping() {
        // The type still exists and its entry is empty: allowed (measured
        // 2026-11-10), with a note; the same entry still mapping is refused.
        let mut with_c = managed();
        with_c["objects"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "c", "schema": {"properties": {}}}));
        let mut existing = repo(json!({}), json!({}));
        existing["resourceMapping"]["genericMapping"]["managed/c"] =
            super::super::storage::generated_entry("c");
        let plan = plan_remove_entry(&with_c, &existing, "c").unwrap();
        assert!(plan.notes.iter().any(|n| n.contains("still declared")));
        existing["resourceMapping"]["genericMapping"]["managed/c"]["properties"] =
            json!({"p": reference("b")});
        let error = plan_remove_entry(&with_c, &existing, "c").unwrap_err();
        assert!(error.contains("still maps p"), "{error}");
        // The entry still maps a property.
        let mut mapped = repo_with_empty_entry_for_gone();
        mapped["resourceMapping"]["genericMapping"]["managed/gone"]["properties"] =
            json!({"p": reference("b")});
        let error = plan_remove_entry(&managed(), &mapped, "gone").unwrap_err();
        assert!(error.contains("still maps p"), "{error}");
        // Another mapping names the gone type.
        let mut read = repo_with_empty_entry_for_gone();
        read["resourceMapping"]["genericMapping"]["managed/b"]["properties"]["gone"] =
            reference("gone");
        let error = plan_remove_entry(&managed(), &read, "gone").unwrap_err();
        assert!(
            error.contains("b") && error.contains("managed/gone"),
            "{error}"
        );
        // No entry at all.
        let error = plan_remove_entry(&managed(), &repo(json!({}), json!({})), "gone").unwrap_err();
        assert!(error.contains("nothing to remove"), "{error}");
    }

    /// A custom storage layout under an otherwise empty entry: every field
    /// name is one aic writes, but the values place records elsewhere. Each
    /// difference is refused and named, by the planner and by the recheck.
    #[tokio::test]
    async fn a_custom_storage_layout_is_refused_by_value_not_by_field_name() {
        let generated = super::super::storage::generated_entry("gone");
        let mut with_c = managed();
        with_c["objects"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "custom_widget", "schema": {"properties": {}}}));
        let custom = json!({
            "dnTemplate": "ou=archive,dc=example,dc=com",
            "jsonAttribute": "custom-json",
            "objectClasses": ["customClass"],
            "jsonQueryEqualityMatchingRule": "customMatch",
            "properties": {}
        });
        let mut doc = repo(json!({}), json!({}));
        doc["resourceMapping"]["genericMapping"]["managed/custom_widget"] = custom;
        let error = plan_remove_entry(&with_c, &doc, "custom_widget").unwrap_err();
        for field in [
            "dnTemplate",
            "jsonAttribute",
            "objectClasses",
            "jsonQueryEqualityMatchingRule",
        ] {
            assert!(error.contains(field), "{error}");
        }
        // An unknown extra field is refused too, and named alone.
        let mut extra = repo_with_empty_entry_for_gone();
        extra["resourceMapping"]["genericMapping"]["managed/gone"]["unusual"] = json!(1);
        let error = plan_remove_entry(&managed(), &extra, "gone").unwrap_err();
        assert!(error.contains("differs in unusual"), "{error}");
        // One generated value changed.
        let mut one = repo_with_empty_entry_for_gone();
        one["resourceMapping"]["genericMapping"]["managed/gone"]["dnTemplate"] = json!("ou=x");
        let error = plan_remove_entry(&managed(), &one, "gone").unwrap_err();
        assert!(error.contains("differs in dnTemplate"), "{error}");
        // The recheck re-plans: a plan made against the generated layout is
        // refused once the entry at write time holds a custom one.
        let good = repo_with_empty_entry_for_gone();
        let plan = plan_remove_entry(&managed(), &good, "gone").unwrap();
        let mut swapped = good.clone();
        swapped["resourceMapping"]["genericMapping"]["managed/gone"] = json!({
            "dnTemplate": "ou=archive,dc=example,dc=com", "properties": {}});
        assert!(recheck(&plan, &managed(), &swapped).is_err());
        // The generated layout itself passes the recheck.
        assert!(recheck(&plan, &managed(), &good).is_ok());
        assert_eq!(
            good["resourceMapping"]["genericMapping"]["managed/gone"],
            generated
        );
    }

    #[tokio::test]
    async fn an_entry_removal_uses_the_same_recheck_backup_send_readback_sequence() {
        let before = repo_with_empty_entry_for_gone();
        let plan = plan_remove_entry(&managed(), &before, "gone").unwrap();
        let tenant = Tenant::new(
            managed(),
            vec![Ok(before.clone()), Ok(plan.after().clone())],
            Ok(()),
        );
        assert_eq!(
            tenant.run(&plan, 0).await.unwrap().readback,
            Readback::Confirmed
        );
        assert_eq!(
            *tenant.log.borrow(),
            [
                "read managed",
                "read repo.ds",
                "backup",
                "send",
                "read repo.ds"
            ]
        );
        // A mapping started naming the type between plan and write: repo.ds
        // differs from what was planned, so nothing is sent.
        let mut drifted = before.clone();
        drifted["resourceMapping"]["genericMapping"]["managed/b"]["properties"]["gone"] =
            reference("gone");
        let tenant = Tenant::new(managed(), vec![Ok(drifted)], Ok(()));
        assert!(matches!(
            tenant.run(&plan, 0).await,
            Err(Failure::NotWritten(_))
        ));
        assert_eq!(*tenant.log.borrow(), ["read managed", "read repo.ds"]);
    }
}
