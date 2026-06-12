//! Background mutations, refresh/result application, and undo for ESV secrets.

use crate::app::{App, InputMode};
use crate::config::tenant::TenantTheme;
use crate::esv::state::{LoadState, UndoFailure, id_of};
use crate::event::{AppEvent, ToastKind};
use crate::screens::prod_confirm::PendingProdAction;
use crate::secrets::screen::{self, Mode};
use crate::secrets::state::{
    CreatePlan, DeletePlan, DetailFocus, SecretOpKind, SetDescriptionPlan, VersionAddPlan,
    description_of, encode_value, rows, secret_in_cache, version_num,
};
use crate::undo::{Capability, ConflictCheck, Sensitivity, UndoEntry, UndoOp};

/// Apply the secret half of the shared ESV poll.
pub fn apply_refresh(
    app: &mut App,
    tenant: &str,
    secrets: &std::result::Result<Vec<serde_json::Value>, String>,
    pending: &std::result::Result<Vec<serde_json::Value>, String>,
) {
    match secrets {
        Ok(vs) => {
            app.secret
                .list
                .data
                .insert(tenant.to_string(), LoadState::Loaded(vs.clone()));
            let n = rows(app, Some(tenant)).len();
            if app.secret.list.selected >= n {
                app.secret.list.selected = n.saturating_sub(1);
            }
        }
        Err(e) => {
            if !matches!(app.secret.list.data.get(tenant), Some(LoadState::Loaded(_))) {
                app.secret
                    .list
                    .data
                    .insert(tenant.to_string(), LoadState::Failed(e.clone()));
            } else {
                tracing::warn!("secret refresh failed for {tenant}: {e}");
            }
        }
    }
    if let Ok(vs) = pending {
        app.secret.list.pending_ids.insert(
            tenant.to_string(),
            vs.iter().map(|v| id_of(v).to_string()).collect(),
        );
    }
}

pub(crate) fn commit_create(app: &mut App) {
    let Some(form) = app.secret.create.as_ref() else {
        return;
    };
    let Some(tenant) = app.active_tenant().map(|t| t.name.clone()) else {
        return;
    };
    let id = form.id.trimmed().to_string();
    let encoding = form.encoding;
    let use_in_placeholders = form.use_in_placeholders;
    let description = form.description.value.clone();

    if id == "esv-" || id.is_empty() {
        set_create_error(app, "Give the secret a name after 'esv-'");
        return;
    }
    if !id.starts_with("esv-") {
        set_create_error(app, "Secret ID must start with 'esv-'");
        return;
    }
    if app
        .secret
        .list
        .data
        .get(&tenant)
        .and_then(|s| match s {
            LoadState::Loaded(items) => Some(items.iter().any(|v| id_of(v) == id)),
            _ => None,
        })
        .unwrap_or(false)
    {
        set_create_error(
            app,
            "A secret with that ID already exists (PUT is create-only)",
        );
        return;
    }

    let value_b64 = match encode_value(encoding, &form.value.value, form.as_json) {
        Ok(v) => v,
        Err(e) => {
            set_create_error(app, &e);
            return;
        }
    };
    let plan = CreatePlan {
        tenant,
        id,
        encoding: encoding.as_str().to_string(),
        use_in_placeholders,
        value_b64,
        description,
    };
    app.secret.create = None;

    let is_prod = app
        .active_tenant()
        .is_some_and(|t| t.theme == TenantTheme::Production);
    if is_prod {
        app.prod_confirm.pending = Some(PendingProdAction::SecretsCreate(plan));
        app.input_mode = InputMode::ProdConfirm;
    } else {
        execute_create(app, plan, false);
    }
}

fn set_create_error(app: &mut App, msg: &str) {
    if let Some(form) = app.secret.create.as_mut() {
        form.error = Some(msg.to_string());
    }
}

pub(crate) fn commit_add_version(app: &mut App) {
    let Some(form) = app.secret.add_version.as_ref() else {
        return;
    };
    let value_b64 = match encode_value(form.encoding, &form.value.value, false) {
        Ok(v) => v,
        Err(e) => {
            if let Some(f) = app.secret.add_version.as_mut() {
                f.error = Some(e);
            }
            return;
        }
    };
    let plan = VersionAddPlan {
        tenant: form.tenant.clone(),
        id: form.id.clone(),
        value_b64,
    };
    app.secret.add_version = None;

    let is_prod = app
        .active_tenant()
        .is_some_and(|t| t.theme == TenantTheme::Production);
    if is_prod {
        app.prod_confirm.pending = Some(PendingProdAction::SecretsAddVersion(plan));
        app.input_mode = InputMode::ProdConfirm;
    } else {
        execute_add_version(app, plan, false);
    }
}

pub(crate) fn commit_description(app: &mut App) {
    let Some((tenant, id)) = app.secret.version_target.clone() else {
        return;
    };
    let new_desc = app.secret.description.value.clone();
    let previous = secret_in_cache(app, &tenant, &id)
        .map(|s| description_of(&s).to_string())
        .unwrap_or_default();
    if new_desc == previous {
        app.push_toast(ToastKind::Info, "Description unchanged".to_string());
        app.secret.detail_focus = DetailFocus::Versions;
        return;
    }
    let plan = SetDescriptionPlan {
        tenant,
        id,
        description: new_desc,
        previous,
    };
    let is_prod = app
        .active_tenant()
        .is_some_and(|t| t.theme == TenantTheme::Production);
    if is_prod {
        app.prod_confirm.pending = Some(PendingProdAction::SecretSetDescription(plan));
        app.input_mode = InputMode::ProdConfirm;
    } else {
        execute_set_description(app, plan, false);
    }
}

pub fn execute_create(app: &mut App, plan: CreatePlan, confirmed_prod: bool) {
    app.secret
        .in_flight
        .insert((plan.tenant.clone(), plan.id.clone()));
    app.input_mode = InputMode::Normal;
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::esv::api::create_secret(
            &plan.tenant,
            &plan.id,
            &plan.encoding,
            plan.use_in_placeholders,
            &plan.value_b64,
            &plan.description,
            confirmed_prod,
        )
        .await
        .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::Secrets(screen::Event::OpResult {
            tenant: plan.tenant,
            id: plan.id,
            kind: SecretOpKind::Create,
            label: "Created secret".to_string(),
            reload_versions: false,
            result,
        }));
    });
}

pub fn execute_add_version(app: &mut App, plan: VersionAddPlan, confirmed_prod: bool) {
    app.secret
        .in_flight
        .insert((plan.tenant.clone(), plan.id.clone()));
    app.input_mode = InputMode::Secrets(Mode::Versions);
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::esv::api::create_secret_version(
            &plan.tenant,
            &plan.id,
            &plan.value_b64,
            confirmed_prod,
        )
        .await
        .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::Secrets(screen::Event::OpResult {
            tenant: plan.tenant,
            id: plan.id,
            kind: SecretOpKind::AddVersion,
            label: "Added secret version".to_string(),
            reload_versions: true,
            result,
        }));
    });
}

pub(crate) fn reload_versions(app: &mut App, tenant: String, id: String) {
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::esv::api::list_secret_versions(&tenant, &id)
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::Secrets(screen::Event::VersionsListed {
            tenant,
            id,
            result,
        }));
    });
}

pub fn execute_set_description(app: &mut App, plan: SetDescriptionPlan, confirmed_prod: bool) {
    app.input_mode = InputMode::Secrets(Mode::Versions);
    app.secret.detail_focus = crate::secrets::state::DetailFocus::Versions;
    app.secret
        .in_flight
        .insert((plan.tenant.clone(), plan.id.clone()));
    set_local_secret_description(app, &plan.tenant, &plan.id, &plan.description);
    record_set_description_undo(app, &plan);

    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::esv::api::set_secret_description(
            &plan.tenant,
            &plan.id,
            &plan.description,
            confirmed_prod,
        )
        .await
        .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::Secrets(screen::Event::OpResult {
            tenant: plan.tenant,
            id: plan.id,
            kind: SecretOpKind::SetDescription,
            label: "Updated description".to_string(),
            reload_versions: false,
            result,
        }));
    });
}

fn record_set_description_undo(app: &mut App, plan: &SetDescriptionPlan) {
    let entry = UndoEntry::pending(
        plan.tenant.clone(),
        "secret",
        format!("Revert description of {}", plan.id),
        Sensitivity::TenantConfig,
        Capability::Undoable,
        Some(UndoOp::SecretSetDescription {
            tenant: plan.tenant.clone(),
            id: plan.id.clone(),
            previous: plan.previous.clone(),
            expected: plan.description.clone(),
        }),
        ConflictCheck::None,
    );
    if let Err(e) = app.undo.record(entry) {
        tracing::warn!(
            "failed to record secret-description undo for {}: {e}",
            plan.id
        );
    }
}

pub async fn undo_set_description(
    tenant: &str,
    id: &str,
    previous: &str,
    expected: &str,
    confirmed_prod: bool,
) -> std::result::Result<(), UndoFailure> {
    match crate::esv::api::get_secret(tenant, id).await {
        Ok(current) => {
            if description_of(&current) != expected {
                return Err(UndoFailure::Conflict(format!(
                    "{id}'s description changed since; refusing to overwrite"
                )));
            }
        }
        Err(crate::Error::Api { status: 404, .. }) => {
            return Err(UndoFailure::Conflict(format!("{id} no longer exists")));
        }
        Err(e) => return Err(UndoFailure::Failed(format!("conflict check failed: {e}"))),
    }
    crate::esv::api::set_secret_description(tenant, id, previous, confirmed_prod)
        .await
        .map(|_| ())
        .map_err(|e| UndoFailure::Failed(e.to_string()))
}

fn set_local_version_status(app: &mut App, tenant: &str, id: &str, version: &str, status: &str) {
    if let Some(LoadState::Loaded(vs)) = app
        .secret
        .versions
        .get_mut(&(tenant.to_string(), id.to_string()))
    {
        for v in vs.iter_mut() {
            if version_num(v).as_deref() == Some(version) {
                if let Some(obj) = v.as_object_mut() {
                    obj.insert(
                        "status".into(),
                        serde_json::Value::String(status.to_string()),
                    );
                }
            }
        }
    }
}

fn set_local_secret_description(app: &mut App, tenant: &str, id: &str, description: &str) {
    if let Some(LoadState::Loaded(items)) = app.secret.list.data.get_mut(tenant) {
        for v in items.iter_mut() {
            if id_of(v) == id {
                if let Some(obj) = v.as_object_mut() {
                    obj.insert(
                        "description".into(),
                        serde_json::Value::String(description.to_string()),
                    );
                }
            }
        }
    }
}

pub fn execute_version_status(
    app: &mut App,
    tenant: String,
    id: String,
    version: String,
    status: String,
    confirmed_prod: bool,
) {
    app.input_mode = InputMode::Secrets(Mode::Versions);
    set_local_version_status(app, &tenant, &id, &version, &status);
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result =
            crate::esv::api::change_version_status(&tenant, &id, &version, &status, confirmed_prod)
                .await
                .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::Secrets(screen::Event::OpResult {
            tenant,
            id,
            kind: SecretOpKind::StatusChange,
            label: format!("Version {version} → {status}"),
            reload_versions: true,
            result,
        }));
    });
}

pub fn execute_version_destroy(
    app: &mut App,
    tenant: String,
    id: String,
    version: String,
    confirmed_prod: bool,
) {
    app.input_mode = InputMode::Secrets(Mode::Versions);
    set_local_version_status(app, &tenant, &id, &version, "DESTROYED");
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result =
            crate::esv::api::destroy_secret_version(&tenant, &id, &version, confirmed_prod)
                .await
                .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::Secrets(screen::Event::OpResult {
            tenant,
            id,
            kind: SecretOpKind::Destroy,
            label: format!("Destroyed version {version}"),
            reload_versions: true,
            result,
        }));
    });
}

pub fn execute_delete(app: &mut App, plan: DeletePlan, confirmed_prod: bool) {
    app.input_mode = InputMode::Normal;
    app.secret
        .in_flight
        .insert((plan.tenant.clone(), plan.id.clone()));
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::esv::api::delete_secret(&plan.tenant, &plan.id, confirmed_prod)
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::Secrets(screen::Event::OpResult {
            tenant: plan.tenant,
            id: plan.id,
            kind: SecretOpKind::Delete,
            label: "Deleted secret".to_string(),
            reload_versions: false,
            result,
        }));
    });
}

pub fn apply_versions_listed(
    app: &mut App,
    tenant: String,
    id: String,
    result: std::result::Result<Vec<serde_json::Value>, String>,
) {
    let state = match result {
        Ok(vs) => {
            let n = vs.len();
            if app.secret.version_selected >= n {
                app.secret.version_selected = n.saturating_sub(1);
            }
            LoadState::Loaded(vs)
        }
        Err(e) => LoadState::Failed(e),
    };
    app.secret.versions.insert((tenant, id), state);
}

pub fn apply_op_result(
    app: &mut App,
    tenant: String,
    id: String,
    kind: SecretOpKind,
    label: String,
    reload: bool,
    result: std::result::Result<serde_json::Value, String>,
) {
    app.secret.in_flight.remove(&(tenant.clone(), id.clone()));
    match result {
        Ok(body) => {
            match kind {
                SecretOpKind::Create => record_create_undo(app, &tenant, &id, &body),
                SecretOpKind::Delete => record_delete_history(app, &tenant, &id),
                _ => {}
            }
            let suffix = if matches!(kind, SecretOpKind::Create | SecretOpKind::SetDescription) {
                " — ^Z to undo"
            } else {
                ""
            };
            app.push_toast(ToastKind::Success, format!("{label}: {id}{suffix}"));
            crate::esv::ops::refresh_tenant(app, &tenant, true);
            if reload {
                reload_versions(app, tenant, id);
            }
        }
        Err(e) => {
            app.push_toast(ToastKind::Error, format!("{label} failed: {id} — {e}"));
            if reload {
                reload_versions(app, tenant, id);
            } else {
                crate::esv::ops::refresh_tenant(app, &tenant, true);
            }
        }
    }
}

fn record_create_undo(app: &mut App, tenant: &str, id: &str, body: &serde_json::Value) {
    let active_version = body
        .get("activeVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("1")
        .to_string();
    let entry = UndoEntry::pending(
        tenant.to_string(),
        "secret",
        format!("Delete created secret {id}"),
        Sensitivity::TenantConfig,
        Capability::Undoable,
        Some(UndoOp::SecretDelete {
            tenant: tenant.to_string(),
            id: id.to_string(),
            active_version,
        }),
        ConflictCheck::None,
    );
    if let Err(e) = app.undo.record(entry) {
        tracing::warn!("failed to record secret-create undo for {id}: {e}");
    }
}

fn record_delete_history(app: &mut App, tenant: &str, id: &str) {
    let entry = UndoEntry::pending(
        tenant.to_string(),
        "secret",
        format!("Deleted secret {id} (irreversible)"),
        Sensitivity::TenantConfig,
        Capability::Irreversible,
        None,
        ConflictCheck::None,
    );
    if let Err(e) = app.undo.record(entry) {
        tracing::warn!("failed to record secret-delete history for {id}: {e}");
    }
}

pub async fn undo_delete(
    tenant: &str,
    id: &str,
    active_version: &str,
    confirmed_prod: bool,
) -> std::result::Result<(), UndoFailure> {
    match crate::esv::api::get_secret(tenant, id).await {
        Ok(current) => {
            let current_version = current
                .get("activeVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if current_version != active_version {
                return Err(UndoFailure::Conflict(format!(
                    "{id} gained new versions since it was created; refusing to delete"
                )));
            }
        }
        Err(crate::Error::Api { status: 404, .. }) => {
            return Err(UndoFailure::Conflict(format!("{id} no longer exists")));
        }
        Err(e) => return Err(UndoFailure::Failed(format!("conflict check failed: {e}"))),
    }
    crate::esv::api::delete_secret(tenant, id, confirmed_prod)
        .await
        .map(|_| ())
        .map_err(|e| UndoFailure::Failed(e.to_string()))
}
