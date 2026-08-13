//! Background loads, guarded writes, and undo execution for the secret-mapping
//! TUI tab.

use serde_json::Value;

use crate::app::event::{AppEvent, ToastKind};
use crate::app::prod_confirm::PendingProdAction;
use crate::app::{App, InputMode};
use crate::secretmap::api;
use crate::secretmap::screen::Event;
use crate::secretmap::state::{LoadState, REALM, State, mapping_snapshot};
use crate::undo::{
    Capability, ConflictCheck, EntryStatus, Sensitivity, UndoEntry, UndoExecutor, UndoId, UndoOp,
};

#[derive(Debug)]
pub enum ProdAction {
    Replace(AliasReplacePlan),
    Delete(MappingDeletePlan),
    Undo(UndoId),
}

pub fn execute_prod_action(app: &mut App, action: ProdAction) {
    match action {
        ProdAction::Replace(plan) => execute_write_plan(app, plan, true),
        ProdAction::Delete(plan) => execute_remove_plan(app, plan, true),
        ProdAction::Undo(undo_id) => execute_undo(app, undo_id, true),
    }
}

pub fn resume_mode(_app: &App, _action: &ProdAction) -> InputMode {
    InputMode::Normal
}

pub fn describe_prod_action(_action: &ProdAction) -> Option<String> {
    None
}

#[derive(Debug)]
pub struct AliasReplacePlan {
    pub tenant: String,
    pub realm: String,
    pub secret_id: String,
    pub prior_alias: Option<String>,
    pub new_alias: String,
    pub snapshot: Value,
}

#[derive(Debug)]
pub struct MappingDeletePlan {
    pub tenant: String,
    pub realm: String,
    pub secret_id: String,
    pub prior_alias: String,
    pub snapshot: Value,
}

#[derive(Debug)]
pub struct WriteOutcome {
    pub mapping: api::Mapping,
    pub deleted: bool,
    pub success_message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteFailure {
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteDecision {
    Write,
    BlockedDrift,
    NothingToDo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoDecision {
    Delete,
    Set,
    BlockedDrift,
}

#[derive(Debug)]
pub enum UndoApplied {
    Restored(api::Mapping),
    Deleted { secret_id: String },
}

#[derive(Debug)]
pub struct UndoOutcome {
    pub description: String,
    pub secret_id: String,
    pub applied: UndoApplied,
}

#[derive(Debug)]
pub enum UndoFailure {
    Conflict(String),
    Failed(String),
}

pub fn load_list(app: &mut App, force: bool) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if !app.is_unlocked()
        || app.secretmap.refreshing.contains(&tenant)
        || (!force && app.secretmap.data.contains_key(&tenant))
    {
        return;
    }

    app.secretmap
        .data
        .insert(tenant.clone(), LoadState::Loading);
    app.secretmap.refreshing.insert(tenant.clone());

    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        match api::list_mappings(&tenant, REALM).await {
            Ok(mappings) => {
                let _ = tx.send(AppEvent::Secretmap(Event::ListLoaded { tenant, mappings }));
            }
            Err(error) => {
                let _ = tx.send(AppEvent::Secretmap(Event::LoadFailed {
                    tenant,
                    esv_secrets: false,
                    message: error.to_string(),
                }));
            }
        }
    });
}

pub fn load_esv_secrets(app: &mut App, force: bool) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if !app.is_unlocked() {
        return;
    }
    if force {
        app.secretmap.invalidate_esv_cache(&tenant);
    }
    if app.secretmap.esv_secret_loading.contains(&tenant)
        || (!force && app.secretmap.esv_secret_ids.contains_key(&tenant))
    {
        return;
    }

    app.secretmap.esv_secret_failed.remove(&tenant);
    app.secretmap.esv_secret_loading.insert(tenant.clone());

    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        match crate::esv::api::list_secrets(&tenant).await {
            Ok(secrets) => {
                let mut ids: Vec<String> = secrets
                    .iter()
                    .filter_map(|secret| secret.get("_id").and_then(Value::as_str))
                    .map(ToString::to_string)
                    .collect();
                ids.sort();
                ids.dedup();
                let _ = tx.send(AppEvent::Secretmap(Event::EsvSecretsLoaded { tenant, ids }));
            }
            Err(error) => {
                let _ = tx.send(AppEvent::Secretmap(Event::LoadFailed {
                    tenant,
                    esv_secrets: true,
                    message: error.to_string(),
                }));
            }
        }
    });
}

pub fn load_valid_secret_ids(app: &mut App, force: bool) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if !app.is_unlocked() {
        return;
    }
    if force {
        app.secretmap.invalidate_valid_label_cache(&tenant);
    }
    if app.secretmap.valid_secret_loading.contains(&tenant)
        || (!force && app.secretmap.valid_secret_ids.contains_key(&tenant))
    {
        return;
    }

    app.secretmap.valid_secret_failed.remove(&tenant);
    app.secretmap.valid_secret_loading.insert(tenant.clone());

    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        match api::valid_secret_ids(&tenant, REALM).await {
            Ok(ids) => {
                let _ = tx.send(AppEvent::Secretmap(Event::ValidLabelsLoaded {
                    tenant,
                    ids,
                }));
            }
            Err(error) => {
                let _ = tx.send(AppEvent::Secretmap(Event::ValidLabelsFailed {
                    tenant,
                    message: error.to_string(),
                }));
            }
        }
    });
}

pub fn submit_alias_replace(app: &mut App, plan: AliasReplacePlan) {
    if !app
        .active_tenant()
        .is_some_and(|tenant| tenant.allows_secret_mappings())
    {
        app.push_toast(
            ToastKind::Warning,
            "Secret mappings are only available on sandbox/development tenants",
        );
        return;
    }
    if app.active_tenant().is_some_and(|tenant| tenant.is_prod()) {
        app.prod_confirm.pending = Some(PendingProdAction::Secretmap(ProdAction::Replace(plan)));
        app.input_mode = InputMode::ProdConfirm;
        return;
    }
    execute_write_plan(app, plan, false);
}

pub fn execute_write_plan(app: &mut App, plan: AliasReplacePlan, confirmed_prod: bool) {
    let undo_id = match record_replace_undo(app, &plan) {
        Ok(undo_id) => undo_id,
        Err(error) => {
            app.push_toast(
                ToastKind::Error,
                format!("Mapping save cancelled: failed to record undo: {error}"),
            );
            return;
        }
    };

    let optimistic = api::Mapping {
        secret_id: plan.secret_id.clone(),
        alias: Some(plan.new_alias.clone()),
    };
    set_cached_mapping(app, &plan.tenant, optimistic.clone());
    app.secretmap
        .in_flight_writes
        .insert((plan.tenant.clone(), plan.secret_id.clone()));
    app.secretmap
        .failed_writes
        .remove(&(plan.tenant.clone(), plan.secret_id.clone()));
    app.secretmap.editing = None;
    app.input_mode = InputMode::Normal;

    let event_tenant = plan.tenant.clone();
    let event_secret_id = plan.secret_id.clone();
    let event_snapshot = plan.snapshot.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = replace_alias_request(plan, confirmed_prod)
            .await
            .map_err(|error| WriteFailure::Failed(error.to_string()));
        let _ = tx.send(AppEvent::Secretmap(Event::WriteResult {
            tenant: event_tenant,
            secret_id: event_secret_id,
            undo_id,
            snapshot: event_snapshot,
            result,
        }));
    });
}

pub fn submit_remove(app: &mut App, plan: MappingDeletePlan) {
    if !app
        .active_tenant()
        .is_some_and(|tenant| tenant.allows_secret_mappings())
    {
        app.push_toast(
            ToastKind::Warning,
            "Secret mappings are only available on sandbox/development tenants",
        );
        return;
    }
    if app.active_tenant().is_some_and(|tenant| tenant.is_prod()) {
        app.prod_confirm.pending = Some(PendingProdAction::Secretmap(ProdAction::Delete(plan)));
        app.input_mode = InputMode::ProdConfirm;
        return;
    }
    execute_remove_plan(app, plan, false);
}

pub fn execute_remove_plan(app: &mut App, plan: MappingDeletePlan, confirmed_prod: bool) {
    let undo_id = match record_remove_undo(app, &plan) {
        Ok(undo_id) => undo_id,
        Err(error) => {
            app.push_toast(
                ToastKind::Error,
                format!("Mapping remove cancelled: failed to record undo: {error}"),
            );
            return;
        }
    };

    remove_cached_mapping(app, &plan.tenant, &plan.secret_id);
    app.secretmap
        .in_flight_writes
        .insert((plan.tenant.clone(), plan.secret_id.clone()));
    app.secretmap
        .failed_writes
        .remove(&(plan.tenant.clone(), plan.secret_id.clone()));
    app.secretmap.pending_delete = None;
    app.input_mode = InputMode::Normal;

    let event_tenant = plan.tenant.clone();
    let event_secret_id = plan.secret_id.clone();
    let event_snapshot = plan.snapshot.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = remove_mapping_request(plan, confirmed_prod)
            .await
            .map_err(|error| WriteFailure::Failed(error.to_string()));
        let _ = tx.send(AppEvent::Secretmap(Event::WriteResult {
            tenant: event_tenant,
            secret_id: event_secret_id,
            undo_id,
            snapshot: event_snapshot,
            result,
        }));
    });
}

fn record_replace_undo(app: &mut App, plan: &AliasReplacePlan) -> crate::Result<UndoId> {
    let after = mapping_snapshot(&plan.secret_id, Some(&plan.new_alias));
    let description = if plan.prior_alias.is_some() {
        format!("Restore secret mapping {}", plan.secret_id)
    } else {
        format!("Delete created secret mapping {}", plan.secret_id)
    };
    app.undo.record(UndoEntry::pending(
        plan.tenant.clone(),
        "secretmap",
        description,
        Sensitivity::PublicMetadata,
        Capability::Undoable,
        Some(UndoOp::SecretMappingReplace {
            tenant: plan.tenant.clone(),
            realm: plan.realm.clone(),
            secret_id: plan.secret_id.clone(),
            prior_alias: plan.prior_alias.clone(),
        }),
        ConflictCheck::ContentEqualsAfter { body: after },
    ))
}

fn record_remove_undo(app: &mut App, plan: &MappingDeletePlan) -> crate::Result<UndoId> {
    let after = mapping_snapshot(&plan.secret_id, None);
    app.undo.record(UndoEntry::pending(
        plan.tenant.clone(),
        "secretmap",
        format!("Re-create removed secret mapping {}", plan.secret_id),
        Sensitivity::PublicMetadata,
        Capability::Undoable,
        Some(UndoOp::SecretMappingReplace {
            tenant: plan.tenant.clone(),
            realm: plan.realm.clone(),
            secret_id: plan.secret_id.clone(),
            prior_alias: Some(plan.prior_alias.clone()),
        }),
        ConflictCheck::ContentEqualsAfter { body: after },
    ))
}

async fn replace_alias_request(
    plan: AliasReplacePlan,
    confirmed_prod: bool,
) -> crate::Result<WriteOutcome> {
    let remote = match api::read_mapping(&plan.tenant, &plan.realm, &plan.secret_id).await {
        Ok(remote) => remote,
        Err(crate::Error::Api { status: 404, .. }) if plan.prior_alias.is_none() => {
            mapping_snapshot(&plan.secret_id, None)
        }
        Err(error) => return Err(error),
    };
    match decide_write(&remote, &plan.snapshot, &plan.new_alias, false) {
        WriteDecision::Write => {}
        WriteDecision::BlockedDrift => {
            return Err(crate::Error::Config(
                "remote changed since you opened the editor; refresh before editing this mapping"
                    .into(),
            ));
        }
        WriteDecision::NothingToDo => {
            return Err(crate::Error::Config(
                "selected alias is already mapped".into(),
            ));
        }
    }

    let saved = api::set_mapping(
        &plan.tenant,
        &plan.realm,
        &plan.secret_id,
        &plan.new_alias,
        confirmed_prod,
    )
    .await?;
    let mut mapping = api::parse_mapping(&saved);
    if mapping.secret_id.is_empty() {
        mapping.secret_id = plan.secret_id.clone();
    }
    if mapping.alias.is_none() {
        mapping.alias = Some(plan.new_alias.clone());
    }
    let success_message = if plan.prior_alias.is_some() {
        "Updated secret mapping. Press ^Z to undo.".to_string()
    } else {
        "Created secret mapping. Press ^Z to undo.".to_string()
    };
    Ok(WriteOutcome {
        mapping,
        deleted: false,
        success_message,
    })
}

async fn remove_mapping_request(
    plan: MappingDeletePlan,
    confirmed_prod: bool,
) -> crate::Result<WriteOutcome> {
    let remote = match api::read_mapping(&plan.tenant, &plan.realm, &plan.secret_id).await {
        Ok(remote) => Some(remote),
        Err(crate::Error::Api { status: 404, .. }) => None,
        Err(error) => return Err(error),
    };
    match decide_delete(remote.as_ref(), &plan.snapshot, false) {
        WriteDecision::Write => {}
        WriteDecision::BlockedDrift => {
            return Err(crate::Error::Config(
                "remote changed since you opened the delete confirmation; refresh before removing this mapping"
                    .into(),
            ));
        }
        WriteDecision::NothingToDo => {
            return Err(crate::Error::Config("mapping is already unmapped".into()));
        }
    }

    api::delete_mapping_confirmed(&plan.tenant, &plan.realm, &plan.secret_id, confirmed_prod)
        .await?;
    Ok(WriteOutcome {
        mapping: api::Mapping {
            secret_id: plan.secret_id,
            alias: None,
        },
        deleted: true,
        success_message: "Removed secret mapping. Press ^Z to undo.".to_string(),
    })
}

pub fn decide_write(
    remote: &Value,
    snapshot: &Value,
    new_alias: &str,
    force: bool,
) -> WriteDecision {
    if api::parse_mapping(snapshot).alias.as_deref() == Some(new_alias) {
        return WriteDecision::NothingToDo;
    }
    if force || api::content_equal(remote, snapshot) {
        WriteDecision::Write
    } else {
        WriteDecision::BlockedDrift
    }
}

pub fn decide_delete(remote: Option<&Value>, snapshot: &Value, force: bool) -> WriteDecision {
    if api::parse_mapping(snapshot).alias.is_none() {
        return WriteDecision::NothingToDo;
    }
    if force || remote_content_equal(remote, snapshot) {
        WriteDecision::Write
    } else {
        WriteDecision::BlockedDrift
    }
}

pub fn decide_undo(
    remote: Option<&Value>,
    expected_current: &Value,
    prior_alias: Option<&str>,
) -> UndoDecision {
    if remote_content_equal(remote, expected_current) {
        if prior_alias.is_some() {
            UndoDecision::Set
        } else {
            UndoDecision::Delete
        }
    } else {
        UndoDecision::BlockedDrift
    }
}

fn remote_content_equal(remote: Option<&Value>, expected_current: &Value) -> bool {
    let remote_unmapped;
    let remote = match remote {
        Some(remote) => remote,
        None => {
            let secret_id = api::parse_mapping(expected_current).secret_id;
            remote_unmapped = mapping_snapshot(&secret_id, None);
            &remote_unmapped
        }
    };
    api::content_equal(remote, expected_current)
}

pub fn apply_write_result(
    app: &mut App,
    tenant: String,
    secret_id: String,
    undo_id: UndoId,
    snapshot: Value,
    result: Result<WriteOutcome, WriteFailure>,
) {
    app.secretmap
        .in_flight_writes
        .remove(&(tenant.clone(), secret_id.clone()));
    match result {
        Ok(WriteOutcome {
            mapping,
            deleted,
            success_message,
        }) => {
            if deleted {
                remove_cached_mapping(app, &tenant, &mapping.secret_id);
            } else {
                set_cached_mapping(app, &tenant, mapping);
            }
            app.secretmap
                .failed_writes
                .remove(&(tenant.clone(), secret_id));
            app.push_toast(ToastKind::Success, success_message);
        }
        Err(failure) => {
            let snapshot_mapping = api::parse_mapping(&snapshot);
            if snapshot_mapping.alias.is_some() {
                set_cached_mapping(app, &tenant, snapshot_mapping);
            } else {
                remove_cached_mapping(app, &tenant, &secret_id);
            }
            app.secretmap
                .failed_writes
                .insert((tenant.clone(), secret_id.clone()));
            if let Err(mark_error) = app.undo.mark_applied(undo_id, EntryStatus::Expired) {
                app.push_toast(
                    ToastKind::Error,
                    format!("Failed to expire undo for failed mapping save: {mark_error}"),
                );
            }
            app.push_toast(
                ToastKind::Error,
                write_failure_message(&secret_id, &failure),
            );
        }
    }
}

fn write_failure_message(secret_id: &str, failure: &WriteFailure) -> String {
    match failure {
        WriteFailure::Failed(error) => format!("Secret mapping save failed: {secret_id}: {error}"),
    }
}

pub fn request_latest_undo(app: &mut App) {
    let Some(tenant) = app.active_tenant() else {
        return;
    };
    let tenant_name = tenant.name.clone();
    if !tenant.allows_secret_mappings() {
        app.push_toast(
            ToastKind::Warning,
            "Secret-mapping undo is only available on sandbox/development tenants",
        );
        return;
    }
    let Some(undo_id) = latest_pending_secretmap_undo(app, &tenant_name) else {
        app.push_toast(ToastKind::Info, "No secret-mapping undo for this tenant");
        return;
    };

    execute_undo(app, undo_id, false);
}

fn latest_pending_secretmap_undo(app: &App, tenant: &str) -> Option<UndoId> {
    app.undo
        .latest_pending(tenant, UndoExecutor::SecretMapping)
        .map(|summary| summary.id)
}

pub fn execute_undo(app: &mut App, undo_id: UndoId, confirmed_prod: bool) {
    let entry = match app.undo.load(undo_id) {
        Ok(entry) => entry,
        Err(error) => {
            app.push_toast(ToastKind::Error, format!("Undo failed: {error}"));
            return;
        }
    };
    if entry.status != EntryStatus::Pending {
        app.push_toast(ToastKind::Info, "Undo entry is no longer pending");
        return;
    }
    if entry.op.is_none() || entry.capability == Capability::Irreversible {
        app.push_toast(ToastKind::Warning, "This change cannot be undone");
        return;
    }
    if !entry
        .op
        .as_ref()
        .is_some_and(|op| op.executor() == UndoExecutor::SecretMapping)
    {
        app.push_toast(ToastKind::Info, "Undo entry is not a secret-mapping change");
        return;
    }
    if !secret_mappings_allowed_for(app, &entry.tenant) {
        app.push_toast(
            ToastKind::Warning,
            "Secret-mapping undo is only available on sandbox/development tenants",
        );
        return;
    }

    let event_tenant = entry.tenant.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = apply_undo_entry(entry, confirmed_prod).await;
        let _ = tx.send(AppEvent::Secretmap(Event::UndoResult {
            undo_id,
            tenant: event_tenant,
            result,
        }));
    });
}

fn secret_mappings_allowed_for(app: &App, tenant_name: &str) -> bool {
    app.tenants
        .iter()
        .find(|tenant| tenant.name == tenant_name)
        .is_some_and(|tenant| tenant.allows_secret_mappings())
}

async fn apply_undo_entry(
    entry: UndoEntry,
    confirmed_prod: bool,
) -> Result<UndoOutcome, UndoFailure> {
    let op = entry
        .op
        .clone()
        .ok_or_else(|| UndoFailure::Failed("undo entry has no operation".into()))?;
    let UndoOp::SecretMappingReplace {
        tenant,
        realm,
        secret_id,
        prior_alias,
    } = op
    else {
        return Err(UndoFailure::Failed(
            "undo entry is not a secret-mapping operation".into(),
        ));
    };

    let expected_current = match entry.conflict_check {
        ConflictCheck::ContentEqualsAfter { body }
        | ConflictCheck::ContentEqualsBefore { body } => body,
        _ => {
            return Err(UndoFailure::Failed(
                "secret-mapping undo has no content snapshot".into(),
            ));
        }
    };

    let remote = match api::read_mapping(&tenant, &realm, &secret_id).await {
        Ok(remote) => Some(remote),
        Err(crate::Error::Api { status: 404, .. }) => None,
        Err(error) => {
            return Err(UndoFailure::Failed(format!(
                "conflict check failed: {error}"
            )));
        }
    };
    match decide_undo(remote.as_ref(), &expected_current, prior_alias.as_deref()) {
        UndoDecision::Set => {}
        UndoDecision::Delete => {
            api::delete_mapping_confirmed(&tenant, &realm, &secret_id, confirmed_prod)
                .await
                .map_err(|error| UndoFailure::Failed(error.to_string()))?;
            return Ok(UndoOutcome {
                description: entry.description,
                secret_id: secret_id.clone(),
                applied: UndoApplied::Deleted { secret_id },
            });
        }
        UndoDecision::BlockedDrift => {
            return Err(UndoFailure::Conflict(
                "remote mapping changed since the original write".into(),
            ));
        }
    }

    let prior_alias_value = prior_alias
        .as_deref()
        .ok_or_else(|| UndoFailure::Failed("undo entry has no prior alias".into()))?;
    let saved = api::set_mapping(
        &tenant,
        &realm,
        &secret_id,
        prior_alias_value,
        confirmed_prod,
    )
    .await
    .map_err(|error| UndoFailure::Failed(error.to_string()))?;
    let mut mapping = api::parse_mapping(&saved);
    if mapping.secret_id.is_empty() {
        mapping.secret_id = secret_id.clone();
    }
    if mapping.alias.is_none() {
        mapping.alias = Some(prior_alias_value.to_string());
    }
    Ok(UndoOutcome {
        description: entry.description,
        secret_id,
        applied: UndoApplied::Restored(mapping),
    })
}

pub fn apply_undo_result(
    app: &mut App,
    undo_id: UndoId,
    tenant: String,
    result: Result<UndoOutcome, UndoFailure>,
) {
    match result {
        Ok(UndoOutcome {
            description,
            secret_id,
            applied,
        }) => {
            if let Err(error) = app.undo.mark_applied(undo_id, EntryStatus::AppliedSuccess) {
                app.push_toast(
                    ToastKind::Error,
                    format!("Undo applied but log update failed: {error}"),
                );
            }
            match applied {
                UndoApplied::Restored(mapping) => {
                    set_cached_mapping(app, &tenant, mapping);
                    app.secretmap
                        .failed_writes
                        .remove(&(tenant.clone(), secret_id));
                    app.push_toast(ToastKind::Success, format!("Undone: {description}"));
                }
                UndoApplied::Deleted { secret_id } => {
                    remove_cached_mapping(app, &tenant, &secret_id);
                    app.secretmap
                        .failed_writes
                        .remove(&(tenant.clone(), secret_id));
                    app.push_toast(ToastKind::Success, format!("Undone: {description}"));
                }
            }
        }
        Err(UndoFailure::Conflict(message)) => {
            if let Err(error) = app.undo.mark_applied(undo_id, EntryStatus::AppliedConflict) {
                app.push_toast(
                    ToastKind::Error,
                    format!("Undo conflict log update failed: {error}"),
                );
            }
            app.push_toast(ToastKind::Warning, format!("Undo conflict: {message}"));
        }
        Err(UndoFailure::Failed(message)) => {
            if let Err(error) = app.undo.mark_applied(undo_id, EntryStatus::AppliedFailure) {
                app.push_toast(
                    ToastKind::Error,
                    format!("Undo failure log update failed: {error}"),
                );
            }
            app.push_toast(ToastKind::Error, format!("Undo failed: {message}"));
        }
    }
}

pub(crate) fn set_cached_mapping(app: &mut App, tenant: &str, mapping: api::Mapping) {
    set_cached_mapping_in_state(&mut app.secretmap, tenant, mapping);
}

pub(crate) fn remove_cached_mapping(app: &mut App, tenant: &str, secret_id: &str) {
    let mut remaining = None;
    if let Some(LoadState::Loaded(mappings)) = app.secretmap.data.get_mut(tenant) {
        mappings.retain(|mapping| mapping.secret_id != secret_id);
        remaining = Some(mappings.len());
    }
    if app
        .active_tenant()
        .is_some_and(|active| active.name == tenant)
    {
        if let Some(n) = remaining {
            app.secretmap.clamp_selection(n);
        }
    }
}

fn set_cached_mapping_in_state(state: &mut State, tenant: &str, mapping: api::Mapping) {
    let Some(LoadState::Loaded(mappings)) = state.data.get_mut(tenant) else {
        return;
    };
    if let Some(slot) = mappings
        .iter_mut()
        .find(|candidate| candidate.secret_id == mapping.secret_id)
    {
        *slot = mapping;
    } else {
        mappings.push(mapping);
        mappings.sort_by(|a, b| a.secret_id.cmp(&b.secret_id));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn decide_write_allows_when_remote_matches_snapshot() {
        let snapshot = json!({
            "secretId": "am.example.secret",
            "aliases": ["esv-old"]
        });
        let remote = json!({
            "_rev": "one",
            "secretId": "am.example.secret",
            "aliases": ["esv-old"]
        });

        assert_eq!(
            decide_write(&remote, &snapshot, "esv-new", false),
            WriteDecision::Write
        );
    }

    #[test]
    fn decide_write_blocks_remote_drift_without_force() {
        let snapshot = json!({
            "secretId": "am.example.secret",
            "aliases": ["esv-old"]
        });
        let remote = json!({
            "secretId": "am.example.secret",
            "aliases": ["esv-other"]
        });

        assert_eq!(
            decide_write(&remote, &snapshot, "esv-new", false),
            WriteDecision::BlockedDrift
        );
        assert_eq!(
            decide_write(&remote, &snapshot, "esv-new", true),
            WriteDecision::Write
        );
    }

    #[test]
    fn decide_write_detects_same_alias() {
        let snapshot = json!({
            "secretId": "am.example.secret",
            "aliases": ["esv-same"]
        });
        let remote = snapshot.clone();

        assert_eq!(
            decide_write(&remote, &snapshot, "esv-same", false),
            WriteDecision::NothingToDo
        );
    }

    #[test]
    fn decide_undo_restores_only_when_current_matches_written_value() {
        let expected = json!({
            "secretId": "am.example.secret",
            "aliases": ["esv-new"]
        });
        let remote = json!({
            "_rev": "two",
            "secretId": "am.example.secret",
            "aliases": ["esv-new"]
        });

        assert_eq!(
            decide_undo(Some(&remote), &expected, Some("esv-old")),
            UndoDecision::Set
        );
    }

    #[test]
    fn decide_undo_deletes_created_mapping_when_prior_alias_is_unset() {
        let expected = json!({
            "secretId": "am.example.secret",
            "aliases": ["esv-new"]
        });
        let remote = json!({
            "_rev": "two",
            "secretId": "am.example.secret",
            "aliases": ["esv-new"]
        });

        assert_eq!(
            decide_undo(Some(&remote), &expected, None),
            UndoDecision::Delete
        );
    }

    #[test]
    fn decide_undo_treats_absent_remote_as_unmapped_for_delete_forward() {
        let expected = json!({
            "secretId": "am.example.secret",
            "aliases": []
        });

        assert_eq!(
            decide_undo(None, &expected, Some("esv-old")),
            UndoDecision::Set
        );
    }

    #[test]
    fn decide_undo_blocks_drift_for_create_and_remove_undo() {
        let expected = json!({
            "secretId": "am.example.secret",
            "aliases": ["esv-new"]
        });
        let drifted = json!({
            "secretId": "am.example.secret",
            "aliases": ["esv-someone-else"]
        });

        assert_eq!(
            decide_undo(Some(&drifted), &expected, Some("esv-old")),
            UndoDecision::BlockedDrift
        );
        assert_eq!(
            decide_undo(None, &expected, None),
            UndoDecision::BlockedDrift
        );

        let expected_unmapped = json!({
            "secretId": "am.example.secret",
            "aliases": []
        });
        assert_eq!(
            decide_undo(Some(&drifted), &expected_unmapped, Some("esv-old")),
            UndoDecision::BlockedDrift
        );
    }

    #[test]
    fn decide_delete_allows_only_when_remote_matches_snapshot() {
        let snapshot = json!({
            "secretId": "am.example.secret",
            "aliases": ["esv-old"]
        });
        let remote = json!({
            "_rev": "two",
            "secretId": "am.example.secret",
            "aliases": ["esv-old"]
        });
        let drifted = json!({
            "secretId": "am.example.secret",
            "aliases": ["esv-other"]
        });

        assert_eq!(
            decide_delete(Some(&remote), &snapshot, false),
            WriteDecision::Write
        );
        assert_eq!(
            decide_delete(Some(&drifted), &snapshot, false),
            WriteDecision::BlockedDrift
        );
        assert_eq!(
            decide_delete(None, &snapshot, false),
            WriteDecision::BlockedDrift
        );
    }

    #[test]
    fn write_failure_message_reports_the_error() {
        let message = write_failure_message(
            "am.example.secret",
            &WriteFailure::Failed("AIC API error: 400 nope".into()),
        );

        assert!(message.contains("am.example.secret"));
        assert!(message.contains("nope"));
    }
}
