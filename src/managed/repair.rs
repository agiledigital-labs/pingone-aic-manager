//! `aic managed relationship repair` — remove a `config/repo.ds` relationship
//! mapping that names no declared property.
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
//! that another mapping reads through, removing the last mapped property of
//! an object, and anything on an object `config/managed` does not declare.
//!
//! Remove only. It never adds or edits a mapping.
//!
//! Nothing here performs I/O except through the closures [`apply_with`] is
//! handed, so a test drives the real order of reads and writes.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::managed::verify::{self, Mapping};

/// What removing one stale mapping will do, computed from one read of each
/// document.
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
    /// `before` minus the one entry: the whole document that is `PUT`.
    #[serde(skip)]
    after: Value,
}

impl Plan {
    pub fn before(&self) -> &Value {
        &self.before
    }

    pub fn after(&self) -> &Value {
        &self.after
    }

    pub fn label(&self) -> String {
        format!("{}.{}", self.object, self.key)
    }
}

/// Proof that the operator consented to a non-dry-run repair. The field is
/// private, so [`authorize`] is the only place one is made, and
/// [`apply_with`] requires one: a dry run cannot reach the write.
pub struct RepairPermit(());

pub fn authorize(dry_run: bool, consented: bool) -> Option<RepairPermit> {
    (!dry_run && consented).then_some(RepairPermit(()))
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
    let Some(entries) = verify::mappings_of(repo_ds, object) else {
        return Err(format!(
            "config/repo.ds has no mappings for managed/{object}; nothing to repair"
        ));
    };
    if !entries.contains_key(key) {
        return Err(format!(
            "config/repo.ds has no mapping for {label}; nothing to repair"
        ));
    }
    let Some(mapping) = verify::mapped(repo_ds, object, key) else {
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
            .map(|dependent| {
                if dependent.declared {
                    format!("{} (declared)", dependent.label)
                } else {
                    format!("{} (also stale: repair it first)", dependent.label)
                }
            })
            .collect();
        return Err(format!(
            "refusing to repair {label}: config/repo.ds reads through it from {}. Removing a \
             mapping another one reads through is unmeasured",
            names.join(", ")
        ));
    }
    if entries.len() == 1 {
        return Err(format!(
            "refusing to repair {label}: it is the last mapped property of managed/{object}, \
             and removing an object's last mapping is unmeasured"
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
    if mapping.kind == "reverseReference"
        && let (Some(partner_object), Some(partner_key)) = (
            mapping
                .resource_path
                .as_deref()
                .and_then(|path| path.strip_prefix("managed/")),
            mapping.property_name.as_deref(),
        )
        && verify::properties(managed, partner_object).is_some_and(|p| p.contains_key(partner_key))
    {
        notes.push(format!(
            "{partner_object}.{partner_key} is declared and keeps its own mapping; when this \
             was measured its links still resolved and new links still wrote afterwards"
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
        before: repo_ds.clone(),
        after,
    })
}

/// A `reverseReference` elsewhere in `repo.ds` that reads through
/// `object.key`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Dependent {
    label: String,
    declared: bool,
}

fn dependents(managed: &Value, repo_ds: &Value, object: &str, key: &str) -> Vec<Dependent> {
    let path = format!("managed/{object}");
    let Some(generic) = repo_ds
        .pointer("/resourceMapping/genericMapping")
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for resource in generic.keys() {
        let Some(other) = resource.strip_prefix("managed/") else {
            continue;
        };
        for other_key in verify::mappings_of(repo_ds, other)
            .into_iter()
            .flat_map(|entries| entries.keys())
        {
            if other == object && other_key == key {
                continue;
            }
            let Some(mapping) = verify::mapped(repo_ds, other, other_key) else {
                continue;
            };
            if mapping.kind == "reverseReference"
                && mapping.resource_path.as_deref() == Some(path.as_str())
                && mapping.property_name.as_deref() == Some(key)
            {
                out.push(Dependent {
                    label: format!("{other}.{other_key}"),
                    declared: verify::properties(managed, other)
                        .is_some_and(|props| props.contains_key(other_key)),
                });
            }
        }
    }
    out
}

/// RFC 6901 escaping for one pointer token.
fn pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// The pre-write check against fresh reads of both documents: `repo.ds` must
/// be exactly what the plan was computed from, and the plan must still hold
/// against the fresh `config/managed` (a property declared since would make
/// the mapping live again).
pub fn recheck(plan: &Plan, managed: &Value, repo_ds: &Value) -> Result<(), String> {
    if repo_ds != &plan.before {
        return Err(format!(
            "config/repo.ds changed since it was read for {}; nothing was written. Run repair \
             again to plan against the current document",
            plan.label()
        ));
    }
    self::plan(managed, repo_ds, &plan.object, &plan.key).map(|_| ())
}

/// How the read-back left things.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Readback {
    /// A read returned exactly the document sent.
    Confirmed,
    /// Every read still returned the pre-write document: the write did not
    /// take effect, at least not yet.
    Unchanged,
    /// The last read was neither: someone else wrote, or the tenant
    /// normalised what was sent.
    Different,
    /// The last read failed.
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
    _permit: &RepairPermit,
    mut read_managed: impl AsyncFnMut() -> crate::Result<Value>,
    mut read_repo_ds: impl AsyncFnMut() -> crate::Result<Value>,
    backup: impl FnOnce(&Value) -> crate::Result<PathBuf>,
    mut send: impl AsyncFnMut(&Value) -> crate::Result<()>,
    delays: &[Duration],
) -> Result<Applied, Failure> {
    let not_written = |error: crate::Error| Failure::NotWritten(error.to_string());
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

    let mut readback = Readback::Unread {
        error: "no read-back was attempted".into(),
    };
    for pause in std::iter::once(None).chain(delays.iter().map(Some)) {
        if let Some(pause) = pause {
            tokio::time::sleep(*pause).await;
        }
        readback = match read_repo_ds().await {
            Ok(read) => judge(plan, &read),
            Err(error) => Readback::Unread {
                error: error.to_string(),
            },
        };
        if readback == Readback::Confirmed {
            break;
        }
    }
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
                .any(|note| note.starts_with("a.live is declared")),
            "{:?}",
            plan.notes
        );

        let both_stale = repo(
            json!({"old": reference("b")}),
            json!({"gone": reverse("a", "old")}),
        );
        let plan = self::plan(&managed(), &both_stale, "b", "gone").unwrap();
        assert!(
            !plan.notes.iter().any(|note| note.contains("is declared")),
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

        // Last mapping on the object: `b` holds only the stale one.
        let mut last = repo(json!({}), json!({}));
        last["resourceMapping"]["genericMapping"]["managed/b"]["properties"] =
            json!({"gone": reverse("a", "live")});
        let error = plan(&managed(), &last, "b", "gone").unwrap_err();
        assert!(error.contains("last mapped property"), "{error}");
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
    }

    /// Drives [`apply_with`] against a scripted tenant and records the order
    /// of every call.
    struct Tenant {
        managed: Value,
        repo_reads: RefCell<Vec<crate::Result<Value>>>,
        send: RefCell<Option<crate::Result<()>>>,
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
}
