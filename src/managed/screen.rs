//! Managed-objects tab interaction: lazy loading, fuzzy search, selection, and
//! in-TUI schema edits routed through the managed object-replace engine.

use serde_json::Value;

use crate::app::event::AppEvent;
use crate::app::event::ToastKind;
use crate::app::{App, InputMode};
use crate::managed::ops;
use crate::managed::state::{
    AddChooseState, AddFieldState, AddHookState, DeleteFieldState, DeleteObjectState,
    FieldEditState, LoadState, NewObjectState, RelationshipFormState, RenameFieldState,
    RenameObjectConfirmState, RenameObjectState,
};

pub use super::keys::{footer_hints, handle_key, help_lines};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Search,
    EditField,
    AddChooseKind,
    AddField,
    Relationship,
    RelationshipTarget,
    RefProp,
    AddHook,
    EnumNarrowConfirm,
    DeleteFieldConfirm,
    DeleteObjectConfirm,
    RenameField,
    RenameObject,
    RenameObjectConfirm,
    NewObject,
}

#[derive(Debug)]
pub enum Event {
    Listed {
        tenant: String,
        result: std::result::Result<Value, String>,
    },
    UpdateResult {
        tenant: String,
        object_name: String,
        undo_id: crate::undo::UndoId,
        previous_object: Value,
        result: std::result::Result<ops::UpdateOutcome, String>,
    },
    UndoResult {
        undo_id: crate::undo::UndoId,
        tenant: String,
        result: std::result::Result<ops::UndoOutcome, ops::UndoFailure>,
    },
    RenameRecordCount {
        draft: RenameObjectState,
        result: std::result::Result<crate::managed::api::RecordCount, String>,
    },
    DeleteObjectRecordCount {
        draft: DeleteObjectState,
        result: std::result::Result<crate::managed::api::RecordCount, String>,
    },
    RenameResult {
        tenant: String,
        old_name: String,
        new_name: String,
        undo_id: crate::undo::UndoId,
        result: std::result::Result<Value, String>,
    },
    DeleteObjectResult {
        tenant: String,
        object_name: String,
        inbound_count: usize,
        undo_id: crate::undo::UndoId,
        result: std::result::Result<Value, String>,
    },
    CreateResult {
        tenant: String,
        name: String,
        undo_id: crate::undo::UndoId,
        result: std::result::Result<Value, String>,
    },
    RelationshipResult {
        tenant: String,
        source_object: String,
        key: String,
        undo_id: crate::undo::UndoId,
        result: std::result::Result<Value, String>,
    },
}

pub fn apply_event(app: &mut App, event: Event) {
    match event {
        Event::Listed { tenant, result } => {
            app.managed.refreshing.remove(&tenant);
            match result {
                Ok(doc) => {
                    app.managed
                        .data
                        .insert(tenant.clone(), LoadState::Loaded(doc));
                }
                Err(error) => {
                    app.managed
                        .data
                        .insert(tenant.clone(), LoadState::Failed(error));
                }
            }
            if app
                .active_tenant()
                .is_some_and(|active| active.name == tenant)
            {
                let count = app.managed.matches(Some(&tenant)).len();
                app.managed.clamp_selection(count);
                clamp_selected_property(app);
            }
        }
        Event::UpdateResult {
            tenant,
            object_name,
            undo_id,
            previous_object,
            result,
        } => ops::apply_update_result(app, tenant, object_name, undo_id, previous_object, result),
        Event::UndoResult {
            undo_id,
            tenant,
            result,
        } => ops::apply_undo_result(app, undo_id, tenant, result),
        Event::RenameRecordCount { draft, result } => {
            if app.input_mode != InputMode::Managed(Mode::RenameObject)
                || app.managed.renaming_object.is_some()
            {
                return;
            }
            let repoints =
                ops::rename_object_in_doc(&draft.original_doc, &draft.old_name, &draft.key.value)
                    .map_or(0, |(_, count)| count);
            let (record_count, count_error) = match result {
                Ok(count) => (Some(count), None),
                Err(error) => (None, Some(error)),
            };
            app.managed.rename_object_confirm = Some(RenameObjectConfirmState {
                draft,
                repoints,
                record_count,
                count_error,
            });
            app.input_mode = InputMode::Managed(Mode::RenameObjectConfirm);
        }
        Event::DeleteObjectRecordCount { mut draft, result } => {
            if app.input_mode != InputMode::Managed(Mode::DeleteObjectConfirm)
                || app.managed.pending_object_delete.is_none()
            {
                return;
            }
            draft.record_count = Some(result);
            app.managed.pending_object_delete = Some(draft);
        }
        Event::RenameResult {
            tenant,
            old_name,
            new_name,
            undo_id,
            result,
        } => ops::apply_rename_result(app, tenant, old_name, new_name, undo_id, result),
        Event::DeleteObjectResult {
            tenant,
            object_name,
            inbound_count,
            undo_id,
            result,
        } => ops::apply_delete_object_result(
            app,
            tenant,
            object_name,
            inbound_count,
            undo_id,
            result,
        ),
        Event::CreateResult {
            tenant,
            name,
            undo_id,
            result,
        } => ops::apply_create_result(app, tenant, name, undo_id, result),
        Event::RelationshipResult {
            tenant,
            source_object,
            key,
            undo_id,
            result,
        } => ops::apply_relationship_result(app, tenant, source_object, key, undo_id, result),
    }
}

pub fn resume_mode_after_prod_cancel(app: &App) -> Option<Mode> {
    if app.managed.editing.is_some() {
        Some(Mode::EditField)
    } else if app.managed.add_choose.is_some() {
        Some(Mode::AddChooseKind)
    } else if app.managed.add_field.is_some() {
        Some(Mode::AddField)
    } else if app.managed.ref_prop_draft.is_some() {
        Some(Mode::RefProp)
    } else if app.managed.relationship_form.is_some() {
        Some(Mode::Relationship)
    } else if app.managed.add_hook.is_some() {
        Some(Mode::AddHook)
    } else if app.managed.pending_delete.is_some() {
        Some(Mode::DeleteFieldConfirm)
    } else if app.managed.pending_object_delete.is_some() {
        Some(Mode::DeleteObjectConfirm)
    } else if app.managed.renaming.is_some() {
        Some(Mode::RenameField)
    } else if app.managed.renaming_object.is_some() {
        Some(Mode::RenameObject)
    } else if app.managed.rename_object_confirm.is_some() {
        Some(Mode::RenameObjectConfirm)
    } else if app.managed.new_object.is_some() {
        Some(Mode::NewObject)
    } else {
        None
    }
}

/// Saves an edited field, interposing a warning only when its enum loses
/// values. The confirmed path still uses `ops::commit_edit`.
pub fn request_edit_save(app: &mut App) {
    let Some(edit) = app.managed.editing.as_mut() else {
        return;
    };
    let change = match edit.enum_change() {
        Ok(change) => change,
        Err(error) => {
            edit.error = Some(error);
            return;
        }
    };
    let removed = ops::removed_enum_values(&edit.original_property, &change);
    if !edit.allow_narrowing && !removed.is_empty() {
        edit.narrowed_enum_values = removed;
        app.input_mode = InputMode::Managed(Mode::EnumNarrowConfirm);
        return;
    }
    ops::commit_edit(app);
}

pub fn refresh(app: &mut App, force: bool) {
    let Some(name) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if !app.is_unlocked()
        || app.managed.refreshing.contains(&name)
        || (!force && app.managed.data.contains_key(&name))
    {
        return;
    }

    app.managed.data.insert(name.clone(), LoadState::Loading);
    app.managed.refreshing.insert(name.clone());

    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::managed::api::get_managed(&name)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Managed(Event::Listed {
            tenant: name,
            result,
        }));
    });
}

pub fn row_count(app: &App) -> usize {
    app.managed
        .matches(app.active_tenant().map(|t| t.name.as_str()))
        .len()
}

pub fn current_selection(app: &App) -> usize {
    app.managed.selected
}

pub fn set_selection(app: &mut App, idx: usize) {
    app.managed.select_object(idx);
}

pub fn filter_active(app: &App) -> bool {
    !app.managed.query.is_empty()
}

pub fn clear_filter(app: &mut App) {
    app.managed.reset_view();
}

pub fn primary(app: &mut App) {
    start_edit_field(app);
}

pub fn delete(app: &mut App) {
    request_delete_field(app);
}

pub fn new_item(app: &mut App) {
    start_add_choose(app);
}

pub fn move_property(app: &mut App, delta: isize) {
    let Some((_, object)) = selected_object(app) else {
        return;
    };
    let n = crate::managed::state::property_names(object).len();
    if n == 0 {
        return;
    }
    let cur = app.managed.property_selected.min(n - 1) as isize;
    let next = (cur + delta).clamp(0, n as isize - 1) as usize;
    app.managed.property_selected = next;
}

pub fn start_edit_field(app: &mut App) {
    let Some((tenant_name, object_name, object, field_key, property, required)) =
        selected_property(app)
    else {
        app.push_toast(ToastKind::Info, "Selected object has no fields");
        return;
    };
    if write_in_flight(app, &tenant_name, &object_name) {
        return;
    }
    if crate::managed::state::is_relationship_property(&property) {
        start_relationship_edit(app);
        return;
    }

    app.managed.editing = Some(FieldEditState::from_property(
        tenant_name,
        object_name,
        field_key,
        object,
        property,
        required,
    ));
    app.input_mode = InputMode::Managed(Mode::EditField);
}

pub fn start_add_field(app: &mut App) {
    let Some((tenant_name, object_name, object)) = selected_object_with_tenant(app) else {
        return;
    };
    if write_in_flight(app, &tenant_name, &object_name) {
        return;
    }
    app.managed.add_field = Some(AddFieldState::new(tenant_name, object_name, object));
    app.input_mode = InputMode::Managed(Mode::AddField);
}

/// Opens the managed-property kind chooser before either add form.
pub fn start_add_choose(app: &mut App) {
    let Some((tenant_name, object_name, _)) = selected_object_with_tenant(app) else {
        return;
    };
    if write_in_flight(app, &tenant_name, &object_name) {
        return;
    }
    app.managed.add_choose = Some(AddChooseState::default());
    app.input_mode = InputMode::Managed(Mode::AddChooseKind);
}

pub fn start_relationship_create(app: &mut App) {
    let Some((tenant_name, object_name, _)) = selected_object_with_tenant(app) else {
        return;
    };
    if write_in_flight(app, &tenant_name, &object_name) {
        return;
    }
    let Some(LoadState::Loaded(doc)) = app.managed.data.get(&tenant_name) else {
        return;
    };
    app.managed.relationship_form = Some(RelationshipFormState::new_create(
        tenant_name,
        object_name,
        doc.clone(),
    ));
    app.input_mode = InputMode::Managed(Mode::Relationship);
}

pub fn start_relationship_edit(app: &mut App) {
    let Some((tenant_name, object_name, _, key, property, _)) = selected_property(app) else {
        return;
    };
    if write_in_flight(app, &tenant_name, &object_name) {
        return;
    }
    let Some(LoadState::Loaded(doc)) = app.managed.data.get(&tenant_name) else {
        return;
    };
    let Some(form) =
        RelationshipFormState::edit(tenant_name, object_name, doc.clone(), key, property)
    else {
        app.push_toast(ToastKind::Error, "Could not parse relationship");
        return;
    };
    app.managed.relationship_form = Some(form);
    app.input_mode = InputMode::Managed(Mode::Relationship);
}

/// Opens a key-only rename draft when the selected property's capabilities allow it.
pub fn start_rename_field(app: &mut App) {
    let Some((tenant_name, object_name, object, field_key, property, _)) = selected_property(app)
    else {
        app.push_toast(ToastKind::Info, "Selected object has no fields");
        return;
    };
    if write_in_flight(app, &tenant_name, &object_name) {
        return;
    }
    let caps = crate::managed::state::field_capability_for_property(&object, &field_key, &property);
    if !caps.rename_key {
        let message = if crate::managed::state::is_relationship_property(&property) {
            "Relationship keys cannot be renamed; delete and recreate the relationship"
        } else {
            "Standard field keys cannot be renamed"
        };
        app.push_toast(ToastKind::Info, message);
        return;
    }
    app.managed.renaming = Some(RenameFieldState::new(
        tenant_name,
        object_name,
        field_key,
        object,
    ));
    app.input_mode = InputMode::Managed(Mode::RenameField);
}

/// Starts the whole-document object rename flow for a custom object only.
pub fn start_rename_object(app: &mut App) {
    let Some((tenant_name, object_name, object)) = selected_object_with_tenant(app) else {
        return;
    };
    if write_in_flight(app, &tenant_name, &object_name) {
        return;
    }
    if crate::managed::state::is_ping_shipped_object(&object) {
        app.push_toast(
            ToastKind::Info,
            "Ping-shipped objects cannot be renamed; create a custom replacement instead",
        );
        return;
    }
    let Some(LoadState::Loaded(doc)) = app.managed.data.get(&tenant_name) else {
        return;
    };
    app.managed.renaming_object = Some(RenameObjectState::new(
        tenant_name,
        object_name,
        doc.clone(),
    ));
    app.input_mode = InputMode::Managed(Mode::RenameObject);
}

/// Starts a guarded whole-document object delete for custom objects only.
pub fn start_delete_object(app: &mut App) {
    let Some((tenant_name, object_name, object)) = selected_object_with_tenant(app) else {
        return;
    };
    if write_in_flight(app, &tenant_name, &object_name) {
        return;
    }
    if crate::managed::state::is_ping_shipped_object(&object) {
        app.push_toast(ToastKind::Info, "Ping-shipped objects cannot be deleted");
        return;
    }
    let Some(LoadState::Loaded(doc)) = app.managed.data.get(&tenant_name) else {
        return;
    };
    let draft = DeleteObjectState {
        tenant_name,
        inbound: ops::inbound_relationships(doc, &object_name),
        object_name,
        original_doc: doc.clone(),
        record_count: None,
    };
    app.managed.pending_object_delete = Some(draft.clone());
    app.input_mode = InputMode::Managed(Mode::DeleteObjectConfirm);
    ops::start_object_record_count(app, draft);
}

/// Opens the whole-document create flow when the managed config is loaded.
pub fn start_new_object(app: &mut App) {
    let Some(tenant_name) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    let Some(LoadState::Loaded(doc)) = app.managed.data.get(&tenant_name) else {
        return;
    };
    app.managed.new_object = Some(NewObjectState::new(tenant_name, doc.clone()));
    app.input_mode = InputMode::Managed(Mode::NewObject);
}

pub fn start_add_hook(app: &mut App) {
    let Some((tenant_name, object_name, object)) = selected_object_with_tenant(app) else {
        return;
    };
    if write_in_flight(app, &tenant_name, &object_name) {
        return;
    }
    let draft = AddHookState::new(tenant_name, object_name, object);
    if draft.events.is_empty() {
        app.push_toast(
            ToastKind::Info,
            "All lifecycle hook events are already registered",
        );
        return;
    }
    app.managed.add_hook = Some(draft);
    app.input_mode = InputMode::Managed(Mode::AddHook);
}

pub fn request_delete_field(app: &mut App) {
    let Some((tenant_name, object_name, object, field_key, property, _)) = selected_property(app)
    else {
        app.push_toast(ToastKind::Info, "Selected object has no fields");
        return;
    };
    if write_in_flight(app, &tenant_name, &object_name) {
        return;
    }
    let caps = crate::managed::state::field_capability_for_property(&object, &field_key, &property);
    if !caps.delete {
        app.push_toast(ToastKind::Warning, "Standard fields cannot be deleted");
        return;
    }
    app.managed.pending_delete = Some(DeleteFieldState {
        tenant_name,
        object_name,
        field_key,
        original_object: object,
        is_relationship: crate::managed::state::is_relationship_property(&property),
    });
    app.input_mode = InputMode::Managed(Mode::DeleteFieldConfirm);
}

pub fn relationship_target_matches(app: &App) -> Vec<String> {
    let Some(tenant_name) = app.active_tenant().map(|tenant| tenant.name.as_str()) else {
        return Vec::new();
    };
    let Some(LoadState::Loaded(doc)) = app.managed.data.get(tenant_name) else {
        return Vec::new();
    };
    let Ok(summaries) = crate::managed::api::summarize(doc) else {
        return Vec::new();
    };
    let query = app
        .managed
        .relationship_form
        .as_ref()
        .map(|draft| draft.target_query.value().trim().to_lowercase())
        .unwrap_or_default();
    let mut names: Vec<String> = summaries.into_iter().map(|summary| summary.name).collect();
    if !query.is_empty() {
        names.retain(|name| name.to_lowercase().contains(&query));
    }
    names.sort();
    names
}

fn selected_object(app: &App) -> Option<(String, &Value)> {
    let tenant_name = app.active_tenant().map(|tenant| tenant.name.as_str())?;
    let matches = app.managed.matches(Some(tenant_name));
    let selected = app.managed.selected.min(matches.len().saturating_sub(1));
    let item = matches.get(selected)?;
    let LoadState::Loaded(doc) = app.managed.data.get(tenant_name)? else {
        return None;
    };
    let object = crate::managed::api::object_named(doc, &item.name).ok()?;
    Some((item.name.clone(), object))
}

fn selected_object_with_tenant(app: &App) -> Option<(String, String, Value)> {
    let tenant_name = app.active_tenant().map(|tenant| tenant.name.clone())?;
    let (object_name, object) = selected_object(app)?;
    Some((tenant_name, object_name, object.clone()))
}

fn selected_property(app: &App) -> Option<(String, String, Value, String, Value, bool)> {
    let (tenant_name, object_name, object) = selected_object_with_tenant(app)?;
    let property_names = crate::managed::state::property_names(&object);
    let property_idx = app
        .managed
        .property_selected
        .min(property_names.len().saturating_sub(1));
    let field_key = property_names.get(property_idx)?.clone();
    let property = crate::managed::state::properties(&object)
        .and_then(|properties| properties.get(&field_key))
        .cloned()?;
    let required = crate::managed::state::required_fields(&object).contains(&field_key);
    Some((
        tenant_name,
        object_name,
        object,
        field_key,
        property,
        required,
    ))
}

fn clamp_selected_property(app: &mut App) {
    let n = selected_object(app)
        .map(|(_, object)| crate::managed::state::property_names(object).len())
        .unwrap_or(0);
    app.managed.clamp_property_selection(n);
}

fn write_in_flight(app: &mut App, tenant_name: &str, object_name: &str) -> bool {
    if app
        .managed
        .in_flight_writes
        .contains(&(tenant_name.to_string(), object_name.to_string()))
    {
        app.push_toast(
            ToastKind::Info,
            format!("Write already in progress: {object_name}"),
        );
        return true;
    }
    false
}
