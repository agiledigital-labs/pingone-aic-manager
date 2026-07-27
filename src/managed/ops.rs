//! Background writes and undo for the managed-object schema editor.
//!
//! The managed config document has no `_rev` and no object-level patch API.
//! Every write is a read-modify-write of `/openidm/config/managed`, guarded by
//! an object-subtree content snapshot so unrelated schema drift is not
//! overwritten silently.

use serde_json::{Value, json};

use crate::app::event::{AppEvent, ToastKind};
use crate::app::prod_confirm::PendingProdAction;
use crate::app::{App, InputMode};
use crate::config::tenant::TenantTheme;
#[derive(Debug)]
pub enum ProdAction {
    Update(Box<ObjectReplacePlan>),
    RenameObject(Box<RenameObjectRequest>),
    CreateObject(Box<CreateObjectRequest>),
    Undo(crate::undo::UndoId),
}
use crate::managed::screen::Event;
use crate::managed::state::{
    AddFieldState, AddRelationshipState, DeleteFieldState, EditFieldFocus, FieldAttr,
    FieldEditState, LoadState, RenameFieldState, RenameObjectState, ScalarFieldType, State,
};
use crate::undo::{Capability, ConflictCheck, EntryStatus, Sensitivity, UndoEntry, UndoId, UndoOp};

#[derive(Debug)]
pub struct ObjectReplacePlan {
    pub(crate) tenant_name: String,
    pub(crate) object_name: String,
    pub(crate) previous_object: Value,
    pub(crate) new_object: Value,
    pub(crate) searchable_changed: bool,
    pub(crate) success_message: String,
}

#[derive(Debug)]
pub struct UpdateOutcome {
    pub(crate) object: Value,
    pub(crate) searchable_changed: bool,
    pub(crate) success_message: String,
}

#[derive(Debug)]
pub struct UndoOutcome {
    pub(crate) description: String,
    pub(crate) object: Option<(String, Value)>,
    pub(crate) doc: Option<Value>,
}

#[derive(Debug)]
pub struct RenameObjectRequest {
    pub(crate) tenant_name: String,
    pub(crate) old_name: String,
    pub(crate) new_name: String,
    pub(crate) previous_doc: Value,
}

#[derive(Debug)]
pub struct CreateObjectRequest {
    pub(crate) tenant_name: String,
    pub(crate) name: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) previous_doc: Value,
}

/// Appends a minimal custom managed object to a whole managed config document.
pub fn create_object_in_doc(
    doc: &Value,
    name: &str,
    title: &str,
    description: &str,
) -> Result<Value, String> {
    let mut created = doc.clone();
    let objects =
        crate::managed::api::objects_mut(&mut created).map_err(|error| error.to_string())?;
    if objects
        .iter()
        .any(|object| object.get("name").and_then(Value::as_str) == Some(name))
    {
        return Err(format!("Managed object '{name}' already exists"));
    }
    let mut schema = serde_json::Map::new();
    schema.insert("type".into(), Value::String("object".into()));
    schema.insert(
        "title".into(),
        Value::String(if title.is_empty() { name } else { title }.into()),
    );
    if !description.is_empty() {
        schema.insert("description".into(), Value::String(description.into()));
    }
    schema.insert("properties".into(), Value::Object(serde_json::Map::new()));
    schema.insert("required".into(), Value::Array(Vec::new()));
    schema.insert("order".into(), Value::Array(Vec::new()));
    objects.push(json!({"name": name, "schema": schema}));
    Ok(created)
}

/// Renames an object identity and every schema relationship path that targets it.
pub fn rename_object_in_doc(doc: &Value, old: &str, new: &str) -> Result<(Value, usize), String> {
    let mut renamed = doc.clone();
    let objects =
        crate::managed::api::objects_mut(&mut renamed).map_err(|error| error.to_string())?;
    if objects
        .iter()
        .any(|object| object.get("name").and_then(Value::as_str) == Some(new))
    {
        return Err(format!("Managed object '{new}' already exists"));
    }
    let Some(source) = objects
        .iter_mut()
        .find(|object| object.get("name").and_then(Value::as_str) == Some(old))
    else {
        return Err(format!("No managed object named '{old}'"));
    };
    let Some(map) = source.as_object_mut() else {
        return Err(format!("Managed object '{old}' is malformed"));
    };
    map.insert("name".into(), Value::String(new.into()));
    let old_path = format!("managed/{old}");
    let new_path = format!("managed/{new}");
    let mut count = 0;
    for object in objects {
        let Some(properties) = object
            .pointer_mut("/schema/properties")
            .and_then(Value::as_object_mut)
        else {
            continue;
        };
        for property in properties.values_mut() {
            rewrite_relationship_paths(
                property.get_mut("resourceCollection"),
                &old_path,
                &new_path,
                &mut count,
            );
            rewrite_relationship_paths(
                property.pointer_mut("/items/resourceCollection"),
                &old_path,
                &new_path,
                &mut count,
            );
        }
    }
    Ok((renamed, count))
}

fn rewrite_relationship_paths(
    collection: Option<&mut Value>,
    old_path: &str,
    new_path: &str,
    count: &mut usize,
) {
    let Some(entries) = collection.and_then(Value::as_array_mut) else {
        return;
    };
    for entry in entries {
        if entry.get("path").and_then(Value::as_str) == Some(old_path) {
            if let Some(path) = entry.get_mut("path") {
                *path = Value::String(new_path.into());
                *count += 1;
            }
        }
    }
}

pub fn start_record_count(app: &mut App, draft: RenameObjectState) {
    let tenant = draft.tenant_name.clone();
    let old_name = draft.old_name.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::managed::api::count_records(&tenant, &old_name)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Managed(Event::RenameRecordCount {
            draft,
            result,
        }));
    });
}

pub fn execute_rename_object(app: &mut App, request: RenameObjectRequest, confirmed_prod: bool) {
    let undo_id = match app.undo.record(UndoEntry::pending(
        request.tenant_name.clone(),
        "managed",
        format!("Revert managed object rename {}", request.new_name),
        Sensitivity::TenantConfig,
        Capability::Undoable,
        Some(UndoOp::ManagedConfigReplace {
            tenant: request.tenant_name.clone(),
            body: request.previous_doc.clone(),
        }),
        ConflictCheck::ContentEqualsAfter {
            body: rename_object_in_doc(&request.previous_doc, &request.old_name, &request.new_name)
                .map_or(Value::Null, |(doc, _)| doc),
        },
    )) {
        Ok(id) => id,
        Err(error) => {
            app.push_toast(
                ToastKind::Error,
                format!("Rename cancelled: failed to record undo: {error}"),
            );
            return;
        }
    };
    let tenant = request.tenant_name.clone();
    app.managed
        .in_flight_writes
        .insert((tenant.clone(), request.old_name.clone()));
    app.managed.clear_active_drafts();
    app.input_mode = InputMode::Normal;
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = rename_object_request(&request, confirmed_prod)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Managed(Event::RenameResult {
            tenant,
            old_name: request.old_name,
            new_name: request.new_name,
            undo_id,
            result,
        }));
    });
}

pub fn execute_create_object(app: &mut App, request: CreateObjectRequest, confirmed_prod: bool) {
    let undo_id = match app.undo.record(UndoEntry::pending(
        request.tenant_name.clone(),
        "managed",
        format!("Remove managed object {}", request.name),
        Sensitivity::TenantConfig,
        Capability::Undoable,
        Some(UndoOp::ManagedConfigReplace {
            tenant: request.tenant_name.clone(),
            body: request.previous_doc.clone(),
        }),
        ConflictCheck::ContentEqualsAfter {
            body: create_object_in_doc(
                &request.previous_doc,
                &request.name,
                &request.title,
                &request.description,
            )
            .unwrap_or(Value::Null),
        },
    )) {
        Ok(id) => id,
        Err(error) => {
            app.push_toast(
                ToastKind::Error,
                format!("Create cancelled: failed to record undo: {error}"),
            );
            return;
        }
    };
    app.managed.clear_active_drafts();
    app.input_mode = InputMode::Normal;
    let tenant = request.tenant_name.clone();
    let name = request.name.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = create_object_request(&request, confirmed_prod)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Managed(Event::CreateResult {
            tenant,
            name,
            undo_id,
            result,
        }));
    });
}

async fn create_object_request(
    request: &CreateObjectRequest,
    confirmed_prod: bool,
) -> crate::Result<Value> {
    let live = crate::managed::api::get_managed(&request.tenant_name).await?;
    if crate::managed::api::object_named(&live, &request.name).is_ok() {
        return Err(crate::Error::Config(format!(
            "managed object '{}' already exists (created since you opened the form)",
            request.name
        )));
    }
    let new_doc = create_object_in_doc(&live, &request.name, &request.title, &request.description)
        .map_err(crate::Error::Config)?;
    crate::managed::api::replace_managed(&request.tenant_name, new_doc, confirmed_prod).await?;
    let confirmed = crate::managed::api::get_managed(&request.tenant_name).await?;
    if crate::managed::api::object_named(&confirmed, &request.name).is_err() {
        return Err(crate::Error::Config(
            "managed object create write returned but read-back did not match".into(),
        ));
    }
    Ok(confirmed)
}

pub fn apply_create_result(
    app: &mut App,
    tenant: String,
    name: String,
    undo_id: UndoId,
    result: Result<Value, String>,
) {
    match result {
        Ok(doc) => {
            app.managed
                .data
                .insert(tenant.clone(), LoadState::Loaded(doc));
            if app
                .active_tenant()
                .is_some_and(|active| active.name == tenant)
            {
                if let Some(index) = app
                    .managed
                    .matches(Some(&tenant))
                    .iter()
                    .position(|item| item.name == name)
                {
                    app.managed.selected = index;
                    app.managed.property_selected = 0;
                }
            }
            app.push_toast(
                ToastKind::Success,
                format!("Created managed object {name}. Press ^Z to undo."),
            );
        }
        Err(error) => {
            let _ = app.undo.mark_applied(undo_id, EntryStatus::Expired);
            app.push_toast(
                ToastKind::Error,
                format!("Managed create failed: {name}: {error}"),
            );
        }
    }
}

async fn rename_object_request(
    request: &RenameObjectRequest,
    confirmed_prod: bool,
) -> crate::Result<Value> {
    let live = crate::managed::api::get_managed(&request.tenant_name).await?;
    let live_old = crate::managed::api::object_named(&live, &request.old_name)?;
    let snapshot_old = crate::managed::api::object_named(&request.previous_doc, &request.old_name)?;
    if !crate::managed::api::object_content_equal(live_old, snapshot_old) {
        return Err(crate::Error::Config(
            "managed object changed since you opened it; refresh and retry".into(),
        ));
    }
    let (new_doc, _) = rename_object_in_doc(&live, &request.old_name, &request.new_name)
        .map_err(crate::Error::Config)?;
    crate::managed::api::replace_managed(&request.tenant_name, new_doc, confirmed_prod).await?;
    let confirmed = crate::managed::api::get_managed(&request.tenant_name).await?;
    if crate::managed::api::object_named(&confirmed, &request.new_name).is_err()
        || crate::managed::api::object_named(&confirmed, &request.old_name).is_ok()
    {
        return Err(crate::Error::Config(
            "managed object rename write returned but read-back did not match".into(),
        ));
    }
    Ok(confirmed)
}

pub fn apply_rename_result(
    app: &mut App,
    tenant: String,
    old_name: String,
    new_name: String,
    undo_id: UndoId,
    result: Result<Value, String>,
) {
    app.managed
        .in_flight_writes
        .remove(&(tenant.clone(), old_name.clone()));
    match result {
        Ok(doc) => {
            app.managed.data.insert(tenant, LoadState::Loaded(doc));
            app.push_toast(
                ToastKind::Success,
                format!("Renamed managed object {old_name} to {new_name}. Press ^Z to undo."),
            );
        }
        Err(error) => {
            let _ = app.undo.mark_applied(undo_id, EntryStatus::Expired);
            app.push_toast(
                ToastKind::Error,
                format!("Managed rename failed: {old_name}: {error}"),
            );
        }
    }
}

#[derive(Debug)]
pub enum UndoFailure {
    Conflict(String),
    Failed(String),
}

pub fn build_edit_field_plan(app: &mut App) -> Option<ObjectReplacePlan> {
    let edit = app.managed.editing.as_mut()?;
    if app
        .managed
        .in_flight_writes
        .contains(&(edit.tenant_name.clone(), edit.object_name.clone()))
    {
        edit.error = Some("Write already in progress for this object".into());
        return None;
    }

    let applied = match apply_field_edit(edit) {
        Ok(applied) => applied,
        Err(message) => {
            edit.error = Some(message);
            return None;
        }
    };
    if crate::managed::api::object_content_equal(&applied.object, &edit.original_object) {
        edit.error = Some("No changes to save".into());
        return None;
    }

    let success_message = if applied.renamed {
        format!(
            "Renamed managed field {}.{} to {}. Press ^Z to undo.",
            edit.object_name, edit.field_key, applied.field_key
        )
    } else {
        "Saved managed field attributes. Press ^Z to undo.".to_string()
    };

    Some(ObjectReplacePlan {
        tenant_name: edit.tenant_name.clone(),
        object_name: edit.object_name.clone(),
        previous_object: edit.original_object.clone(),
        new_object: applied.object,
        searchable_changed: applied.searchable_changed,
        success_message,
    })
}

pub fn build_add_field_plan(app: &mut App) -> Option<ObjectReplacePlan> {
    let draft = app.managed.add_field.as_mut()?;
    if app
        .managed
        .in_flight_writes
        .contains(&(draft.tenant_name.clone(), draft.object_name.clone()))
    {
        draft.error = Some("Write already in progress for this object".into());
        return None;
    }

    let applied = match apply_add_field(draft) {
        Ok(applied) => applied,
        Err(message) => {
            draft.error = Some(message);
            return None;
        }
    };

    Some(ObjectReplacePlan {
        tenant_name: draft.tenant_name.clone(),
        object_name: draft.object_name.clone(),
        previous_object: draft.original_object.clone(),
        new_object: applied.object,
        searchable_changed: applied.searchable_changed,
        success_message: format!(
            "Added managed field {}.{}. Press ^Z to undo.",
            draft.object_name, applied.field_key
        ),
    })
}

pub fn build_add_relationship_plan(app: &mut App) -> Option<ObjectReplacePlan> {
    let draft = app.managed.add_relationship.as_mut()?;
    if app
        .managed
        .in_flight_writes
        .contains(&(draft.tenant_name.clone(), draft.object_name.clone()))
    {
        draft.error = Some("Write already in progress for this object".into());
        return None;
    }

    let applied = match apply_add_relationship(draft) {
        Ok(applied) => applied,
        Err(message) => {
            draft.error = Some(message);
            return None;
        }
    };

    Some(ObjectReplacePlan {
        tenant_name: draft.tenant_name.clone(),
        object_name: draft.object_name.clone(),
        previous_object: draft.original_object.clone(),
        new_object: applied.object,
        searchable_changed: false,
        success_message: format!(
            "Added relationship {}.{}. Press ^Z to undo.",
            draft.object_name, applied.field_key
        ),
    })
}

pub fn build_add_hook_plan(app: &mut App) -> Option<ObjectReplacePlan> {
    let draft = app.managed.add_hook.as_mut()?;
    if app
        .managed
        .in_flight_writes
        .contains(&(draft.tenant_name.clone(), draft.object_name.clone()))
    {
        draft.error = Some("Write already in progress for this object".into());
        return None;
    }

    let event = match draft.events.get(draft.selected).copied() {
        Some(event) => event,
        None => {
            draft.error = Some("No hook event available to add".into());
            return None;
        }
    };
    let new_object = match apply_add_hook(&draft.original_object, &draft.object_name, event) {
        Ok(object) => object,
        Err(message) => {
            draft.error = Some(message);
            return None;
        }
    };
    if crate::managed::api::object_content_equal(&new_object, &draft.original_object) {
        draft.error = Some("Hook already exists on this object".into());
        return None;
    }

    Some(ObjectReplacePlan {
        tenant_name: draft.tenant_name.clone(),
        object_name: draft.object_name.clone(),
        previous_object: draft.original_object.clone(),
        new_object,
        searchable_changed: false,
        success_message: format!(
            "Hook registered — edit it with `aic script pull managed/{}.{event}`",
            draft.object_name
        ),
    })
}

pub fn build_delete_field_plan(app: &mut App) -> Option<ObjectReplacePlan> {
    let pending = app.managed.pending_delete.as_ref()?;
    if app
        .managed
        .in_flight_writes
        .contains(&(pending.tenant_name.clone(), pending.object_name.clone()))
    {
        app.push_toast(
            ToastKind::Info,
            format!("Write already in progress: {}", pending.object_name),
        );
        return None;
    }

    let new_object = match apply_delete_field(pending) {
        Ok(object) => object,
        Err(message) => {
            app.push_toast(ToastKind::Error, message);
            return None;
        }
    };
    let mut success_message = format!(
        "Deleted field {}.{}. Press ^Z to undo.",
        pending.object_name, pending.field_key
    );
    if pending.is_relationship {
        success_message = format!(
            "Deleted relationship {}.{}. Reverse links on the target were not changed. Press ^Z to undo.",
            pending.object_name, pending.field_key
        );
    }

    Some(ObjectReplacePlan {
        tenant_name: pending.tenant_name.clone(),
        object_name: pending.object_name.clone(),
        previous_object: pending.original_object.clone(),
        new_object,
        searchable_changed: false,
        success_message,
    })
}

/// Builds the undoable whole-object replacement plan for a property-key rename.
pub fn build_rename_field_plan(app: &mut App) -> Option<ObjectReplacePlan> {
    let rename = app.managed.renaming.as_mut()?;
    if app
        .managed
        .in_flight_writes
        .contains(&(rename.tenant_name.clone(), rename.object_name.clone()))
    {
        rename.error = Some("Write already in progress for this object".into());
        return None;
    }

    let new_key = rename.key.value.clone();
    let new_object = match apply_rename_field(rename, &new_key) {
        Ok(object) => object,
        Err(message) => {
            rename.error = Some(message);
            return None;
        }
    };
    if crate::managed::api::object_content_equal(&new_object, &rename.original_object) {
        rename.error = Some("No changes to save".into());
        return None;
    }

    Some(ObjectReplacePlan {
        tenant_name: rename.tenant_name.clone(),
        object_name: rename.object_name.clone(),
        previous_object: rename.original_object.clone(),
        new_object,
        searchable_changed: false,
        success_message: format!(
            "Renamed managed field {}.{} to {}. Press ^Z to undo.",
            rename.object_name, rename.old_key, new_key
        ),
    })
}

pub fn execute_update_plan(app: &mut App, plan: ObjectReplacePlan, confirmed_prod: bool) {
    let ObjectReplacePlan {
        tenant_name,
        object_name,
        previous_object,
        new_object,
        searchable_changed,
        success_message,
    } = plan;

    let undo_id = match record_replace_undo(
        app,
        &tenant_name,
        &object_name,
        &previous_object,
        &new_object,
    ) {
        Ok(undo_id) => undo_id,
        Err(error) => {
            app.push_toast(
                ToastKind::Error,
                format!("Save cancelled: failed to record undo: {error}"),
            );
            return;
        }
    };

    set_cached_object(app, &tenant_name, &object_name, new_object.clone());
    app.managed
        .in_flight_writes
        .insert((tenant_name.clone(), object_name.clone()));
    app.managed
        .failed_writes
        .remove(&(tenant_name.clone(), object_name.clone()));
    app.managed.clear_active_drafts();
    app.input_mode = InputMode::Normal;

    let request = ObjectReplacePlan {
        tenant_name,
        object_name,
        previous_object,
        new_object,
        searchable_changed,
        success_message,
    };
    let event_tenant = request.tenant_name.clone();
    let event_object = request.object_name.clone();
    let event_previous_object = request.previous_object.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = replace_object_request(request, confirmed_prod)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Managed(Event::UpdateResult {
            tenant: event_tenant,
            object_name: event_object,
            undo_id,
            previous_object: event_previous_object,
            result,
        }));
    });
}

fn record_replace_undo(
    app: &mut App,
    tenant_name: &str,
    object_name: &str,
    previous_object: &Value,
    new_object: &Value,
) -> crate::Result<UndoId> {
    app.undo.record(UndoEntry::pending(
        tenant_name.to_string(),
        "managed",
        format!("Revert managed object {object_name}"),
        Sensitivity::TenantConfig,
        Capability::Undoable,
        Some(UndoOp::ManagedObjectReplace {
            tenant: tenant_name.to_string(),
            object_name: object_name.to_string(),
            body: previous_object.clone(),
        }),
        ConflictCheck::ContentEqualsAfter {
            body: new_object.clone(),
        },
    ))
}

async fn replace_object_request(
    plan: ObjectReplacePlan,
    confirmed_prod: bool,
) -> crate::Result<UpdateOutcome> {
    let ObjectReplacePlan {
        tenant_name,
        object_name,
        previous_object,
        new_object,
        searchable_changed,
        success_message,
    } = plan;

    let confirmed = replace_object_with_snapshot(
        &tenant_name,
        &object_name,
        &previous_object,
        &new_object,
        confirmed_prod,
    )
    .await?;
    Ok(UpdateOutcome {
        object: confirmed,
        searchable_changed,
        success_message,
    })
}

async fn replace_object_with_snapshot(
    tenant_name: &str,
    object_name: &str,
    expected_current: &Value,
    replacement: &Value,
    confirmed_prod: bool,
) -> crate::Result<Value> {
    let (mut doc, current_object) =
        crate::managed::api::get_managed_with_object(tenant_name, object_name).await?;
    if !crate::managed::api::object_content_equal(&current_object, expected_current) {
        return Err(crate::Error::Config(format!(
            "remote managed object '{object_name}' changed since you opened it; refresh and retry"
        )));
    }

    crate::managed::api::replace_object(&mut doc, object_name, replacement.clone())?;
    crate::managed::api::replace_managed(tenant_name, doc, confirmed_prod).await?;

    // Managed-config read-back is strongly consistent for schema storage
    // (verified 2026-06-14), so one fresh GET is enough. Hook runtime
    // activation has a separate lag and is handled by scripts/managed_hooks.
    let (_, confirmed_object) =
        crate::managed::api::get_managed_with_object(tenant_name, object_name).await?;
    if !crate::managed::api::object_content_equal(&confirmed_object, replacement) {
        return Err(crate::Error::Config(format!(
            "managed object '{object_name}' write returned but read-back did not match"
        )));
    }
    Ok(confirmed_object)
}

pub fn apply_update_result(
    app: &mut App,
    tenant: String,
    object_name: String,
    undo_id: UndoId,
    previous_object: Value,
    result: Result<UpdateOutcome, String>,
) {
    app.managed
        .in_flight_writes
        .remove(&(tenant.clone(), object_name.clone()));
    match result {
        Ok(UpdateOutcome {
            object,
            searchable_changed,
            success_message,
        }) => {
            set_cached_object(app, &tenant, &object_name, object);
            app.managed
                .failed_writes
                .remove(&(tenant.clone(), object_name.clone()));
            let mut message = success_message;
            if searchable_changed {
                // This flag only changes the managed schema. Full directory
                // indexing also requires repo.ds mapping/index work, which is
                // deliberately outside this managed-object editor slice.
                message.push_str(" Searchable only updates schema; repo.ds indexing is separate.");
            }
            app.push_toast(ToastKind::Success, message);
        }
        Err(error) => {
            if let Err(mark_error) = revert_failed_update(
                &mut app.managed,
                app.undo.as_mut(),
                &tenant,
                &object_name,
                undo_id,
                previous_object,
            ) {
                app.push_toast(
                    ToastKind::Error,
                    format!("Failed to expire undo for failed managed save: {mark_error}"),
                );
            }
            app.push_toast(
                ToastKind::Error,
                format!("Managed save failed: {object_name}: {error}"),
            );
        }
    }
}

pub fn request_latest_undo(app: &mut App) {
    let Some(tenant) = app.active_tenant() else {
        return;
    };
    let tenant_name = tenant.name.clone();
    let Some(undo_id) = latest_pending_managed_undo(app, &tenant_name) else {
        app.push_toast(ToastKind::Info, "No managed-object undo for this tenant");
        return;
    };

    if tenant.theme == TenantTheme::Production {
        app.prod_confirm.pending = Some(PendingProdAction::Managed(ProdAction::Undo(undo_id)));
        app.input_mode = InputMode::ProdConfirm;
    } else {
        execute_undo(app, undo_id, false);
    }
}

fn latest_pending_managed_undo(app: &App, tenant: &str) -> Option<UndoId> {
    app.undo
        .list(100)
        .into_iter()
        .filter(|summary| {
            summary.tenant == tenant
                && summary.status == EntryStatus::Pending
                && matches!(
                    summary.capability,
                    Capability::Undoable | Capability::BestEffort
                )
        })
        .find_map(|summary| {
            let entry = app.undo.load(summary.id).ok()?;
            matches!(
                entry.op,
                Some(UndoOp::ManagedObjectReplace { .. } | UndoOp::ManagedConfigReplace { .. })
            )
            .then_some(summary.id)
        })
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
    if !matches!(
        entry.op,
        Some(UndoOp::ManagedObjectReplace { .. } | UndoOp::ManagedConfigReplace { .. })
    ) {
        app.push_toast(ToastKind::Info, "Undo entry is not a managed-object change");
        return;
    }

    let event_tenant = entry.tenant.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = apply_undo_entry(entry, confirmed_prod).await;
        let _ = tx.send(AppEvent::Managed(Event::UndoResult {
            undo_id,
            tenant: event_tenant,
            result,
        }));
    });
}

async fn apply_undo_entry(
    entry: UndoEntry,
    confirmed_prod: bool,
) -> Result<UndoOutcome, UndoFailure> {
    let op = entry
        .op
        .clone()
        .ok_or_else(|| UndoFailure::Failed("undo entry has no operation".into()))?;
    let expected_current = match entry.conflict_check {
        ConflictCheck::ContentEqualsAfter { body }
        | ConflictCheck::ContentEqualsBefore { body } => body,
        _ => {
            return Err(UndoFailure::Failed(
                "managed-object undo has no content snapshot".into(),
            ));
        }
    };

    let outcome = match op {
        UndoOp::ManagedObjectReplace {
            tenant,
            object_name,
            body,
        } => {
            let object = replace_object_with_snapshot(
                &tenant,
                &object_name,
                &expected_current,
                &body,
                confirmed_prod,
            )
            .await
            .map_err(undo_failure)?;
            UndoOutcome {
                description: entry.description,
                object: Some((object_name, object)),
                doc: None,
            }
        }
        UndoOp::ManagedConfigReplace { tenant, body } => {
            let live = crate::managed::api::get_managed(&tenant)
                .await
                .map_err(undo_failure)?;
            if live != expected_current {
                return Err(UndoFailure::Conflict(
                    "managed config changed since the rename; refresh and retry".into(),
                ));
            }
            crate::managed::api::replace_managed(&tenant, body.clone(), confirmed_prod)
                .await
                .map_err(undo_failure)?;
            let confirmed = crate::managed::api::get_managed(&tenant)
                .await
                .map_err(undo_failure)?;
            if confirmed != body {
                return Err(UndoFailure::Failed(
                    "managed config undo write returned but read-back did not match".into(),
                ));
            }
            UndoOutcome {
                description: entry.description,
                object: None,
                doc: Some(confirmed),
            }
        }
        _ => {
            return Err(UndoFailure::Failed(
                "undo entry is not a managed-object operation".into(),
            ));
        }
    };
    Ok(outcome)
}

fn undo_failure(error: crate::Error) -> UndoFailure {
    match error {
        crate::Error::Config(message) if message.contains("changed since") => {
            UndoFailure::Conflict(message)
        }
        other => UndoFailure::Failed(other.to_string()),
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
            object,
            doc,
        }) => {
            if let Err(error) = app.undo.mark_applied(undo_id, EntryStatus::AppliedSuccess) {
                app.push_toast(
                    ToastKind::Error,
                    format!("Undo applied but log update failed: {error}"),
                );
            }
            if let Some((object_name, object)) = object {
                set_cached_object(app, &tenant, &object_name, object);
                app.managed
                    .failed_writes
                    .remove(&(tenant.clone(), object_name));
            }
            if let Some(doc) = doc {
                app.managed
                    .data
                    .insert(tenant.clone(), LoadState::Loaded(doc));
            }
            app.push_toast(ToastKind::Success, format!("Undone: {description}"));
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

pub(crate) fn set_cached_object(app: &mut App, tenant: &str, object_name: &str, object: Value) {
    if let Err(error) = set_cached_object_in_state(&mut app.managed, tenant, object_name, object) {
        tracing::warn!("failed to update managed cache for {object_name}: {error}");
    }
}

fn set_cached_object_in_state(
    managed: &mut State,
    tenant: &str,
    object_name: &str,
    object: Value,
) -> crate::Result<()> {
    if let Some(LoadState::Loaded(doc)) = managed.data.get_mut(tenant) {
        crate::managed::api::replace_object(doc, object_name, object)?;
    }
    Ok(())
}

fn revert_failed_update(
    managed: &mut State,
    undo: &mut dyn crate::undo::UndoLog,
    tenant: &str,
    object_name: &str,
    undo_id: UndoId,
    previous_object: Value,
) -> crate::Result<()> {
    if let Err(error) = set_cached_object_in_state(managed, tenant, object_name, previous_object) {
        tracing::warn!("failed to restore managed cache for {object_name}: {error}");
    }
    managed
        .failed_writes
        .insert((tenant.to_string(), object_name.to_string()));
    undo.mark_applied(undo_id, EntryStatus::Expired)
}

#[derive(Debug)]
struct FieldEditApplied {
    object: Value,
    field_key: String,
    searchable_changed: bool,
    renamed: bool,
}

#[derive(Debug)]
struct AddFieldApplied {
    object: Value,
    field_key: String,
    searchable_changed: bool,
}

fn apply_field_edit(edit: &FieldEditState) -> Result<FieldEditApplied, String> {
    let mut object = edit.original_object.clone();
    let mut property = edit.original_property.clone();
    let property_map = property.as_object_mut().ok_or_else(|| {
        format!(
            "field '{}' is not an object-valued property",
            edit.field_key
        )
    })?;

    if edit.caps.can_edit_attr(FieldAttr::Title) {
        set_optional_string(property_map, "title", &edit.title.value);
    }
    if edit.caps.can_edit_attr(FieldAttr::Description) {
        set_optional_string(property_map, "description", &edit.description.value);
    }
    if edit.caps.can_edit_attr(FieldAttr::Searchable) {
        property_map.insert("searchable".into(), Value::Bool(edit.searchable));
    }
    if edit.caps.can_edit_attr(FieldAttr::Viewable) {
        property_map.insert("viewable".into(), Value::Bool(edit.viewable));
    }
    if edit.caps.can_edit_attr(FieldAttr::UserEditable) {
        property_map.insert("userEditable".into(), Value::Bool(edit.user_editable));
    }

    let field_key = if edit.caps.rename_key {
        crate::managed::state::normalize_new_property_key(&edit.original_object, &edit.key.value)?
    } else {
        edit.field_key.clone()
    };
    if !edit.caps.rename_key && field_key != edit.field_key {
        return Err("This field key cannot be renamed".into());
    }
    if edit.caps.rename_key && crate::managed::state::is_relationship_property(&property) {
        return Err(
            "Relationship keys cannot be renamed; delete and recreate the relationship".into(),
        );
    }

    upsert_property(&mut object, &edit.field_key, &field_key, property)?;

    if edit.caps.can_edit_attr(FieldAttr::Required) {
        set_required(&mut object, &field_key, edit.required)?;
    }
    let searchable_changed = edit
        .original_property
        .get("searchable")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        != edit.searchable;

    Ok(FieldEditApplied {
        object,
        renamed: field_key != edit.field_key,
        field_key,
        searchable_changed,
    })
}

fn apply_add_field(draft: &AddFieldState) -> Result<AddFieldApplied, String> {
    let mut object = draft.original_object.clone();
    let field_key = crate::managed::state::normalize_new_property_key(
        &draft.original_object,
        &draft.key.value,
    )?;
    ensure_property_available(&object, &field_key, None)?;

    let mut property = match draft.field_type {
        ScalarFieldType::String => json!({"type": "string"}),
        ScalarFieldType::Boolean => json!({"type": "boolean"}),
        ScalarFieldType::Number => json!({"type": "number"}),
        ScalarFieldType::StringArray => json!({"type": "array", "items": {"type": "string"}}),
    };
    let property_map = property
        .as_object_mut()
        .ok_or_else(|| "new scalar property is not object-valued".to_string())?;
    set_optional_string(property_map, "title", draft.title.trimmed());
    set_optional_string(property_map, "description", draft.description.trimmed());
    property_map.insert("searchable".into(), Value::Bool(draft.searchable));
    property_map.insert("viewable".into(), Value::Bool(draft.viewable));
    property_map.insert("userEditable".into(), Value::Bool(draft.user_editable));

    insert_new_property(&mut object, &field_key, property, draft.required)?;
    Ok(AddFieldApplied {
        object,
        field_key,
        searchable_changed: draft.searchable,
    })
}

fn apply_add_relationship(draft: &AddRelationshipState) -> Result<AddFieldApplied, String> {
    let mut object = draft.original_object.clone();
    let field_key = crate::managed::state::normalize_new_property_key(
        &draft.original_object,
        &draft.key.value,
    )?;
    ensure_property_available(&object, &field_key, None)?;
    let target = draft
        .target_name
        .as_deref()
        .ok_or_else(|| "Choose a target object".to_string())?;
    let property = relationship_property(draft, &field_key, target);
    insert_new_property(&mut object, &field_key, property, false)?;
    Ok(AddFieldApplied {
        object,
        field_key,
        searchable_changed: false,
    })
}

fn apply_add_hook(object_def: &Value, object_name: &str, event: &str) -> Result<Value, String> {
    let mut object = object_def.clone();
    let object_map = object
        .as_object_mut()
        .ok_or_else(|| format!("managed object '{object_name}' is not object-valued"))?;
    if object_map.contains_key(event) {
        return Ok(object);
    }
    object_map.insert(
        event.to_string(),
        json!({
            "type": "text/javascript",
            "source": format!("// {event} for {object_name}\n"),
        }),
    );
    Ok(object)
}

fn apply_delete_field(pending: &DeleteFieldState) -> Result<Value, String> {
    let mut object = pending.original_object.clone();
    let property = properties_mut(&mut object)?
        .remove(&pending.field_key)
        .ok_or_else(|| format!("field '{}' no longer exists", pending.field_key))?;
    let caps = crate::managed::state::field_capability_for_property(
        &pending.original_object,
        &pending.field_key,
        &property,
    );
    if !caps.delete {
        return Err("Standard fields cannot be deleted".into());
    }
    remove_order_key(&mut object, &pending.field_key)?;
    set_required(&mut object, &pending.field_key, false)?;
    Ok(object)
}

fn apply_rename_field(rename: &RenameFieldState, new_key: &str) -> Result<Value, String> {
    if new_key.trim() != new_key {
        return Err("Property key cannot have leading or trailing whitespace".into());
    }
    crate::managed::state::validate_property_key(new_key)?;

    let mut object = rename.original_object.clone();
    ensure_property_available(&object, new_key, Some(&rename.old_key))?;
    if new_key == rename.old_key {
        return Ok(object);
    }

    let properties = properties_mut(&mut object)?;
    let previous = std::mem::take(properties);
    let mut renamed = false;
    for (key, property) in previous {
        if key == rename.old_key {
            properties.insert(new_key.to_string(), property);
            renamed = true;
        } else {
            properties.insert(key, property);
        }
    }
    if !renamed {
        return Err(format!("field '{}' no longer exists", rename.old_key));
    }

    replace_key_in_array(&mut object, "/schema/order", &rename.old_key, new_key);
    replace_key_in_array(&mut object, "/schema/required", &rename.old_key, new_key);
    Ok(object)
}

fn replace_key_in_array(object: &mut Value, pointer: &str, old_key: &str, new_key: &str) {
    let Some(values) = object.pointer_mut(pointer).and_then(Value::as_array_mut) else {
        return;
    };
    for value in values {
        if value.as_str() == Some(old_key) {
            *value = Value::String(new_key.to_string());
        }
    }
}

fn relationship_property(draft: &AddRelationshipState, field_key: &str, target: &str) -> Value {
    let title = if draft.title.trimmed().is_empty() {
        title_from_key(field_key)
    } else {
        draft.title.trimmed().to_string()
    };
    let description = draft.description.trimmed().to_string();
    let mut relationship = json!({
        "type": "relationship",
        "title": title.clone(),
        "description": description.clone(),
        "validate": draft.validate,
        "resourceCollection": [{"path": format!("managed/{target}")}],
        "properties": {
            "_ref": {
                "description": "References a relationship from a managed object",
                "type": "string"
            },
            "_refProperties": {
                "description": "Supports metadata within the relationship",
                "properties": {
                    "_id": {
                        "description": "_refProperties object ID",
                        "type": "string"
                    }
                },
                "title": format!("{field_key} _refProperties"),
                "type": "object"
            }
        }
    });
    let reverse = draft.reverse_property_name.trimmed();
    if !reverse.is_empty() {
        if let Some(map) = relationship.as_object_mut() {
            map.insert(
                "reversePropertyName".into(),
                Value::String(reverse.to_string()),
            );
            map.insert("reverseRelationship".into(), Value::Bool(true));
        }
    }

    if draft.collection {
        json!({
            "type": "array",
            "title": title,
            "description": description,
            "items": relationship,
            "returnByDefault": false,
            "searchable": false,
            "userEditable": true,
            "viewable": true
        })
    } else {
        let mut property = relationship;
        if let Some(map) = property.as_object_mut() {
            map.insert("returnByDefault".into(), Value::Bool(false));
            map.insert("searchable".into(), Value::Bool(false));
            map.insert("userEditable".into(), Value::Bool(true));
            map.insert("viewable".into(), Value::Bool(true));
        }
        property
    }
}

fn title_from_key(key: &str) -> String {
    key.trim_start_matches("custom_")
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn set_optional_string(map: &mut serde_json::Map<String, Value>, key: &str, value: &str) {
    if value.is_empty() {
        map.remove(key);
    } else {
        map.insert(key.into(), Value::String(value.to_string()));
    }
}

fn insert_new_property(
    object: &mut Value,
    field_key: &str,
    property: Value,
    required: bool,
) -> Result<(), String> {
    ensure_property_available(object, field_key, None)?;
    properties_mut(object)?.insert(field_key.to_string(), property);
    append_order_key(object, field_key)?;
    set_required(object, field_key, required)?;
    Ok(())
}

fn upsert_property(
    object: &mut Value,
    old_key: &str,
    new_key: &str,
    property: Value,
) -> Result<(), String> {
    ensure_property_available(object, new_key, Some(old_key))?;
    let properties = properties_mut(object)?;
    if old_key == new_key {
        properties.insert(old_key.to_string(), property);
        return Ok(());
    }
    properties.remove(old_key);
    properties.insert(new_key.to_string(), property);
    rename_order_key(object, old_key, new_key)?;
    rename_required_key(object, old_key, new_key)?;
    Ok(())
}

fn ensure_property_available(
    object: &Value,
    field_key: &str,
    current_key: Option<&str>,
) -> Result<(), String> {
    let Some(properties) = crate::managed::state::properties(object) else {
        return Ok(());
    };
    if properties.contains_key(field_key) && current_key != Some(field_key) {
        return Err(format!("field '{field_key}' already exists"));
    }
    Ok(())
}

fn properties_mut(object: &mut Value) -> Result<&mut serde_json::Map<String, Value>, String> {
    let schema = schema_mut(object)?;
    schema
        .entry("properties".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "managed object has no schema.properties map".to_string())
}

fn schema_mut(object: &mut Value) -> Result<&mut serde_json::Map<String, Value>, String> {
    let map = object
        .as_object_mut()
        .ok_or_else(|| "managed object is not an object".to_string())?;
    map.entry("schema".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "managed object has no schema object".to_string())
}

fn append_order_key(object: &mut Value, field_key: &str) -> Result<(), String> {
    let schema = schema_mut(object)?;
    let order_value = schema
        .entry("order".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let order = order_value
        .as_array_mut()
        .ok_or_else(|| "schema.order is not an array".to_string())?;
    if !order.iter().any(|value| value.as_str() == Some(field_key)) {
        order.push(Value::String(field_key.to_string()));
    }
    Ok(())
}

fn rename_order_key(object: &mut Value, old_key: &str, new_key: &str) -> Result<(), String> {
    let schema = schema_mut(object)?;
    let order_value = schema
        .entry("order".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let order = order_value
        .as_array_mut()
        .ok_or_else(|| "schema.order is not an array".to_string())?;
    let mut replaced = false;
    for value in order.iter_mut() {
        if value.as_str() == Some(old_key) {
            *value = Value::String(new_key.to_string());
            replaced = true;
        }
    }
    order.retain(|value| value.as_str() != Some(old_key));
    if !replaced && !order.iter().any(|value| value.as_str() == Some(new_key)) {
        order.push(Value::String(new_key.to_string()));
    }
    dedupe_string_array(order);
    Ok(())
}

fn remove_order_key(object: &mut Value, field_key: &str) -> Result<(), String> {
    let Some(order) = object
        .pointer_mut("/schema/order")
        .and_then(Value::as_array_mut)
    else {
        return Ok(());
    };
    order.retain(|value| value.as_str() != Some(field_key));
    Ok(())
}

fn rename_required_key(object: &mut Value, old_key: &str, new_key: &str) -> Result<(), String> {
    let Some(required) = object
        .pointer_mut("/schema/required")
        .and_then(Value::as_array_mut)
    else {
        return Ok(());
    };
    for value in required.iter_mut() {
        if value.as_str() == Some(old_key) {
            *value = Value::String(new_key.to_string());
        }
    }
    required.retain(|value| value.as_str() != Some(old_key));
    dedupe_string_array(required);
    Ok(())
}

fn dedupe_string_array(values: &mut Vec<Value>) {
    let mut seen = std::collections::HashSet::new();
    values.retain(|value| match value.as_str() {
        Some(text) => seen.insert(text.to_string()),
        None => true,
    });
}

fn set_required(object: &mut Value, field_key: &str, required: bool) -> Result<(), String> {
    let schema = schema_mut(object)?;
    let required_value = schema
        .entry("required".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let required_array = required_value
        .as_array_mut()
        .ok_or_else(|| "schema.required is not an array".to_string())?;

    if required {
        if !required_array
            .iter()
            .any(|value| value.as_str() == Some(field_key))
        {
            required_array.push(Value::String(field_key.to_string()));
        }
    } else {
        required_array.retain(|value| value.as_str() != Some(field_key));
    }
    Ok(())
}

pub fn commit_edit(app: &mut App) {
    let Some(plan) = build_edit_field_plan(app) else {
        return;
    };
    submit_plan(app, plan);
}

pub fn commit_add_field(app: &mut App) {
    let Some(plan) = build_add_field_plan(app) else {
        return;
    };
    submit_plan(app, plan);
}

pub fn commit_add_relationship(app: &mut App) {
    let Some(plan) = build_add_relationship_plan(app) else {
        return;
    };
    submit_plan(app, plan);
}

pub fn commit_add_hook(app: &mut App) {
    let Some(plan) = build_add_hook_plan(app) else {
        return;
    };
    submit_plan(app, plan);
}

pub fn commit_delete_field(app: &mut App) {
    let Some(plan) = build_delete_field_plan(app) else {
        return;
    };
    submit_plan(app, plan);
}

/// Submits the active property-key rename through the normal update pipeline.
pub fn commit_rename_field(app: &mut App) {
    let Some(plan) = build_rename_field_plan(app) else {
        return;
    };
    submit_plan(app, plan);
}

fn submit_plan(app: &mut App, plan: ObjectReplacePlan) {
    let is_prod = app
        .active_tenant()
        .is_some_and(|tenant| tenant.theme == TenantTheme::Production);
    if is_prod {
        app.prod_confirm.pending = Some(PendingProdAction::Managed(ProdAction::Update(Box::new(
            plan,
        ))));
        app.input_mode = InputMode::ProdConfirm;
        return;
    }
    execute_update_plan(app, plan, false);
}

pub fn cancel_active_draft(app: &mut App) {
    app.managed.clear_active_drafts();
    app.input_mode = InputMode::Normal;
}

pub fn advance_focus(edit: &mut FieldEditState, forward: bool) {
    edit.focused = if forward {
        edit.focused.next(edit.caps)
    } else {
        edit.focused.prev(edit.caps)
    };
}

pub fn advance_add_field_focus(draft: &mut AddFieldState, forward: bool) {
    draft.focused = if forward {
        draft.focused.next()
    } else {
        draft.focused.prev()
    };
}

pub fn advance_add_relationship_focus(draft: &mut AddRelationshipState, forward: bool) {
    draft.focused = if forward {
        draft.focused.next()
    } else {
        draft.focused.prev()
    };
}

/// Advances the rename draft's single focusable field.
pub fn advance_rename_field_focus(_draft: &mut RenameFieldState, _forward: bool) {}

pub fn handle_enter_in_edit(app: &mut App) {
    let Some(focused) = app.managed.editing.as_ref().map(|edit| edit.focused) else {
        return;
    };
    match focused {
        EditFieldFocus::Save => commit_edit(app),
        focus if focus.is_bool() => {
            if let Some(edit) = app.managed.editing.as_mut() {
                edit.toggle_focused_bool();
                edit.error = None;
            }
        }
        _ => {
            if let Some(edit) = app.managed.editing.as_mut() {
                advance_focus(edit, true);
            }
        }
    }
}

pub fn execute_prod_action(app: &mut App, action: ProdAction) {
    match action {
        ProdAction::Update(plan) => execute_update_plan(app, *plan, true),
        ProdAction::RenameObject(request) => execute_rename_object(app, *request, true),
        ProdAction::CreateObject(request) => execute_create_object(app, *request, true),
        ProdAction::Undo(undo_id) => execute_undo(app, undo_id, true),
    }
}

pub fn resume_mode(app: &App, _action: &ProdAction) -> InputMode {
    crate::managed::screen::resume_mode_after_prod_cancel(app)
        .map(InputMode::Managed)
        .unwrap_or(InputMode::Normal)
}

pub fn describe_prod_action(_action: &ProdAction) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use crate::undo::{MemoryLog, UndoLog};
    use serde_json::json;

    use super::*;

    fn edit_state(required: bool) -> FieldEditState {
        let required_value = if required {
            json!(["custom_code"])
        } else {
            json!([])
        };
        let object = json!({
            "name": "alpha_user",
            "type": "managed",
            "meta": {},
            "schema": {
                "properties": {
                    "custom_code": {
                        "title": "Old",
                        "description": "Old desc",
                        "type": "string",
                        "searchable": false,
                        "viewable": true,
                        "userEditable": false
                    }
                },
                "required": required_value,
                "order": ["custom_code"]
            }
        });
        let property = object["schema"]["properties"]["custom_code"].clone();
        FieldEditState::from_property(
            "sandbox".into(),
            "alpha_user".into(),
            "custom_code".into(),
            object,
            property,
            required,
        )
    }

    fn managed_doc(object: Value) -> Value {
        json!({ "objects": [object] })
    }

    fn cached_object<'a>(
        state: &'a crate::managed::state::State,
        tenant: &str,
        object_name: &str,
    ) -> &'a Value {
        let LoadState::Loaded(doc) = state.data.get(tenant).unwrap() else {
            panic!("managed cache is not loaded");
        };
        crate::managed::api::object_named(doc, object_name).unwrap()
    }

    #[test]
    fn field_edit_updates_property_and_required_without_touching_order() {
        let mut edit = edit_state(false);
        edit.title.set("New");
        edit.description.set("New desc");
        edit.required = true;
        edit.searchable = true;
        edit.user_editable = true;

        let object = apply_field_edit(&edit).unwrap().object;
        let property = &object["schema"]["properties"]["custom_code"];
        assert_eq!(property["title"], json!("New"));
        assert_eq!(property["description"], json!("New desc"));
        assert_eq!(property["searchable"], json!(true));
        assert_eq!(property["viewable"], json!(true));
        assert_eq!(property["userEditable"], json!(true));
        assert_eq!(object["schema"]["required"], json!(["custom_code"]));
        assert_eq!(object["schema"]["order"], json!(["custom_code"]));
    }

    #[test]
    fn field_edit_removes_required_when_cleared() {
        let mut edit = edit_state(true);
        edit.required = false;

        let object = apply_field_edit(&edit).unwrap().object;
        assert_eq!(object["schema"]["required"], json!([]));
    }

    #[test]
    fn pure_attribute_edit_leaves_missing_order_entry_unchanged() {
        let object = json!({
            "name": "alpha_user",
            "type": "managed",
            "meta": {},
            "schema": {
                "properties": {
                    "custom_code": {
                        "title": "Old",
                        "type": "string",
                        "searchable": false,
                        "viewable": true,
                        "userEditable": false
                    }
                },
                "required": [],
                "order": ["givenName"]
            }
        });
        let property = object["schema"]["properties"]["custom_code"].clone();
        let mut edit = FieldEditState::from_property(
            "sandbox".into(),
            "alpha_user".into(),
            "custom_code".into(),
            object,
            property,
            false,
        );
        edit.title.set("New");

        let object = apply_field_edit(&edit).unwrap().object;
        assert_eq!(
            object["schema"]["properties"]["custom_code"]["title"],
            json!("New")
        );
        assert_eq!(object["schema"]["order"], json!(["givenName"]));
    }

    #[test]
    fn add_field_auto_prefixes_standard_object_custom_key() {
        let object = json!({
            "name": "alpha_user",
            "type": "managed",
            "meta": {},
            "schema": {"properties": {}, "required": [], "order": []}
        });
        let mut draft = AddFieldState::new("sandbox".into(), "alpha_user".into(), object);
        draft.key.set("loyaltyId");
        draft.title.set("Loyalty ID");
        draft.required = true;

        let applied = apply_add_field(&draft).unwrap();
        assert_eq!(applied.field_key, "custom_loyaltyId");
        assert!(applied.object["schema"]["properties"]["custom_loyaltyId"].is_object());
        assert!(applied.object["schema"]["properties"]["loyaltyId"].is_null());
        assert_eq!(
            applied.object["schema"]["order"],
            json!(["custom_loyaltyId"])
        );
        assert_eq!(
            applied.object["schema"]["required"],
            json!(["custom_loyaltyId"])
        );
    }

    #[test]
    fn add_field_materializes_missing_schema() {
        let object = json!({"name": "test_empty"});
        let mut draft = AddFieldState::new("sandbox".into(), "test_empty".into(), object);
        draft.key.set("first");

        let applied = apply_add_field(&draft).unwrap();
        assert!(applied.object["schema"].is_object());
        assert!(applied.object["schema"]["properties"]["first"].is_object());
        assert_eq!(applied.object["schema"]["order"], json!(["first"]));
    }

    #[test]
    fn add_field_materializes_missing_properties() {
        let object = json!({"name": "test_empty", "schema": {}});
        let mut draft = AddFieldState::new("sandbox".into(), "test_empty".into(), object);
        draft.key.set("first");

        let applied = apply_add_field(&draft).unwrap();
        assert!(applied.object["schema"]["properties"]["first"].is_object());
        assert_eq!(applied.object["schema"]["order"], json!(["first"]));
    }

    #[test]
    fn add_relationship_builds_single_relationship_shape() {
        let object = json!({
            "name": "alpha_lock",
            "schema": {"properties": {}, "required": [], "order": []}
        });
        let mut draft = AddRelationshipState::new("sandbox".into(), "alpha_lock".into(), object);
        draft.key.set("owner");
        draft.title.set("Owner");
        draft.description.set("Owning user");
        draft.target_name = Some("alpha_user".into());
        draft.reverse_property_name.set("locks");

        let applied = apply_add_relationship(&draft).unwrap();
        let property = &applied.object["schema"]["properties"]["owner"];
        assert_eq!(property["type"], json!("relationship"));
        assert_eq!(
            property["resourceCollection"],
            json!([{"path": "managed/alpha_user"}])
        );
        assert_eq!(property["title"], json!("Owner"));
        assert_eq!(property["description"], json!("Owning user"));
        assert_eq!(property["validate"], json!(true));
        assert_eq!(property["reversePropertyName"], json!("locks"));
        assert_eq!(property["reverseRelationship"], json!(true));
        assert_eq!(property["returnByDefault"], json!(false));
        assert_eq!(property["userEditable"], json!(true));
        assert_eq!(property["properties"]["_ref"]["type"], json!("string"));
        assert_eq!(
            property["properties"]["_refProperties"]["properties"]["_id"]["type"],
            json!("string")
        );
        assert_eq!(applied.object["schema"]["order"], json!(["owner"]));
    }

    #[test]
    fn add_relationship_builds_array_relationship_shape() {
        let object = json!({
            "name": "alpha_lock",
            "schema": {"properties": {}, "required": [], "order": []}
        });
        let mut draft = AddRelationshipState::new("sandbox".into(), "alpha_lock".into(), object);
        draft.key.set("owners");
        draft.target_name = Some("alpha_user".into());
        draft.collection = true;
        draft.validate = false;

        let applied = apply_add_relationship(&draft).unwrap();
        let property = &applied.object["schema"]["properties"]["owners"];
        assert_eq!(property["type"], json!("array"));
        assert_eq!(property["returnByDefault"], json!(false));
        assert_eq!(property["userEditable"], json!(true));
        assert_eq!(property["items"]["type"], json!("relationship"));
        assert_eq!(
            property["items"]["resourceCollection"],
            json!([{"path": "managed/alpha_user"}])
        );
        assert_eq!(property["items"]["validate"], json!(false));
        assert!(property["items"]["reversePropertyName"].is_null());
        assert_eq!(
            property["items"]["properties"]["_refProperties"]["type"],
            json!("object")
        );
    }

    #[test]
    fn add_hook_inserts_top_level_sibling_and_preserves_existing() {
        let object = json!({
            "name": "alpha_lock",
            "schema": {"properties": {}, "required": [], "order": []}
        });

        let added = apply_add_hook(&object, "alpha_lock", "onCreate").unwrap();
        assert_eq!(added["onCreate"]["type"], json!("text/javascript"));
        assert_eq!(
            added["onCreate"]["source"],
            json!("// onCreate for alpha_lock\n")
        );
        assert!(added["schema"].is_object());

        let existing = json!({
            "name": "alpha_lock",
            "schema": {"properties": {}, "required": [], "order": []},
            "onCreate": {"type": "text/javascript", "source": "old();"}
        });
        let unchanged = apply_add_hook(&existing, "alpha_lock", "onCreate").unwrap();
        assert_eq!(unchanged, existing);
    }

    #[test]
    fn rename_custom_field_keeps_order_and_required_in_sync() {
        let mut edit = edit_state(true);
        edit.key.set("custom_new_code");

        let object = apply_field_edit(&edit).unwrap().object;
        assert!(object["schema"]["properties"]["custom_code"].is_null());
        assert!(object["schema"]["properties"]["custom_new_code"].is_object());
        assert_eq!(object["schema"]["order"], json!(["custom_new_code"]));
        assert_eq!(object["schema"]["required"], json!(["custom_new_code"]));
    }

    #[test]
    fn rename_field_plan_mutation_updates_order_required_and_rejects_collisions() {
        let object = json!({
            "name": "alpha_lock",
            "schema": {
                "properties": {
                    "before": {"type": "string"},
                    "old_key": {"type": "string"},
                    "after": {"type": "string"}
                },
                "order": ["before", "old_key", "after"],
                "required": ["old_key"]
            }
        });
        let mut rename = RenameFieldState::new(
            "sandbox".into(),
            "alpha_lock".into(),
            "old_key".into(),
            object,
        );
        rename.key.set("new_key");

        let renamed = apply_rename_field(&rename, &rename.key.value).unwrap();
        let properties = renamed["schema"]["properties"].as_object().unwrap();
        assert!(properties.contains_key("new_key"));
        assert!(!properties.contains_key("old_key"));
        // Field display order is governed by schema.order, not properties key
        // position, so we only assert order/required are kept in sync.
        assert_eq!(
            renamed["schema"]["order"],
            json!(["before", "new_key", "after"])
        );
        assert_eq!(renamed["schema"]["required"], json!(["new_key"]));

        rename.key.set("after");
        let error = apply_rename_field(&rename, &rename.key.value).unwrap_err();
        assert_eq!(error, "field 'after' already exists");
    }

    #[test]
    fn delete_custom_field_keeps_order_and_required_in_sync() {
        let edit = edit_state(true);
        let pending = DeleteFieldState {
            tenant_name: "sandbox".into(),
            object_name: "alpha_user".into(),
            field_key: "custom_code".into(),
            original_object: edit.original_object,
            is_relationship: false,
        };

        let object = apply_delete_field(&pending).unwrap();
        assert!(object["schema"]["properties"]["custom_code"].is_null());
        assert_eq!(object["schema"]["order"], json!([]));
        assert_eq!(object["schema"]["required"], json!([]));
    }

    #[test]
    fn delete_standard_field_is_blocked_by_capability() {
        let object = json!({
            "name": "alpha_user",
            "type": "managed",
            "meta": {},
            "schema": {
                "properties": {"givenName": {"type": "string"}},
                "required": ["givenName"],
                "order": ["givenName"]
            }
        });
        let pending = DeleteFieldState {
            tenant_name: "sandbox".into(),
            object_name: "alpha_user".into(),
            field_key: "givenName".into(),
            original_object: object,
            is_relationship: false,
        };

        let error = apply_delete_field(&pending).unwrap_err();
        assert!(error.contains("Standard fields"));
    }

    #[test]
    fn failed_update_reverts_cache_and_expires_pending_undo() {
        let previous = json!({
            "name": "alpha_user",
            "type": "managed",
            "meta": {},
            "schema": {
                "properties": {
                    "custom_code": {"title": "Old", "type": "string"}
                },
                "required": [],
                "order": ["custom_code"]
            }
        });
        let optimistic = json!({
            "name": "alpha_user",
            "type": "managed",
            "meta": {},
            "schema": {
                "properties": {
                    "custom_code": {"title": "Unsaved", "type": "string"}
                },
                "required": [],
                "order": ["custom_code"]
            }
        });
        let mut state = crate::managed::state::State::new();
        state.data.insert(
            "sandbox".into(),
            LoadState::Loaded(managed_doc(optimistic.clone())),
        );
        state
            .in_flight_writes
            .insert(("sandbox".into(), "alpha_user".into()));

        let mut undo = MemoryLog::new();
        let undo_id = undo
            .record(UndoEntry::pending(
                "sandbox".to_string(),
                "managed",
                "Revert managed object alpha_user",
                Sensitivity::TenantConfig,
                Capability::Undoable,
                Some(UndoOp::ManagedObjectReplace {
                    tenant: "sandbox".to_string(),
                    object_name: "alpha_user".to_string(),
                    body: previous.clone(),
                }),
                ConflictCheck::ContentEqualsAfter { body: optimistic },
            ))
            .unwrap();

        revert_failed_update(
            &mut state,
            &mut undo,
            "sandbox",
            "alpha_user",
            undo_id,
            previous.clone(),
        )
        .unwrap();

        assert!(crate::managed::api::object_content_equal(
            cached_object(&state, "sandbox", "alpha_user"),
            &previous
        ));
        let resnapshot = cached_object(&state, "sandbox", "alpha_user").clone();
        assert!(crate::managed::api::object_content_equal(
            &resnapshot,
            &previous
        ));
        assert!(
            state
                .failed_writes
                .contains(&("sandbox".into(), "alpha_user".into()))
        );
        assert_eq!(undo.load(undo_id).unwrap().status, EntryStatus::Expired);
    }

    #[test]
    fn failed_update_does_not_reinsert_object_missing_from_cache() {
        let previous = json!({
            "name": "alpha_user",
            "schema": {"properties": {}, "required": [], "order": []}
        });
        let mut state = crate::managed::state::State::new();
        state
            .data
            .insert("sandbox".into(), LoadState::Loaded(json!({"objects": []})));
        let mut undo = MemoryLog::new();
        let undo_id = undo
            .record(UndoEntry::pending(
                "sandbox".to_string(),
                "managed",
                "Revert managed object alpha_user",
                Sensitivity::TenantConfig,
                Capability::Undoable,
                Some(UndoOp::ManagedObjectReplace {
                    tenant: "sandbox".to_string(),
                    object_name: "alpha_user".to_string(),
                    body: previous.clone(),
                }),
                ConflictCheck::ContentEqualsAfter { body: json!({}) },
            ))
            .unwrap();

        revert_failed_update(
            &mut state,
            &mut undo,
            "sandbox",
            "alpha_user",
            undo_id,
            previous,
        )
        .unwrap();

        let LoadState::Loaded(doc) = state.data.get("sandbox").unwrap() else {
            panic!("managed cache is not loaded");
        };
        assert!(crate::managed::api::objects(doc).unwrap().is_empty());
        assert_eq!(undo.load(undo_id).unwrap().status, EntryStatus::Expired);
    }

    #[test]
    fn rename_object_repoints_scalar_and_array_relationships() {
        let doc = json!({"objects": [
            {"name": "A", "schema": {"properties": {}}},
            {"name": "B", "schema": {"properties": {"a": {"type": "relationship", "resourceCollection": [{"path": "managed/A"}]}}}},
            {"name": "C", "schema": {"properties": {"as": {"type": "array", "items": {"type": "relationship", "resourceCollection": [{"path": "managed/A"}]}}}}}
        ]});
        let (renamed, count) = rename_object_in_doc(&doc, "A", "A2").unwrap();
        assert_eq!(count, 2);
        assert_eq!(renamed["objects"][0]["name"], "A2");
        assert_eq!(
            renamed["objects"][1]["schema"]["properties"]["a"]["resourceCollection"][0]["path"],
            "managed/A2"
        );
        assert_eq!(
            renamed["objects"][2]["schema"]["properties"]["as"]["items"]["resourceCollection"][0]["path"],
            "managed/A2"
        );
    }

    #[test]
    fn rename_object_rejects_collision_and_missing_source() {
        let doc = json!({"objects": [{"name": "A"}, {"name": "B"}]});
        assert!(rename_object_in_doc(&doc, "A", "B").is_err());
        assert!(rename_object_in_doc(&doc, "missing", "C").is_err());
    }

    #[test]
    fn create_object_appends_minimal_shape() {
        let doc = json!({"objects": []});
        let created = create_object_in_doc(&doc, "test_object", "Test object", "").unwrap();
        let object = &created["objects"][0];
        assert_eq!(object["name"], json!("test_object"));
        assert_eq!(object["schema"]["type"], json!("object"));
        assert_eq!(object["schema"]["title"], json!("Test object"));
        assert_eq!(object["schema"]["properties"], json!({}));
        assert_eq!(object["schema"]["required"], json!([]));
        assert_eq!(object["schema"]["order"], json!([]));
        assert!(object["schema"].get("description").is_none());
    }

    #[test]
    fn create_object_includes_description_and_falls_back_to_name_for_title() {
        let doc = json!({"objects": []});
        let created = create_object_in_doc(&doc, "test_object", "", "Description").unwrap();
        assert_eq!(
            created["objects"][0]["schema"]["title"],
            json!("test_object")
        );
        assert_eq!(
            created["objects"][0]["schema"]["description"],
            json!("Description")
        );
    }

    #[test]
    fn create_object_rejects_name_collision() {
        let doc = json!({"objects": [{"name": "test_object"}]});
        assert!(create_object_in_doc(&doc, "test_object", "", "").is_err());
    }
}
