//! Pure, tenant-free transforms over the raw `config/access` document.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use crate::access::api;
use crate::access::spec::{Amendment, RuleEdit, RuleSpec, TouchedIndices};
use crate::access::state::{DeleteState, Document, FormKind, RuleFormState};
use crate::app::event::{AppEvent, ToastKind};
use crate::app::prod_confirm::PendingProdAction;
use crate::app::{App, InputMode};
use crate::config::ProjectConfig;
use crate::config::tenant::TenantTheme;
use crate::undo::{
    Capability, ConflictCheck, EntryStatus, Sensitivity, UndoEntry, UndoExecutor, UndoId, UndoOp,
};
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub enum ProdAction {
    Write(Box<WriteRequest>),
    Undo(UndoId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeMode {
    Create,
    Edit,
    DeleteConfirm,
}

impl ResumeMode {
    fn input_mode(self) -> InputMode {
        let mode = match self {
            Self::Create => crate::access::screen::Mode::Create,
            Self::Edit => crate::access::screen::Mode::Edit,
            Self::DeleteConfirm => crate::access::screen::Mode::DeleteConfirm,
        };
        InputMode::Access(mode)
    }
}

#[derive(Debug, Clone)]
pub struct WriteRequest {
    pub tenant: String,
    pub expected_document_digest: String,
    pub previous_document: Value,
    pub amendment: Amendment,
    pub expected_rule: Option<(usize, String)>,
    pub after: Value,
    pub description: String,
    pub resume_mode: ResumeMode,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum WriteFailure {
    Stale(String),
    NotWritten(String),
    AcceptedButUnconfirmed(String),
}

#[derive(Debug)]
pub enum UndoFailure {
    Conflict(String),
    Failed(String),
}

/// One changed rule at its document index.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleChange {
    pub index: usize,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

/// Rule-level changes and the number of rules that remained byte-identical.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Changes {
    pub changed: Vec<RuleChange>,
    pub unchanged: usize,
    pub touched: TouchedIndices,
    /// True when an apply diff could only recover approximate source positions.
    pub positions_approximate: bool,
}

/// How changed rules should be paired when producing a summary.
pub enum ChangeBasis<'a> {
    /// A built-in transform knows its exact original and resulting indices.
    Touched(&'a TouchedIndices),
    /// A hand-edited document must be matched by content.
    Multiset,
}

/// A transformed document and the exact original/result indices it touched.
#[derive(Debug, Clone, PartialEq)]
pub struct Transformed {
    pub document: Value,
    pub touched: TouchedIndices,
}

/// A completed pure amendment plus its exact rule-level change summary.
#[derive(Debug, Clone, PartialEq)]
pub struct Amended {
    pub after: Value,
    pub touched: TouchedIndices,
    pub summary: Changes,
}

/// Apply one CLI/TUI amendment without tenant I/O or caller policy.
pub fn amend(before: &Value, amendment: Amendment) -> Result<Amended> {
    let (after, exact_touched) = match amendment {
        Amendment::Add(rule) => {
            let transformed = append(before, rule)?;
            (transformed.document, Some(transformed.touched))
        }
        Amendment::Edit { index, edit } => {
            let transformed = replace_at(before, index, edit)?;
            (transformed.document, Some(transformed.touched))
        }
        Amendment::Remove(indices) => {
            let transformed = remove_at(before, &indices)?;
            (transformed.document, Some(transformed.touched))
        }
        Amendment::Apply(mut document) => {
            normalize_apply_id(&mut document)?;
            (document, None)
        }
    };
    let summary = match exact_touched.as_ref() {
        Some(touched) => changes(before, &after, ChangeBasis::Touched(touched)),
        None => changes(before, &after, ChangeBasis::Multiset),
    };
    Ok(Amended {
        after,
        touched: summary.touched.clone(),
        summary,
    })
}

/// The `configs` array of an access document.
pub fn rules(doc: &Value) -> Result<&Vec<Value>> {
    doc.get("configs")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("unexpected /openidm/config/access shape: {doc}"),
        })
}

/// Append one grant without rebuilding or reordering existing rules.
pub fn append(doc: &Value, spec: RuleSpec) -> Result<Transformed> {
    let index = rules(doc)?.len();
    let mut amended = doc.clone();
    rules_mut(&mut amended)?.push(new_rule_value(spec));
    Ok(Transformed {
        document: amended,
        touched: TouchedIndices::appended(index),
    })
}

/// Mutate only supplied fields on the rule at `index`.
pub fn replace_at(doc: &Value, index: usize, edit: RuleEdit) -> Result<Transformed> {
    let len = rules(doc)?.len();
    crate::access::spec::ensure_index(index, len)?;
    reject_conflicting_optional_edits(&edit)?;

    let mut amended = doc.clone();
    let rule = rules_mut(&mut amended)?
        .get_mut(index)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| Error::Config(format!("access rule at index {index} is not an object")))?;

    replace_when_some(rule, "pattern", edit.pattern);
    replace_when_some(rule, "roles", edit.roles);
    replace_when_some(rule, "methods", edit.methods);
    apply_optional(rule, "actions", edit.actions, edit.clear_actions);
    apply_optional(
        rule,
        "customAuthz",
        edit.custom_authz,
        edit.clear_custom_authz,
    );
    apply_optional(
        rule,
        "excludePatterns",
        edit.exclude_patterns,
        edit.clear_exclude_patterns,
    );
    Ok(Transformed {
        document: amended,
        touched: TouchedIndices::replaced(index),
    })
}

/// Remove the original indices as one operation, without index-shift mistakes.
pub fn remove_at(doc: &Value, indices: &[usize]) -> Result<Transformed> {
    let len = rules(doc)?.len();
    for &index in indices {
        crate::access::spec::ensure_index(index, len)?;
    }

    let mut amended = doc.clone();
    let rules = rules_mut(&mut amended)?;
    let unique = indices.iter().copied().collect::<BTreeSet<_>>();
    for &index in unique.iter().rev() {
        rules.remove(index);
    }
    Ok(Transformed {
        document: amended,
        touched: TouchedIndices::removed(unique),
    })
}

/// Compare rule arrays using exact transform indices or content matching.
pub fn changes(before: &Value, after: &Value, basis: ChangeBasis<'_>) -> Changes {
    let before = before
        .get("configs")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let after = after
        .get("configs")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();

    match basis {
        ChangeBasis::Touched(touched) => touched_changes(before, after, touched),
        ChangeBasis::Multiset => multiset_changes(before, after),
    }
}

fn touched_changes(before: &[Value], after: &[Value], touched: &TouchedIndices) -> Changes {
    let mut summary = Changes {
        touched: touched.clone(),
        ..Changes::default()
    };
    for &index in touched.before().intersection(touched.after()) {
        let old = before.get(index);
        let new = after.get(index);
        if old != new {
            summary.changed.push(RuleChange {
                index,
                before: old.cloned(),
                after: new.cloned(),
            });
        }
    }
    for &index in touched.before().difference(touched.after()) {
        summary.changed.push(RuleChange {
            index,
            before: before.get(index).cloned(),
            after: None,
        });
    }
    for &index in touched.after().difference(touched.before()) {
        summary.changed.push(RuleChange {
            index,
            before: None,
            after: after.get(index).cloned(),
        });
    }
    let paired_changes = touched
        .before()
        .intersection(touched.after())
        .filter(|&&index| before.get(index) != after.get(index))
        .count();
    summary.unchanged = before.len().min(after.len()).saturating_sub(paired_changes);
    summary
}

fn multiset_changes(before: &[Value], after: &[Value]) -> Changes {
    let mut used_before = vec![false; before.len()];
    let mut added = Vec::new();
    let mut unchanged = 0;

    for (index, rule) in after.iter().enumerate() {
        if let Some((matched, _)) = before
            .iter()
            .enumerate()
            .find(|(candidate, old)| !used_before[*candidate] && *old == rule)
        {
            used_before[matched] = true;
            unchanged += 1;
        } else {
            added.push(RuleChange {
                index,
                before: None,
                after: Some(rule.clone()),
            });
        }
    }

    let mut changed = before
        .iter()
        .enumerate()
        .filter(|(index, _)| !used_before[*index])
        .map(|(index, rule)| RuleChange {
            index,
            before: Some(rule.clone()),
            after: None,
        })
        .collect::<Vec<_>>();
    changed.extend(added);
    let touched_before = changed
        .iter()
        .filter(|change| change.before.is_some())
        .map(|change| change.index)
        .collect();
    let touched_after = changed
        .iter()
        .filter(|change| change.after.is_some())
        .map(|change| change.index)
        .collect();
    Changes {
        changed,
        unchanged,
        touched: TouchedIndices::from_sets(touched_before, touched_after),
        positions_approximate: duplicate_flags(before)
            .into_iter()
            .any(|duplicate| duplicate),
    }
}

/// Mark every rule that is byte-identical to at least one other rule.
pub fn duplicate_flags(rules: &[Value]) -> Vec<bool> {
    rules
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            rules
                .iter()
                .enumerate()
                .any(|(other, candidate)| other != index && candidate == rule)
        })
        .collect()
}

fn rules_mut(doc: &mut Value) -> Result<&mut Vec<Value>> {
    if doc.get("configs").and_then(Value::as_array).is_none() {
        return Err(Error::Api {
            status: 0,
            body: format!("unexpected /openidm/config/access shape: {doc}"),
        });
    }
    Ok(doc
        .get_mut("configs")
        .and_then(Value::as_array_mut)
        .expect("checked configs array above"))
}

fn reject_conflicting_optional_edits(edit: &RuleEdit) -> Result<()> {
    for (key, value, clear) in [
        ("actions", &edit.actions, edit.clear_actions),
        ("customAuthz", &edit.custom_authz, edit.clear_custom_authz),
        (
            "excludePatterns",
            &edit.exclude_patterns,
            edit.clear_exclude_patterns,
        ),
    ] {
        if value.is_some() && clear {
            return Err(Error::Config(format!(
                "cannot set and clear access rule field {key:?} in the same edit"
            )));
        }
    }
    Ok(())
}

fn replace_when_some(rule: &mut Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        rule.insert(key.to_string(), Value::String(value));
    }
}

fn apply_optional(rule: &mut Map<String, Value>, key: &str, value: Option<String>, clear: bool) {
    if clear {
        rule.remove(key);
    } else {
        replace_when_some(rule, key, value);
    }
}

fn new_rule_value(spec: RuleSpec) -> Value {
    let mut rule = Map::new();
    rule.insert("pattern".into(), Value::String(spec.pattern));
    rule.insert("roles".into(), Value::String(spec.roles));
    rule.insert("methods".into(), Value::String(spec.methods));
    replace_when_some(&mut rule, "actions", spec.actions);
    replace_when_some(&mut rule, "customAuthz", spec.custom_authz);
    replace_when_some(&mut rule, "excludePatterns", spec.exclude_patterns);
    Value::Object(rule)
}

fn normalize_apply_id(document: &mut Value) -> Result<()> {
    let Some(object) = document.as_object_mut() else {
        return Ok(());
    };
    match object.get("_id") {
        None => {
            object.insert("_id".into(), Value::String("access".into()));
            Ok(())
        }
        Some(Value::String(id)) if id == "access" => Ok(()),
        Some(other) => Err(Error::Config(format!(
            "config/access `_id` must be \"access\", got {other}"
        ))),
    }
}

pub fn request_from_form(form: &RuleFormState) -> Result<WriteRequest> {
    let amendment = form.amendment();
    let amended = amend(&form.original_document, amendment.clone())?;
    let warnings = validate_amended(&amended, form.known_roles.as_ref())?;
    if amended.summary.changed.is_empty() {
        return Err(Error::Config("access rule is unchanged".into()));
    }
    let (expected_rule, description, resume_mode) = match form.kind {
        FormKind::Create => (
            None,
            "Remove newly created access rule".into(),
            ResumeMode::Create,
        ),
        FormKind::Edit { index } => (
            form.original_rule_digest
                .clone()
                .map(|digest| (index, digest)),
            format!("Revert access rule #{index}"),
            ResumeMode::Edit,
        ),
    };
    Ok(WriteRequest {
        tenant: form.tenant.clone(),
        expected_document_digest: form.original_digest.clone(),
        previous_document: form.original_document.clone(),
        amendment,
        expected_rule,
        after: amended.after,
        description,
        resume_mode,
        warnings,
    })
}

pub fn request_from_delete(delete: &DeleteState) -> Result<WriteRequest> {
    if !delete.confirmed() {
        return Err(Error::Config(
            "access rule deletion requires confirmation".into(),
        ));
    }
    let amendment = Amendment::Remove(vec![delete.index]);
    let amended = amend(&delete.original_document, amendment.clone())?;
    let warnings = validate_amended(&amended, None)?;
    Ok(WriteRequest {
        tenant: delete.tenant.clone(),
        expected_document_digest: delete.original_digest.clone(),
        previous_document: delete.original_document.clone(),
        amendment,
        expected_rule: Some((delete.index, delete.rule_digest.clone())),
        after: amended.after,
        description: format!("Restore access rule #{}", delete.index),
        resume_mode: ResumeMode::DeleteConfirm,
        warnings,
    })
}

fn validate_amended(
    amended: &Amended,
    known_roles: Option<&crate::access::spec::RoleIndex>,
) -> Result<Vec<String>> {
    let findings = crate::access::spec::validate_document(
        &amended.after,
        known_roles,
        crate::access::spec::WarningScope::Touched(&amended.touched),
    );
    if findings.errors.is_empty() {
        return Ok(findings
            .warnings
            .into_iter()
            .map(|finding| {
                finding.index.map_or(finding.message.clone(), |index| {
                    format!("rule #{index}: {}", finding.message)
                })
            })
            .collect());
    }
    let messages = findings
        .errors
        .into_iter()
        .map(|finding| {
            finding.index.map_or(finding.message.clone(), |index| {
                format!("rule #{index}: {}", finding.message)
            })
        })
        .collect::<Vec<_>>()
        .join("; ");
    Err(Error::Config(format!(
        "config/access validation failed: {messages}"
    )))
}

pub fn submit_write(app: &mut App, request: WriteRequest) {
    if app
        .active_tenant()
        .is_some_and(|tenant| tenant.theme == TenantTheme::Production)
    {
        if let Some(form) = app.access.form.as_mut() {
            form.confirming = false;
        }
        app.prod_confirm.pending = Some(PendingProdAction::Access(ProdAction::Write(Box::new(
            request,
        ))));
        app.input_mode = InputMode::ProdConfirm;
        return;
    }
    execute_write(app, request, false);
}

pub fn execute_write(app: &mut App, request: WriteRequest, confirmed_prod: bool) {
    if app.access.in_flight_writes.contains(&request.tenant) {
        app.push_toast(ToastKind::Info, "An Access write is already in progress");
        return;
    }
    let undo_id = match record_write_undo(app.undo.as_mut(), &request) {
        Ok(id) => id,
        Err(error) => {
            set_draft_error(app, request.resume_mode, format!("Save cancelled: {error}"));
            return;
        }
    };
    app.access.in_flight_writes.insert(request.tenant.clone());
    app.input_mode = InputMode::Normal;

    let tenant = request.tenant.clone();
    let after = request.after.clone();
    let resume_mode = request.resume_mode;
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = write_request(&request, confirmed_prod).await;
        let _ = tx.send(AppEvent::Access(
            crate::access::screen::Event::WriteResult {
                tenant,
                after,
                undo_id,
                resume_mode,
                result,
            },
        ));
    });
}

pub(crate) fn record_write_undo(
    undo: &mut dyn crate::undo::UndoLog,
    request: &WriteRequest,
) -> Result<UndoId> {
    undo.record(UndoEntry::pending(
        request.tenant.clone(),
        "access",
        request.description.clone(),
        Sensitivity::TenantConfig,
        Capability::Undoable,
        Some(UndoOp::AccessConfigReplace {
            tenant: request.tenant.clone(),
            body: request.previous_document.clone(),
        }),
        ConflictCheck::ContentEqualsAfter {
            body: request.after.clone(),
        },
    ))
}

async fn write_request(
    request: &WriteRequest,
    confirmed_prod: bool,
) -> std::result::Result<(), WriteFailure> {
    let live = api::get_access(&request.tenant)
        .await
        .map_err(|error| WriteFailure::NotWritten(error.to_string()))?;
    let amended = prepare_live_write(request, &live)?;
    backup_document(&request.tenant, &live, Utc::now())
        .map_err(|error| WriteFailure::NotWritten(error.to_string()))?;
    api::put_access_confirmed(&request.tenant, amended.after, confirmed_prod)
        .await
        .map_err(|error| match error {
            api::ConfirmedWriteError::NotWritten(error) => {
                WriteFailure::NotWritten(error.to_string())
            }
            api::ConfirmedWriteError::AcceptedButUnconfirmed(message) => {
                WriteFailure::AcceptedButUnconfirmed(message)
            }
        })
}

pub(crate) fn backup_document(
    tenant: &str,
    document: &Value,
    now: DateTime<Utc>,
) -> Result<PathBuf> {
    let path = ProjectConfig::dir().join("backups").join(backup_filename(
        tenant,
        now,
        uuid::Uuid::new_v4(),
    ));
    let write = || -> Result<()> {
        ProjectConfig::write_gitignore()?;
        let mut bytes = serde_json::to_vec_pretty(document)?;
        bytes.push(b'\n');
        write_private_file(&path, &bytes, true)
    };
    write().map_err(|error| {
        Error::Config(format!(
            "could not create config/access backup {}: {error}; nothing was written to the tenant",
            path.display()
        ))
    })?;
    Ok(path)
}

fn backup_filename(tenant: &str, now: DateTime<Utc>, nonce: uuid::Uuid) -> String {
    let tenant = tenant
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!(
        "access-{tenant}-{}-{nonce}.json",
        now.format("%Y%m%dT%H%M%SZ")
    )
}

/// Write `bytes` at mode 0600, creating parents. `exclusive` refuses an existing
/// path, which is what a backup wants — a nonce in the filename plus
/// `create_new` means a backup can never overwrite an earlier one. `aic access
/// get --out` passes false, because overwriting the file you named is the point.
///
/// The one writer for this feature. It arrived as two — a backup-shaped copy
/// here and `cli.rs`'s general one — which is the duplication `REVIEW.md`'s
/// standing check about auditing `cli.rs` privates exists to catch.
pub(crate) fn write_private_file(path: &Path, bytes: &[u8], exclusive: bool) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).mode(0o600);
    if exclusive {
        options.create_new(true);
    } else {
        options.create(true).truncate(true);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(())
}

pub(crate) fn prepare_live_write(
    request: &WriteRequest,
    live: &Value,
) -> std::result::Result<Amended, WriteFailure> {
    crate::access::spec::check_digest(Some(&request.expected_document_digest), live)
        .map_err(|error| WriteFailure::Stale(error.to_string()))?;
    if let Some((index, expected_digest)) = &request.expected_rule {
        let rules = rules(live).map_err(|error| WriteFailure::Stale(error.to_string()))?;
        let Some(rule) = rules.get(*index) else {
            return Err(WriteFailure::Stale(format!(
                "selected access rule #{index} no longer exists; nothing was written"
            )));
        };
        let actual = crate::access::spec::digest(rule);
        if actual != *expected_digest {
            return Err(WriteFailure::Stale(format!(
                "selected access rule #{index} changed from digest {expected_digest} to {actual}; nothing was written"
            )));
        }
    }
    amend(live, request.amendment.clone())
        .map_err(|error| WriteFailure::NotWritten(error.to_string()))
}

pub fn apply_write_result(
    app: &mut App,
    tenant: String,
    after: Value,
    undo_id: UndoId,
    resume_mode: ResumeMode,
    result: std::result::Result<(), WriteFailure>,
) {
    app.access.in_flight_writes.remove(&tenant);
    if let Some(status) = undo_disposition(&result) {
        mark_undo(app, undo_id, status);
    }
    match result {
        Ok(()) => {
            match Document::from_value(after) {
                Ok(document) => {
                    app.access.data.insert(
                        tenant.clone(),
                        crate::access::state::LoadState::Loaded(document),
                    );
                }
                Err(error) => {
                    app.access.data.remove(&tenant);
                    app.push_toast(
                        ToastKind::Error,
                        format!("Access saved, but its local view failed: {error}"),
                    );
                }
            }
            app.access.form = None;
            app.access.pending_delete = None;
            app.input_mode = InputMode::Normal;
            let count = crate::access::screen::row_count(app);
            app.access.clamp_selection(count);
            app.push_toast(
                ToastKind::Success,
                "Access rules updated. Press ^Z to undo.",
            );
        }
        Err(WriteFailure::Stale(message)) => {
            set_draft_error(
                app,
                resume_mode,
                format!("{message}. {}", retained_guidance(resume_mode)),
            );
            app.push_toast(ToastKind::Warning, "Access write blocked by remote changes");
        }
        Err(WriteFailure::NotWritten(message)) => {
            set_draft_error(app, resume_mode, format!("Access save failed: {message}"));
            app.push_toast(ToastKind::Error, "Access write failed; nothing was written");
        }
        Err(WriteFailure::AcceptedButUnconfirmed(message)) => {
            app.access.data.remove(&tenant);
            set_draft_error(
                app,
                resume_mode,
                format!("{message}. {}", retained_guidance(resume_mode)),
            );
            app.push_toast(
                ToastKind::Error,
                "Access write was accepted but could not be confirmed",
            );
        }
    }
}

pub(crate) fn undo_disposition(
    result: &std::result::Result<(), WriteFailure>,
) -> Option<EntryStatus> {
    match result {
        Ok(()) | Err(WriteFailure::AcceptedButUnconfirmed(_)) => None,
        Err(WriteFailure::Stale(_) | WriteFailure::NotWritten(_)) => {
            Some(EntryStatus::AppliedFailure)
        }
    }
}

fn retained_guidance(mode: ResumeMode) -> &'static str {
    match mode {
        ResumeMode::Create | ResumeMode::Edit => {
            "Your edit is preserved in this form; cancel when ready, then refresh before retrying."
        }
        ResumeMode::DeleteConfirm => {
            "The selected index and rule digest remain in the confirmation; cancel, then refresh before retrying."
        }
    }
}

fn set_draft_error(app: &mut App, resume_mode: ResumeMode, message: String) {
    if let Some(form) = app.access.form.as_mut() {
        form.confirming = false;
        form.error = Some(message.clone());
    } else {
        app.push_toast(ToastKind::Error, message);
    }
    app.input_mode = resume_mode.input_mode();
}

fn mark_undo(app: &mut App, undo_id: UndoId, status: EntryStatus) {
    if let Err(error) = app.undo.mark_applied(undo_id, status) {
        app.push_toast(
            ToastKind::Error,
            format!("Failed to retire Access undo entry: {error}"),
        );
    }
}

pub fn request_latest_undo(app: &mut App) {
    let Some(tenant) = app.active_tenant() else {
        return;
    };
    let undo_id = app
        .undo
        .latest_pending(&tenant.name, UndoExecutor::Access)
        .map(|summary| summary.id);
    let Some(undo_id) = undo_id else {
        app.push_toast(ToastKind::Info, "No Access undo for this tenant");
        return;
    };
    if tenant.theme == TenantTheme::Production {
        app.prod_confirm.pending = Some(PendingProdAction::Access(ProdAction::Undo(undo_id)));
        app.input_mode = InputMode::ProdConfirm;
    } else {
        execute_undo(app, undo_id, false);
    }
}

pub fn execute_undo(app: &mut App, undo_id: UndoId, confirmed_prod: bool) {
    let entry = match app.undo.load(undo_id) {
        Ok(entry) if entry.status == EntryStatus::Pending => entry,
        Ok(_) => {
            app.push_toast(ToastKind::Info, "Undo entry is no longer pending");
            return;
        }
        Err(error) => {
            app.push_toast(ToastKind::Error, format!("Undo failed: {error}"));
            return;
        }
    };
    if !entry
        .op
        .as_ref()
        .is_some_and(|op| op.executor() == UndoExecutor::Access)
    {
        app.push_toast(ToastKind::Info, "Undo entry is not an Access change");
        return;
    }
    let tenant = entry.tenant.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = apply_undo_entry(entry, confirmed_prod).await;
        let _ = tx.send(AppEvent::Access(crate::access::screen::Event::UndoResult {
            tenant,
            undo_id,
            result,
        }));
    });
}

async fn apply_undo_entry(
    entry: UndoEntry,
    confirmed_prod: bool,
) -> std::result::Result<Value, UndoFailure> {
    let expected = match entry.conflict_check {
        ConflictCheck::ContentEqualsAfter { body }
        | ConflictCheck::ContentEqualsBefore { body } => body,
        _ => {
            return Err(UndoFailure::Failed(
                "Access undo has no whole-document conflict snapshot".into(),
            ));
        }
    };
    let Some(UndoOp::AccessConfigReplace { tenant, body }) = entry.op else {
        return Err(UndoFailure::Failed(
            "undo entry is not an Access operation".into(),
        ));
    };
    let live = api::get_access(&tenant)
        .await
        .map_err(|error| UndoFailure::Failed(error.to_string()))?;
    undo_precondition(&live, &expected)?;
    api::put_access_confirmed(&tenant, body.clone(), confirmed_prod)
        .await
        .map_err(|error| match error {
            api::ConfirmedWriteError::NotWritten(error) => UndoFailure::Failed(error.to_string()),
            api::ConfirmedWriteError::AcceptedButUnconfirmed(message) => {
                UndoFailure::Failed(message)
            }
        })?;
    Ok(body)
}

pub(crate) fn undo_precondition(
    live: &Value,
    expected: &Value,
) -> std::result::Result<(), UndoFailure> {
    if live == expected {
        Ok(())
    } else {
        Err(UndoFailure::Conflict(
            "config/access changed since this edit; nothing was written".into(),
        ))
    }
}

pub fn apply_undo_result(
    app: &mut App,
    tenant: String,
    undo_id: UndoId,
    result: std::result::Result<Value, UndoFailure>,
) {
    match result {
        Ok(document) => {
            if let Err(error) = app.undo.mark_applied(undo_id, EntryStatus::AppliedSuccess) {
                app.push_toast(
                    ToastKind::Error,
                    format!("Undo applied but log update failed: {error}"),
                );
            }
            if let Ok(document) = Document::from_value(document) {
                app.access
                    .data
                    .insert(tenant, crate::access::state::LoadState::Loaded(document));
            }
            app.push_toast(ToastKind::Success, "Access change undone");
        }
        Err(UndoFailure::Conflict(message)) => {
            let _ = app.undo.mark_applied(undo_id, EntryStatus::AppliedConflict);
            app.push_toast(ToastKind::Warning, format!("Undo conflict: {message}"));
        }
        Err(UndoFailure::Failed(message)) => {
            let _ = app.undo.mark_applied(undo_id, EntryStatus::AppliedFailure);
            app.push_toast(ToastKind::Error, format!("Undo failed: {message}"));
        }
    }
}

pub fn execute_prod_action(app: &mut App, action: ProdAction) {
    match action {
        ProdAction::Write(request) => execute_write(app, *request, true),
        ProdAction::Undo(undo_id) => execute_undo(app, undo_id, true),
    }
}

pub fn resume_mode(_app: &App, action: &ProdAction) -> InputMode {
    match action {
        ProdAction::Write(request) => request.resume_mode.input_mode(),
        ProdAction::Undo(_) => InputMode::Normal,
    }
}

pub fn describe_prod_action(action: &ProdAction) -> Option<String> {
    match action {
        ProdAction::Write(_) => {
            Some("replace the complete config/access authorization document".into())
        }
        ProdAction::Undo(_) => {
            Some("restore a prior complete config/access authorization document".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use crate::access::spec::{digest, short_digest};
    use crate::access::state::{DeleteState, Document, RuleFormState};
    use crate::undo::{ConflictCheck, MemoryLog, UndoLog, UndoOp};

    use super::*;

    fn new_rule() -> RuleSpec {
        RuleSpec {
            pattern: "endpoint/new/*".into(),
            roles: "internal/role/new-reader".into(),
            methods: "read".into(),
            actions: None,
            custom_authz: None,
            exclude_patterns: None,
        }
    }

    fn create_form() -> RuleFormState {
        let document = Document::from_value(crate::access::six_rule_fixture()).unwrap();
        let mut form = RuleFormState::create("sandbox".into(), &document);
        form.pattern.set("endpoint/new/*");
        form.roles.set("internal/role/new-reader");
        form.methods.set("read");
        form
    }

    #[test]
    fn append_preserves_order_and_unknown_keys() {
        let before = crate::access::six_rule_fixture();
        let transformed = append(&before, new_rule()).unwrap();
        let after = transformed.document;

        assert_eq!(&rules(&after).unwrap()[..6], rules(&before).unwrap());
        assert_eq!(after["unknownTopLevelKey"], before["unknownTopLevelKey"]);
        assert_eq!(
            after["configs"][0]["unknownRuleKey"],
            before["configs"][0]["unknownRuleKey"]
        );
        assert!(after["configs"][6].get("actions").is_none());
    }

    #[test]
    fn replace_changes_only_supplied_keys() {
        let before = crate::access::six_rule_fixture();
        let after = replace_at(
            &before,
            0,
            RuleEdit {
                methods: Some("read,query,update".into()),
                ..RuleEdit::default()
            },
        )
        .unwrap()
        .document;

        assert_eq!(after["configs"][0]["methods"], json!("read,query,update"));
        assert!(after["configs"][0].get("actions").is_none());
        assert_eq!(
            after["configs"][0]["unknownRuleKey"],
            before["configs"][0]["unknownRuleKey"]
        );
        assert_eq!(&rules(&after).unwrap()[1..], &rules(&before).unwrap()[1..]);
    }

    #[test]
    fn removing_one_duplicate_removes_exactly_one_rule() {
        let before = crate::access::six_rule_fixture();
        let duplicate = before["configs"][4].clone();
        let after = remove_at(&before, &[4]).unwrap().document;

        assert_eq!(rules(&after).unwrap().len(), 5);
        assert_eq!(
            rules(&after)
                .unwrap()
                .iter()
                .filter(|rule| *rule == &duplicate)
                .count(),
            1
        );
        assert_eq!(&rules(&after).unwrap()[..4], &rules(&before).unwrap()[..4]);
    }

    #[test]
    fn removing_multiple_indices_does_not_shift_later_targets() {
        let before = crate::access::six_rule_fixture();
        let after = remove_at(&before, &[1, 4]).unwrap().document;

        assert_eq!(
            rules(&after).unwrap(),
            &vec![
                before["configs"][0].clone(),
                before["configs"][2].clone(),
                before["configs"][3].clone(),
                before["configs"][5].clone(),
            ]
        );
    }

    #[test]
    fn document_digest_changes_on_edit() {
        let before = crate::access::six_rule_fixture();
        let after = replace_at(
            &before,
            2,
            RuleEdit {
                methods: Some("read,query".into()),
                ..RuleEdit::default()
            },
        )
        .unwrap()
        .document;

        assert_ne!(digest(&before), digest(&after));
        assert_eq!(short_digest(&before).len(), 8);
    }

    #[test]
    fn exact_change_summaries_are_table_driven() {
        let before = crate::access::six_rule_fixture();
        let cases = [
            (
                "append",
                append(&before, new_rule()).unwrap(),
                6,
                false,
                true,
                6,
            ),
            (
                "remove non-duplicate",
                remove_at(&before, &[2]).unwrap(),
                2,
                true,
                false,
                5,
            ),
            (
                "remove first addressed duplicate",
                remove_at(&before, &[4]).unwrap(),
                4,
                true,
                false,
                5,
            ),
            (
                "remove second addressed duplicate",
                remove_at(&before, &[5]).unwrap(),
                5,
                true,
                false,
                5,
            ),
        ];

        for (name, transformed, index, has_before, has_after, unchanged) in cases {
            let summary = changes(
                &before,
                &transformed.document,
                ChangeBasis::Touched(&transformed.touched),
            );
            assert_eq!(summary.changed.len(), 1, "{name}: {summary:?}");
            assert_eq!(summary.changed[0].index, index, "{name}");
            assert_eq!(summary.changed[0].before.is_some(), has_before, "{name}");
            assert_eq!(summary.changed[0].after.is_some(), has_after, "{name}");
            assert_eq!(summary.unchanged, unchanged, "{name}");
            assert!(!summary.positions_approximate, "{name}");
        }
    }

    #[test]
    fn apply_changes_match_a_multiset_instead_of_pairing_positions() {
        let before = crate::access::six_rule_fixture();
        let mut after = before.clone();
        after["configs"].as_array_mut().unwrap().remove(1);
        after["configs"].as_array_mut().unwrap().push(json!({
            "pattern": "endpoint/new/*",
            "roles": "*",
            "methods": "read"
        }));

        let summary = changes(&before, &after, ChangeBasis::Multiset);
        assert_eq!(summary.unchanged, 5);
        assert_eq!(summary.changed.len(), 2);
        assert!(
            summary
                .changed
                .iter()
                .any(|change| change.before == Some(before["configs"][1].clone())
                    && change.after.is_none())
        );
        assert!(
            summary.changed.iter().any(|change| change.before.is_none()
                && change.after == Some(after["configs"][5].clone()))
        );
        assert!(summary.positions_approximate);
        assert_eq!(summary.touched.before(), &BTreeSet::from([1]));
        assert_eq!(summary.touched.after(), &BTreeSet::from([5]));
    }

    #[test]
    fn apply_positions_are_exact_when_the_source_has_no_duplicates() {
        let before = json!({
            "configs": [
                {"pattern": "a", "roles": "*", "methods": "read"},
                {"pattern": "b", "roles": "*", "methods": "read"}
            ]
        });
        let mut after = before.clone();
        after["configs"][1]["methods"] = json!("read,query");

        let summary = changes(&before, &after, ChangeBasis::Multiset);
        assert!(!summary.positions_approximate);
    }

    #[test]
    fn out_of_range_index_names_valid_range() {
        let error =
            replace_at(&crate::access::six_rule_fixture(), 6, RuleEdit::default()).unwrap_err();
        assert!(
            matches!(error, Error::Config(ref message) if message.contains("0..=5")),
            "{error}"
        );
    }

    #[test]
    fn write_undo_restores_the_prior_document_and_checks_the_result() {
        // Recording only a rule subtree, or omitting the optimistic post-write
        // snapshot, makes the whole-document assertions fail.
        let document = Document::from_value(crate::access::six_rule_fixture()).unwrap();
        let mut form = RuleFormState::edit("sandbox".into(), &document, &document.rows[1]);
        form.methods.set("read");
        let request = request_from_form(&form).unwrap();
        let mut undo = MemoryLog::new();
        let id = record_write_undo(&mut undo, &request).unwrap();
        let entry = undo.load(id).unwrap();

        assert!(matches!(
            entry.op,
            Some(UndoOp::AccessConfigReplace { ref tenant, ref body })
                if tenant == "sandbox" && body == &document.value
        ));
        assert!(matches!(
            entry.conflict_check,
            ConflictCheck::ContentEqualsAfter { ref body } if body == &request.after
        ));
    }

    #[test]
    fn create_form_omits_empty_optional_keys() {
        // Making create fields serialize their empty strings as Some makes
        // this add actions/customAuthz/excludePatterns keys to the new rule.
        let form = create_form();
        let Amendment::Add(rule) = form.amendment() else {
            panic!("create form returned a non-add amendment");
        };

        assert_eq!(rule.actions, None);
        assert_eq!(rule.custom_authz, None);
        assert_eq!(rule.exclude_patterns, None);
    }

    #[test]
    fn create_request_has_no_expected_rule_precondition() {
        // Reusing the edit request branch for FormKind::Create makes the new
        // append depend on an unrelated indexed rule and fails this assertion.
        let request = request_from_form(&create_form()).unwrap();

        assert_eq!(request.expected_rule, None);
        assert!(matches!(request.amendment, Amendment::Add(_)));
    }

    #[test]
    fn unchanged_edit_is_rejected_before_write_submission() {
        // Removing the changed.is_empty() rejection lets an untouched edit
        // create a backup and schedule a whole-document PUT.
        let document = Document::from_value(crate::access::six_rule_fixture()).unwrap();
        let form = RuleFormState::edit("sandbox".into(), &document, &document.rows[0]);

        assert!(
            matches!(request_from_form(&form), Err(Error::Config(message)) if message == "access rule is unchanged")
        );
    }

    #[test]
    fn touched_validation_warnings_are_returned_for_review() {
        // Passing None for known_roles or discarding Findings::warnings makes
        // at least one of the advisory checks disappear from the review.
        let mut form = create_form();
        form.methods.set("reed");
        form.custom_authz.input.set("false");
        form.known_roles = Some(crate::access::spec::RoleIndex::from_roles(
            std::iter::empty(),
        ));
        let request = request_from_form(&form).unwrap();

        assert!(
            request
                .warnings
                .iter()
                .any(|warning| warning.contains("absent from internal roles"))
        );
        assert!(
            request
                .warnings
                .iter()
                .any(|warning| warning.contains("unrecognised access method"))
        );
        assert!(
            request
                .warnings
                .iter()
                .any(|warning| warning.contains("customAuthz can only deny"))
        );

        let document = Document::from_value(crate::access::six_rule_fixture()).unwrap();
        let duplicate = &document.rows[4].summary;
        let mut duplicate_form = RuleFormState::create("sandbox".into(), &document);
        duplicate_form.pattern.set(&duplicate.pattern);
        duplicate_form.roles.set(&duplicate.roles);
        duplicate_form.methods.set(&duplicate.methods);
        duplicate_form.actions.input.set("*");
        let request = request_from_form(&duplicate_form).unwrap();
        assert!(
            request
                .warnings
                .iter()
                .any(|warning| warning.contains("byte-identical"))
        );
    }

    #[test]
    fn undo_precondition_compares_the_complete_document() {
        // Comparing only the selected rule or removing undo_precondition lets
        // the changed-methods row pass and permits a whole-document clobber.
        let expected = crate::access::six_rule_fixture();
        let mut changed = expected.clone();
        changed["configs"][2]["methods"] = json!("read,query");

        assert!(undo_precondition(&expected, &expected).is_ok());
        assert!(matches!(
            undo_precondition(&changed, &expected),
            Err(UndoFailure::Conflict(_))
        ));
    }

    #[test]
    fn write_results_choose_the_safe_undo_disposition() {
        // Changing either failure grouping, especially retiring an accepted
        // but unconfirmed write, makes the corresponding table row fail.
        let cases = [
            ("success", Ok(()), None),
            (
                "stale",
                Err(WriteFailure::Stale("stale".into())),
                Some(EntryStatus::AppliedFailure),
            ),
            (
                "not written",
                Err(WriteFailure::NotWritten("failed".into())),
                Some(EntryStatus::AppliedFailure),
            ),
            (
                "accepted but unconfirmed",
                Err(WriteFailure::AcceptedButUnconfirmed("unknown".into())),
                None,
            ),
        ];

        for (name, result, expected) in cases {
            assert_eq!(undo_disposition(&result), expected, "{name}");
        }
    }

    #[test]
    fn backup_filename_is_utc_and_writer_sets_mode_0600() {
        // Dropping the UTC timestamp/nonce or the explicit backup file mode
        // makes the naming or permission assertion fail.
        let now = Utc.with_ymd_and_hms(2026, 8, 11, 4, 5, 6).unwrap();
        let first_nonce = uuid::Uuid::from_u128(1);
        let second_nonce = uuid::Uuid::from_u128(2);
        let filename = backup_filename("sand/box", now, first_nonce);
        assert_eq!(
            filename,
            "access-sand_box-20260811T040506Z-00000000-0000-0000-0000-000000000001.json"
        );
        assert_ne!(filename, backup_filename("sand/box", now, second_nonce));

        let dir = std::env::temp_dir().join(format!("aic-access-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join(filename);
        let mut bytes =
            serde_json::to_vec_pretty(&json!({"_id": "access", "configs": []})).unwrap();
        bytes.push(b'\n');
        write_private_file(&path, &bytes, true).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved, json!({"_id": "access", "configs": []}));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn delete_request_is_unavailable_until_the_confirmation_step() {
        // Letting request_from_delete ignore DeleteState::confirmed makes the
        // first assertion fail and permits a key path to schedule a write early.
        let document = Document::from_value(crate::access::six_rule_fixture()).unwrap();
        let mut delete = DeleteState::new("sandbox".into(), &document, &document.rows[4]);

        assert!(request_from_delete(&delete).is_err());
        delete.confirm();
        let request = request_from_delete(&delete).unwrap();
        assert!(matches!(request.amendment, Amendment::Remove(ref indices) if indices == &[4]));
    }

    #[test]
    fn stale_document_digest_blocks_before_the_write_body_is_prepared() {
        // Removing check_digest from prepare_live_write makes this return an
        // amended document that the async path would send to the tenant.
        let document = Document::from_value(crate::access::six_rule_fixture()).unwrap();
        let mut form = RuleFormState::edit("sandbox".into(), &document, &document.rows[1]);
        form.methods.set("read");
        let request = request_from_form(&form).unwrap();
        let mut live = document.value.clone();
        live["configs"][0]["methods"] = json!("read");

        assert!(matches!(
            prepare_live_write(&request, &live),
            Err(WriteFailure::Stale(message)) if message.contains("nothing was written")
        ));
    }
}
