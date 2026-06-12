//! Background operations for ESV variables: refresh, save, delete, restart,
//! and undo execution plus their event-result handlers.

use std::collections::HashSet;
use std::time::Instant;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;

use crate::app::{App, InputMode};
use crate::config::tenant::TenantTheme;
use crate::esv::api::StartupStatus;
use crate::esv::screen::{Event, Mode};
use crate::esv::state::{
    ApplyState, DELETE_TOMBSTONE_TTL, DeleteOutcome, DeletePlan, DeleteTombstone, LoadState,
    RECENT_WRITE_TTL, RefreshOutcome, SaveOutcome, SavePlan, UndoApplied, UndoFailure, UndoOutcome,
    can_request_restart, id_of, is_applying, pending_count, queued_count,
};
use crate::event::{AppEvent, ToastKind};
use crate::screens::prod_confirm::PendingProdAction;
use crate::undo::{Capability, ConflictCheck, EntryStatus, Sensitivity, UndoEntry, UndoId, UndoOp};

struct SaveRequest {
    tenant_name: String,
    id: String,
    description: String,
    expr_type: String,
    value_b64: String,
    original: Option<serde_json::Value>,
}

struct DeleteRequest {
    tenant_name: String,
    id: String,
    original: serde_json::Value,
}

/// Kick off a background ESV fetch for the active tenant.
///
/// - `force = false`: only fetches when there's no cached entry yet
///   (initial load on startup / tenant switch).
/// - `force = true`: always fetches, even if a `Loaded` entry exists.
///   The stale data stays visible until the new fetch completes; failed
///   refetches don't clobber the cached value (see the nested ESV list
///   handler in `app::handle_event`).
///
/// A no-op when (a) the app is locked (still on the unlock screen — the
/// agent would return `Locked` and we'd just surface noise), (b) there's
/// no active tenant, or (c) a fetch for this tenant is already in flight.
/// Refetches keep the previous list and apply-state visible until both
/// tenant calls return through the event loop.
pub fn refresh(app: &mut App, force: bool) {
    let Some(name) = app.active_tenant().map(|t| t.name.clone()) else {
        return;
    };
    refresh_tenant(app, &name, force);
}

/// Like [`refresh`] but for a specific tenant by name — used by async
/// completion handlers so a result that lands after the user switched
/// tenants still refreshes the tenant it actually mutated, not whichever
/// one happens to be active now.
pub fn refresh_tenant(app: &mut App, name: &str, force: bool) {
    if !app.is_unlocked() {
        return;
    }
    let name = name.to_string();
    if app.esv.refreshing.contains(&name) {
        return;
    }
    if !force && app.esv.list.data.contains_key(&name) {
        return;
    }

    // Only show the Loading spinner when there's nothing cached yet;
    // refetches keep the previous Loaded entry visible.
    if !app.esv.list.data.contains_key(&name) {
        app.esv.list.data.insert(name.clone(), LoadState::Loading);
    }
    app.esv.refreshing.insert(name.clone());
    app.esv.last_poll = Instant::now();

    let tx = app.events.tx.clone();
    let tenant_name = name.clone();
    tokio::spawn(async move {
        let (variables, pending_variables, secrets, pending_secrets, startup) = tokio::join!(
            crate::esv::api::list_variables(&tenant_name),
            crate::esv::api::list_pending_variables(&tenant_name),
            crate::esv::api::list_secrets(&tenant_name),
            crate::esv::api::list_pending_secrets(&tenant_name),
            crate::esv::api::startup_status(&tenant_name),
        );
        let outcome = RefreshOutcome {
            variables: variables.map_err(|e| e.to_string()),
            pending_variables: pending_variables.map_err(|e| e.to_string()),
            secrets: secrets.map_err(|e| e.to_string()),
            pending_secrets: pending_secrets.map_err(|e| e.to_string()),
            startup: startup.map_err(|e| e.to_string()),
        };
        let _ = tx.send(AppEvent::Esv(Event::Listed {
            tenant: name,
            outcome,
        }));
    });
}

/// Open the restart-confirm popup if there's anything to apply, the
/// background saves have caught up, and a restart isn't already in
/// flight. Each negative case gets its own info toast so the user can
/// see why the keystroke was a no-op.
pub fn request_restart(app: &mut App) {
    let Some(tenant_name) = app.active_tenant().map(|t| t.name.clone()) else {
        return;
    };
    if is_applying(app, &tenant_name) {
        app.push_toast(crate::event::ToastKind::Info, "Restart already in progress");
        return;
    }
    if queued_count(app, &tenant_name) > 0 {
        // Purple banner already tells the user a save is in flight;
        // swallow ^S silently rather than stacking a contradicting toast.
        return;
    }
    if !can_request_restart(app, &tenant_name) {
        app.push_toast(crate::event::ToastKind::Info, "No pending changes to apply");
        return;
    }
    app.input_mode = InputMode::Esv(Mode::RestartConfirm);
}

pub fn request_delete(app: &mut App) {
    let Some(plan) = build_delete_plan(app) else {
        return;
    };
    app.esv.pending_delete = Some(plan);
    app.input_mode = InputMode::Esv(Mode::DeleteConfirm);
}

fn build_delete_plan(app: &mut App) -> Option<DeletePlan> {
    let tenant = app.active_tenant()?;
    let tenant_name = tenant.name.clone();
    let matches = app.esv.matches(Some(&tenant_name));
    let m = matches.get(app.esv.list.selected)?;
    if m.deleted {
        app.push_toast(
            ToastKind::Info,
            "Variable is already deleted; press ^Z to undo",
        );
        return None;
    }
    if app
        .esv
        .in_flight_writes
        .contains(&(tenant_name.clone(), m.id.clone()))
    {
        app.push_toast(
            ToastKind::Info,
            format!("Write already in progress: {}", m.id),
        );
        return None;
    }
    let Some(LoadState::Loaded(items)) = app.esv.list.data.get(&tenant_name) else {
        return None;
    };
    let original = items.get(m.idx?).cloned()?;
    Some(DeletePlan {
        tenant_name,
        id: m.id.clone(),
        original,
    })
}

pub(crate) fn trigger_restart(app: &mut App) {
    let Some(tenant) = app.active_tenant() else {
        return;
    };
    let tenant_name = tenant.name.clone();
    if tenant.theme == TenantTheme::Production {
        app.prod_confirm.pending = Some(PendingProdAction::EsvRestart { tenant_name });
        app.input_mode = InputMode::ProdConfirm;
        return;
    }
    trigger_restart_confirmed(app, tenant_name, false);
}

pub fn trigger_restart_confirmed(app: &mut App, tenant_name: String, confirmed_prod: bool) {
    // Flip the banner to its "applying" state immediately so the user
    // sees their click registered. It stays there until `/environment/startup`
    // and `/environment/variables` together prove a new state.
    let pending = pending_count(app, &tenant_name);
    set_apply_state(app, &tenant_name, ApplyState::Restarting(pending));
    app.input_mode = InputMode::Normal;
    app.push_toast(
        crate::event::ToastKind::Info,
        "Restart triggered — runtime will pick up changes in a few minutes",
    );
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::esv::api::trigger_restart(&tenant_name, confirmed_prod)
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::Esv(Event::RestartResult {
            tenant: tenant_name,
            result,
        }));
    });
}

/// Apply the async restart-trigger result. Success → toast; on error
/// surface the message and immediately roll back the "applying" banner
/// state — there's no in-flight restart to wait for.
pub fn apply_restart_result(
    app: &mut App,
    tenant: String,
    result: Result<serde_json::Value, String>,
) {
    match result {
        Ok(_) => {}
        Err(e) => {
            refresh_apply_state_from_cache(app, &tenant);
            app.push_toast(
                crate::event::ToastKind::Error,
                format!("Restart failed: {e}"),
            );
        }
    }
}

/// Apply a completed ESV list event to the tab.
pub fn apply_refresh(app: &mut App, tenant: String, outcome: RefreshOutcome) {
    let is_active = app.active_tenant().is_some_and(|t| t.name == tenant);
    app.esv.refreshing.remove(&tenant);
    let pending_for_merge = outcome
        .pending_variables
        .as_ref()
        .ok()
        .cloned()
        .unwrap_or_default();
    let variables_refreshed = match outcome.variables {
        Ok(mut vs) => {
            // Re-merge any entries we recently saved but the polled list
            // hasn't picked up yet (AIC's variable-list endpoint is
            // eventually consistent — a brand-new variable can lag by a
            // few seconds). Drop expired write pins while we're here.
            app.esv
                .recent_writes
                .retain(|_, (saved_at, _)| saved_at.elapsed() < RECENT_WRITE_TTL);
            // Drop expired delete tombstones so the red `!` ghost rows clear
            // once the delete has had time to settle.
            app.esv
                .recent_deletes
                .retain(|_, tomb| tomb.deleted_at.elapsed() < DELETE_TOMBSTONE_TTL);
            for ((t, recent_id), (_, body)) in app.esv.recent_writes.iter() {
                if t != &tenant {
                    continue;
                }
                if !vs.iter().any(|v| id_of(v) == recent_id) {
                    vs.push(body.clone());
                }
            }
            for pending in &pending_for_merge {
                let pending_id = id_of(pending);
                if !vs.iter().any(|v| id_of(v) == pending_id) {
                    vs.push(pending.clone());
                }
            }
            // Negative pin: AIC's list endpoint is eventually consistent, so a
            // just-deleted variable can still come back for a few polls. While
            // its tombstone is alive, suppress it from the live list so the row
            // stays "deleted" instead of flickering back to a normal entry.
            let suppressed: HashSet<String> = app
                .esv
                .recent_deletes
                .keys()
                .filter(|(t, _)| t == &tenant)
                .map(|(_, id)| id.clone())
                .collect();
            vs.retain(|v| !suppressed.contains(id_of(v)));
            app.esv
                .list
                .data
                .insert(tenant.clone(), LoadState::Loaded(vs));
            if is_active {
                let n = app
                    .esv
                    .matches(app.active_tenant().map(|t| t.name.as_str()))
                    .len();
                app.esv.clamp_selection(n);
            }
            true
        }
        Err(e) => {
            // Don't clobber a previously-cached list with a background-
            // refresh failure — keep showing the stale data and just log.
            if matches!(app.esv.list.data.get(&tenant), Some(LoadState::Loaded(_))) {
                tracing::warn!("ESV refresh failed for {tenant}: {e}");
            } else {
                app.esv
                    .list
                    .data
                    .insert(tenant.clone(), LoadState::Failed(e));
            }
            false
        }
    };
    let pending_refreshed = match outcome.pending_variables {
        Ok(vs) => {
            app.esv.list.pending_ids.insert(
                tenant.clone(),
                vs.iter().map(|v| id_of(v).to_string()).collect(),
            );
            true
        }
        Err(e) => {
            tracing::warn!("ESV pending-variable refresh failed for {tenant}: {e}");
            false
        }
    };

    // Hand the secret half of the poll to the secrets screen. Whether the
    // pending-secret fetch succeeded gates "authoritative" below, since the
    // pending count now folds in secrets.
    let secret_pending_refreshed = outcome.pending_secrets.is_ok();
    crate::screens::secret::apply_refresh(app, &tenant, &outcome.secrets, &outcome.pending_secrets);

    match outcome.startup {
        Ok(StartupStatus::Restarting) => {
            set_apply_state(
                app,
                &tenant,
                ApplyState::Restarting(pending_count(app, &tenant)),
            );
        }
        Ok(StartupStatus::Ready)
            if variables_refreshed && pending_refreshed && secret_pending_refreshed =>
        {
            set_apply_state(
                app,
                &tenant,
                ApplyState::from_authoritative(StartupStatus::Ready, pending_count(app, &tenant)),
            );
        }
        Ok(StartupStatus::Ready) => {
            // Startup alone can prove "restarting", but it cannot prove
            // "no changes" without a fresh variable list. Keep the cached
            // apply state until both tenant reads have succeeded together.
        }
        Err(e) => {
            tracing::warn!("ESV startup-status refresh failed for {tenant}: {e}");
        }
    }
}

fn set_apply_state(app: &mut App, tenant: &str, state: ApplyState) {
    match state {
        ApplyState::Restarting(_) => {
            app.esv
                .restart_started_at
                .entry(tenant.to_string())
                .or_insert_with(Instant::now);
        }
        ApplyState::NoChanges | ApplyState::Unapplied(_) => {
            app.esv.restart_started_at.remove(tenant);
        }
    }
    app.esv.apply_states.insert(tenant.to_string(), state);
}

fn refresh_apply_state_from_cache(app: &mut App, tenant: &str) {
    let pending = pending_count(app, tenant);
    let state = match app.esv.apply_states.get(tenant).copied() {
        Some(ApplyState::Restarting(_)) => ApplyState::Restarting(pending),
        _ if pending > 0 => ApplyState::Unapplied(pending),
        _ => ApplyState::NoChanges,
    };
    set_apply_state(app, tenant, state);
}

pub(crate) fn build_save_plan(app: &mut App) -> Option<SavePlan> {
    let tenant_name = app.active_tenant().map(|t| t.name.clone())?;
    let edit = app.esv.editing.as_mut()?;

    if edit.creating {
        let id = edit.id_input.value.trim().to_string();
        // The `esv-` prefix is locked in the field, so the only id problem the
        // user can still hit is leaving the suffix empty.
        if id == "esv-" || id.is_empty() {
            edit.error = Some("Give the variable a name after 'esv-'".into());
            return None;
        }
        if !id.starts_with("esv-") {
            edit.error = Some("_id must start with 'esv-'".into());
            return None;
        }
        edit.id = id;
    }

    // A variable value must be non-empty: base64 of "" is "", which AIC
    // rejects (and a rejected create leaves a confusing local-only row).
    // A single space is a valid, non-empty value.
    if edit.value.value.is_empty() {
        edit.error = Some("Value cannot be empty (a single space is allowed)".into());
        return None;
    }

    // Pre-flight validation. Catches obvious type/value mismatches before
    // we apply optimistically and ship a request that would just bounce.
    if let Err(msg) = edit.expr_type.validate(&edit.value.value) {
        edit.error = Some(msg);
        return None;
    }

    let id = edit.id.clone();
    let description = edit.description.value.clone();
    let expr_type = edit.expr_type.as_str().to_string();
    let value_str = edit.value.value.clone();
    let value_b64 = B64.encode(value_str.as_bytes());
    let creating = edit.creating;
    let was_creating = edit.creating;
    let original_for_conflict = if creating {
        None
    } else {
        Some(edit.original.clone())
    };

    if app
        .esv
        .in_flight_writes
        .contains(&(tenant_name.clone(), id.clone()))
    {
        edit.error = Some("Save already in progress for this variable".into());
        return None;
    }

    // Build the optimistic body the local list will show until the
    // server's echo lands. Server-managed fields are inherited from the
    // snapshot when editing, and stubbed for creates.
    let mut optimistic = if creating {
        serde_json::json!({})
    } else {
        edit.original.clone()
    };
    optimistic["_id"] = serde_json::Value::String(id.clone());
    optimistic["description"] = serde_json::Value::String(description.clone());
    optimistic["expressionType"] = serde_json::Value::String(expr_type.clone());
    optimistic["valueBase64"] = serde_json::Value::String(value_b64.clone());
    // We don't know the new lastChangeDate yet; stamp a placeholder so
    // it doesn't look like the previous edit was just now.
    optimistic["lastChangeDate"] = serde_json::Value::String("(saving…)".into());
    // The runtime hasn't picked it up yet — restart is pending until the
    // user triggers one. Holds for both edits and creates.
    optimistic["loaded"] = serde_json::Value::Bool(false);

    Some(SavePlan {
        tenant_name,
        id,
        description,
        expr_type,
        value_b64,
        original: original_for_conflict,
        optimistic,
        was_creating,
    })
}

pub fn execute_save_plan(app: &mut App, plan: SavePlan, confirmed_prod: bool) {
    let SavePlan {
        tenant_name,
        id,
        description,
        expr_type,
        value_b64,
        original,
        optimistic,
        was_creating,
    } = plan;

    if let Err(e) = record_save_undo(
        app,
        &tenant_name,
        &id,
        original.as_ref(),
        &optimistic,
        was_creating,
    ) {
        app.push_toast(
            ToastKind::Error,
            format!("Save cancelled: failed to record undo — {e}"),
        );
        return;
    }

    // Apply locally + pin across polls.
    if let Some(LoadState::Loaded(items)) = app.esv.list.data.get_mut(&tenant_name) {
        if let Some(slot) = items.iter_mut().find(|v| id_of(v) == id) {
            *slot = optimistic.clone();
        } else {
            items.push(optimistic.clone());
        }
    }
    app.esv.recent_writes.insert(
        (tenant_name.clone(), id.clone()),
        (Instant::now(), optimistic),
    );
    app.esv
        .recent_deletes
        .remove(&(tenant_name.clone(), id.clone()));
    // Mark the background PUT as in flight so the banner can flip
    // purple and ^S is gated until it returns.
    app.esv
        .in_flight_writes
        .insert((tenant_name.clone(), id.clone()));

    // Close the form. The actual save runs in the background; result
    // events arrive via `apply_save_result` and either silently refresh
    // the entry with the server's echo or toast an error.
    app.esv.editing = None;
    app.input_mode = InputMode::Normal;
    // Jump to the new row if we just created one.
    if was_creating {
        let matches = app.esv.matches(Some(&tenant_name));
        if let Some(pos) = matches.iter().position(|m| m.id == id) {
            app.esv.list.selected = pos;
        }
    }

    let request = SaveRequest {
        tenant_name,
        id,
        description,
        expr_type,
        value_b64,
        original,
    };
    let event_tenant = request.tenant_name.clone();
    let event_id = request.id.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = save_variable(request, confirmed_prod).await;
        let _ = tx.send(AppEvent::Esv(Event::SaveResult {
            tenant: event_tenant,
            id: event_id,
            result,
        }));
    });
}

fn record_save_undo(
    app: &mut App,
    tenant_name: &str,
    id: &str,
    original: Option<&serde_json::Value>,
    optimistic: &serde_json::Value,
    was_creating: bool,
) -> crate::Result<UndoId> {
    let entry = if was_creating {
        UndoEntry::pending(
            tenant_name.to_string(),
            "esv",
            format!("Delete created variable {id}"),
            Sensitivity::PublicMetadata,
            Capability::Undoable,
            Some(UndoOp::EsvVariableDelete {
                tenant: tenant_name.to_string(),
                id: id.to_string(),
                recorded_body: optimistic.clone(),
            }),
            ConflictCheck::ContentEqualsAfter {
                body: optimistic.clone(),
            },
        )
    } else if let Some(original) = original {
        UndoEntry::pending(
            tenant_name.to_string(),
            "esv",
            format!("Revert {id} to previous value"),
            Sensitivity::PublicMetadata,
            Capability::Undoable,
            Some(UndoOp::EsvVariableUpdateTo {
                tenant: tenant_name.to_string(),
                id: id.to_string(),
                body: original.clone(),
            }),
            ConflictCheck::ContentEqualsAfter {
                body: optimistic.clone(),
            },
        )
    } else {
        UndoEntry::pending(
            tenant_name.to_string(),
            "esv",
            format!("Changed {id}"),
            Sensitivity::PublicMetadata,
            Capability::Irreversible,
            None,
            ConflictCheck::None,
        )
    };
    app.undo.record(entry)
}

pub fn execute_delete_plan(app: &mut App, plan: DeletePlan, confirmed_prod: bool) {
    let DeletePlan {
        tenant_name,
        id,
        original,
    } = plan;

    if let Err(e) = record_delete_undo(app, &tenant_name, &id, &original) {
        app.push_toast(
            ToastKind::Error,
            format!("Delete cancelled: failed to record undo — {e}"),
        );
        app.input_mode = InputMode::Normal;
        return;
    }

    let mut remaining = None;
    if let Some(LoadState::Loaded(items)) = app.esv.list.data.get_mut(&tenant_name) {
        items.retain(|v| id_of(v) != id);
        remaining = Some(items.len());
    }
    if let Some(n) = remaining {
        app.esv.clamp_selection(n);
    }
    app.esv
        .recent_writes
        .remove(&(tenant_name.clone(), id.clone()));
    app.esv.recent_deletes.insert(
        (tenant_name.clone(), id.clone()),
        DeleteTombstone {
            deleted_at: Instant::now(),
            body: original.clone(),
        },
    );
    select_id(app, &tenant_name, &id);
    app.esv
        .failed_writes
        .remove(&(tenant_name.clone(), id.clone()));
    app.esv
        .in_flight_writes
        .insert((tenant_name.clone(), id.clone()));
    app.esv
        .in_flight_deletes
        .insert((tenant_name.clone(), id.clone()), original.clone());
    app.input_mode = InputMode::Normal;

    let request = DeleteRequest {
        tenant_name,
        id,
        original,
    };
    let event_tenant = request.tenant_name.clone();
    let event_id = request.id.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = delete_variable_request(request, confirmed_prod).await;
        let _ = tx.send(AppEvent::Esv(Event::DeleteResult {
            tenant: event_tenant,
            id: event_id,
            result,
        }));
    });
}

fn record_delete_undo(
    app: &mut App,
    tenant_name: &str,
    id: &str,
    original: &serde_json::Value,
) -> crate::Result<UndoId> {
    app.undo.record(UndoEntry::pending(
        tenant_name.to_string(),
        "esv",
        format!("Restore deleted variable {id}"),
        Sensitivity::PublicMetadata,
        Capability::Undoable,
        Some(UndoOp::EsvVariableRestore {
            tenant: tenant_name.to_string(),
            body: original.clone(),
        }),
        ConflictCheck::ResourceAbsent,
    ))
}

pub fn request_latest_undo(app: &mut App) {
    let Some(tenant) = app.active_tenant() else {
        return;
    };
    let tenant_name = tenant.name.clone();
    let Some(summary) = app.undo.latest_pending(&tenant_name) else {
        app.push_toast(ToastKind::Info, "Nothing to undo for this tenant");
        return;
    };

    if tenant.theme == TenantTheme::Production {
        app.prod_confirm.pending = Some(PendingProdAction::EsvUndo(summary.id));
        app.input_mode = InputMode::ProdConfirm;
    } else {
        execute_undo(app, summary.id, false);
    }
}

pub fn execute_undo(app: &mut App, undo_id: UndoId, confirmed_prod: bool) {
    let entry = match app.undo.load(undo_id) {
        Ok(entry) => entry,
        Err(e) => {
            app.push_toast(ToastKind::Error, format!("Undo failed: {e}"));
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

    let event_tenant = entry.tenant.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = apply_undo_entry(entry, confirmed_prod).await;
        let _ = tx.send(AppEvent::Esv(Event::UndoResult {
            undo_id,
            tenant: event_tenant,
            result,
        }));
    });
}

async fn save_variable(request: SaveRequest, confirmed_prod: bool) -> Result<SaveOutcome, String> {
    let SaveRequest {
        tenant_name,
        id,
        description,
        expr_type,
        value_b64,
        original,
    } = request;

    // Conflict check (against the snapshot we opened), the type-change
    // DELETE-then-PUT quirk, and create-on-absent all live in the shared
    // helper so the CLI takes exactly the same path.
    let saved = crate::esv::api::save_variable(
        &tenant_name,
        &id,
        &description,
        &expr_type,
        &value_b64,
        confirmed_prod,
        original.as_ref(),
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(SaveOutcome {
        body: saved.body,
        created: saved.created,
    })
}

async fn delete_variable_request(
    request: DeleteRequest,
    confirmed_prod: bool,
) -> Result<DeleteOutcome, String> {
    let DeleteRequest {
        tenant_name,
        id,
        original,
    } = request;

    let current = crate::esv::api::get_variable(&tenant_name, &id)
        .await
        .map_err(|e| format!("conflict check: {e}"))?;
    if !crate::esv::api::content_equal(&current, &original) {
        return Err("remote value changed since you selected it; refresh and retry".into());
    }
    crate::esv::api::delete_variable(&tenant_name, &id, confirmed_prod)
        .await
        .map_err(|e| e.to_string())?;
    Ok(DeleteOutcome)
}

async fn apply_undo_entry(
    entry: UndoEntry,
    confirmed_prod: bool,
) -> Result<UndoOutcome, UndoFailure> {
    let op = entry
        .op
        .clone()
        .ok_or_else(|| UndoFailure::Failed("undo entry has no operation".into()))?;
    check_undo_conflict(&op, &entry.conflict_check).await?;

    match op {
        UndoOp::EsvVariableRestore { tenant, body } => {
            let id = body_id(&body)?;
            let saved = upsert_variable_body(&tenant, &body, confirmed_prod).await?;
            Ok(UndoOutcome {
                description: entry.description,
                applied: UndoApplied::Upsert { id, body: saved },
            })
        }
        UndoOp::EsvVariableUpdateTo { tenant, id, body } => {
            let saved = upsert_variable_body(&tenant, &body, confirmed_prod).await?;
            Ok(UndoOutcome {
                description: entry.description,
                applied: UndoApplied::Upsert { id, body: saved },
            })
        }
        UndoOp::EsvVariableDelete {
            tenant,
            id,
            recorded_body,
        } => {
            crate::esv::api::delete_variable(&tenant, &id, confirmed_prod)
                .await
                .map_err(|e| UndoFailure::Failed(e.to_string()))?;
            Ok(UndoOutcome {
                description: entry.description,
                applied: UndoApplied::Delete {
                    id,
                    body: Some(recorded_body),
                },
            })
        }
        UndoOp::SecretDelete {
            tenant,
            id,
            active_version,
        } => {
            crate::screens::secret::undo_delete(&tenant, &id, &active_version, confirmed_prod)
                .await?;
            Ok(UndoOutcome {
                description: entry.description,
                applied: UndoApplied::SecretRemoved { id },
            })
        }
        UndoOp::SecretSetDescription {
            tenant,
            id,
            previous,
            expected,
        } => {
            crate::screens::secret::undo_set_description(
                &tenant,
                &id,
                &previous,
                &expected,
                confirmed_prod,
            )
            .await?;
            Ok(UndoOutcome {
                description: entry.description,
                applied: UndoApplied::SecretDescriptionSet,
            })
        }
    }
}

async fn check_undo_conflict(op: &UndoOp, check: &ConflictCheck) -> Result<(), UndoFailure> {
    match check {
        ConflictCheck::ContentEqualsAfter { body }
        | ConflictCheck::ContentEqualsBefore { body } => {
            let tenant = op.tenant();
            let id = op
                .resource_id()
                .ok_or_else(|| UndoFailure::Failed("undo operation has no resource id".into()))?;
            let current = crate::esv::api::get_variable(tenant, id)
                .await
                .map_err(|e| UndoFailure::Conflict(format!("current value unavailable: {e}")))?;
            if crate::esv::api::content_equal(&current, body) {
                Ok(())
            } else {
                Err(UndoFailure::Conflict(
                    "remote value changed since the original write".into(),
                ))
            }
        }
        ConflictCheck::ResourceAbsent => {
            let tenant = op.tenant();
            let id = op
                .resource_id()
                .ok_or_else(|| UndoFailure::Failed("undo operation has no resource id".into()))?;
            match crate::esv::api::get_variable(tenant, id).await {
                Ok(_) => Err(UndoFailure::Conflict(format!(
                    "{id} already exists; refusing to restore over it"
                ))),
                Err(e) if is_not_found(&e) => Ok(()),
                Err(e) => Err(UndoFailure::Failed(format!("conflict check failed: {e}"))),
            }
        }
        ConflictCheck::None => Ok(()),
    }
}

async fn upsert_variable_body(
    tenant: &str,
    body: &serde_json::Value,
    confirmed_prod: bool,
) -> Result<serde_json::Value, UndoFailure> {
    let id = body_id(body)?;
    let description = body
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let expression_type = body
        .get("expressionType")
        .and_then(|v| v.as_str())
        .ok_or_else(|| UndoFailure::Failed(format!("{id} has no expressionType")))?;
    let value_base64 = body
        .get("valueBase64")
        .and_then(|v| v.as_str())
        .ok_or_else(|| UndoFailure::Failed(format!("{id} has no valueBase64")))?;

    let delete_first = match crate::esv::api::get_variable(tenant, &id).await {
        Ok(current) => {
            current
                .get("expressionType")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                != expression_type
        }
        Err(e) if is_not_found(&e) => false,
        Err(e) => return Err(UndoFailure::Failed(format!("preflight fetch failed: {e}"))),
    };
    if delete_first {
        crate::esv::api::delete_variable(tenant, &id, confirmed_prod)
            .await
            .map_err(|e| UndoFailure::Failed(format!("type change delete failed: {e}")))?;
    }

    crate::esv::api::update_variable(
        tenant,
        &id,
        description,
        expression_type,
        value_base64,
        confirmed_prod,
    )
    .await
    .map_err(|e| UndoFailure::Failed(e.to_string()))
}

fn body_id(body: &serde_json::Value) -> Result<String, UndoFailure> {
    body.get("_id")
        .and_then(|v| v.as_str())
        .map(|id| id.to_string())
        .ok_or_else(|| UndoFailure::Failed("undo body has no _id".into()))
}

fn is_not_found(error: &crate::Error) -> bool {
    matches!(error, crate::Error::Api { status: 404, .. })
}

/// Background save finished. The edit form was already closed and the
/// list already shows the optimistic body — this just replaces the
/// optimistic placeholder with the server's echo (so `lastChangeDate`
/// and friends become real) or surfaces an error toast.
pub fn apply_save_result(
    app: &mut App,
    tenant: String,
    id: String,
    result: Result<SaveOutcome, String>,
) {
    // The background PUT has returned — clear the in-flight marker so
    // the "queued" banner can drop and ^S unlocks. Done regardless of
    // success or failure; failures are tracked separately in
    // `failed_writes`.
    app.esv
        .in_flight_writes
        .remove(&(tenant.clone(), id.clone()));
    match result {
        Ok(SaveOutcome { body, created }) => {
            if let Some(LoadState::Loaded(items)) = app.esv.list.data.get_mut(&tenant) {
                if let Some(slot) = items.iter_mut().find(|v| id_of(v) == id) {
                    *slot = body.clone();
                }
            }
            // Refresh the pin so the new server-echoed body survives
            // the next poll's eventual-consistency window.
            app.esv
                .recent_writes
                .insert((tenant.clone(), id.clone()), (Instant::now(), body));
            app.esv.recent_deletes.remove(&(tenant.clone(), id.clone()));
            // Clear any prior failure marker — the save went through.
            app.esv.failed_writes.remove(&(tenant.clone(), id.clone()));
            refresh_apply_state_from_cache(app, &tenant);
            let msg = if created {
                format!("{id} was missing on AIC — created it. Press ^Z to undo.")
            } else {
                "Saved ESV. Press ^Z to undo.".to_string()
            };
            app.push_toast(ToastKind::Success, msg);
        }
        Err(e) => {
            // Keep the optimistic body in `recent_writes` so the user
            // doesn't lose their attempted edit, and flag the row so the
            // list highlights it red.
            app.esv.failed_writes.insert((tenant, id.clone()));
            app.push_toast(ToastKind::Error, format!("Save failed: {id} — {e}"));
        }
    }
}

pub fn apply_delete_result(
    app: &mut App,
    tenant: String,
    id: String,
    result: Result<DeleteOutcome, String>,
) {
    app.esv
        .in_flight_writes
        .remove(&(tenant.clone(), id.clone()));
    let original = app
        .esv
        .in_flight_deletes
        .remove(&(tenant.clone(), id.clone()));

    match result {
        Ok(DeleteOutcome) => {
            app.esv.recent_writes.remove(&(tenant.clone(), id.clone()));
            app.esv.failed_writes.remove(&(tenant.clone(), id.clone()));
            refresh_apply_state_from_cache(app, &tenant);
            app.push_toast(
                ToastKind::Success,
                format!("Deleted {id}. Press ^Z to undo."),
            );
        }
        Err(e) => {
            app.esv.recent_deletes.remove(&(tenant.clone(), id.clone()));
            if let Some(original) = original {
                if let Some(LoadState::Loaded(items)) = app.esv.list.data.get_mut(&tenant) {
                    if let Some(slot) = items.iter_mut().find(|v| id_of(v) == id) {
                        *slot = original.clone();
                    } else {
                        items.push(original.clone());
                    }
                }
            }
            app.push_toast(ToastKind::Error, format!("Delete failed: {id} — {e}"));
        }
    }
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
            applied,
        }) => {
            if let Err(e) = app.undo.mark_applied(undo_id, EntryStatus::AppliedSuccess) {
                app.push_toast(
                    ToastKind::Error,
                    format!("Undo applied but log update failed: {e}"),
                );
            }
            match applied {
                UndoApplied::Upsert { id, body } => {
                    if let Some(LoadState::Loaded(items)) = app.esv.list.data.get_mut(&tenant) {
                        if let Some(slot) = items.iter_mut().find(|v| id_of(v) == id) {
                            *slot = body.clone();
                        } else {
                            items.push(body.clone());
                        }
                    }
                    app.esv
                        .recent_writes
                        .insert((tenant.clone(), id.clone()), (Instant::now(), body));
                    app.esv.recent_deletes.remove(&(tenant.clone(), id.clone()));
                    app.esv.failed_writes.remove(&(tenant.clone(), id.clone()));
                    select_id(app, &tenant, &id);
                }
                UndoApplied::Delete { id, body } => {
                    let mut remaining = None;
                    if let Some(LoadState::Loaded(items)) = app.esv.list.data.get_mut(&tenant) {
                        items.retain(|v| id_of(v) != id);
                        remaining = Some(items.len());
                    }
                    if let Some(n) = remaining {
                        app.esv.clamp_selection(n);
                    }
                    app.esv.recent_writes.remove(&(tenant.clone(), id.clone()));
                    if let Some(body) = body {
                        app.esv.recent_deletes.insert(
                            (tenant.clone(), id.clone()),
                            DeleteTombstone {
                                deleted_at: Instant::now(),
                                body,
                            },
                        );
                    }
                    app.esv.failed_writes.remove(&(tenant.clone(), id));
                }
                UndoApplied::SecretRemoved { id } => {
                    // Drop the secret locally and re-poll so the list + pending
                    // state reflect the removal.
                    if let Some(LoadState::Loaded(items)) = app.secret.list.data.get_mut(&tenant) {
                        items.retain(|v| id_of(v) != id);
                    }
                    refresh(app, true);
                }
                UndoApplied::SecretDescriptionSet => {
                    // The description lives only on the server object; re-poll
                    // to pull the reverted value back into the cache.
                    refresh(app, true);
                }
            }
            refresh_apply_state_from_cache(app, &tenant);
            app.push_toast(ToastKind::Success, format!("Undone: {description}"));
        }
        Err(UndoFailure::Conflict(message)) => {
            app.push_toast(ToastKind::Warning, format!("Undo conflict: {message}"));
        }
        Err(UndoFailure::Failed(message)) => {
            if let Err(e) = app.undo.mark_applied(undo_id, EntryStatus::AppliedFailure) {
                app.push_toast(
                    ToastKind::Error,
                    format!("Undo failure log update failed: {e}"),
                );
            }
            app.push_toast(ToastKind::Error, format!("Undo failed: {message}"));
        }
    }
}

fn select_id(app: &mut App, tenant: &str, id: &str) {
    let matches = app.esv.matches(Some(tenant));
    if let Some(pos) = matches.iter().position(|m| m.id == id) {
        app.esv.list.selected = pos;
    }
}
