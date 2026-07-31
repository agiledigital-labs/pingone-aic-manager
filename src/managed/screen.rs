//! Managed-objects tab interaction: lazy loading, fuzzy search, selection, and
//! in-TUI schema edits routed through the managed object-replace engine.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;

use crate::app::event::AppEvent;
use crate::app::event::ToastKind;
use crate::app::{App, InputMode};
use crate::managed::ops;
use crate::managed::state::{
    AddChooseState, AddFieldFocus, AddFieldState, AddHookState, AddKind, DeleteFieldState,
    DeleteObjectState, EditFieldFocus, FieldEditState, LoadState, NewObjectFocus, NewObjectState,
    RefPropDraft, RefPropFocus, RelationshipFocus, RelationshipFormState, RenameFieldState,
    RenameObjectConfirmState, RenameObjectState,
};

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

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) {
    match mode {
        Mode::Search => handle_search_key(app, key),
        Mode::EditField => handle_edit_key(app, key),
        Mode::AddChooseKind => handle_add_choose_key(app, key),
        Mode::AddField => handle_add_field_key(app, key),
        Mode::Relationship => handle_relationship_key(app, key),
        Mode::RelationshipTarget => handle_relationship_target_key(app, key),
        Mode::RefProp => handle_ref_prop_key(app, key),
        Mode::AddHook => handle_add_hook_key(app, key),
        Mode::DeleteFieldConfirm => handle_delete_confirm_key(app, key),
        Mode::DeleteObjectConfirm => handle_delete_object_confirm_key(app, key),
        Mode::RenameField => handle_rename_field_key(app, key),
        Mode::RenameObject => handle_rename_object_key(app, key),
        Mode::RenameObjectConfirm => handle_rename_object_confirm_key(app, key),
        Mode::NewObject => handle_new_object_key(app, key),
    }
}

pub fn footer_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let InputMode::Managed(mode) = app.input_mode else {
        return Vec::new();
    };
    match mode {
        Mode::Search => vec![("Enter", "keep filter"), ("Esc", "clear + exit")],
        Mode::EditField => {
            let mut out = vec![("Tab", "navigate")];
            match app.managed.editing.as_ref().map(|edit| edit.focused) {
                Some(EditFieldFocus::Save) => out.push(("Enter", "save")),
                Some(focus) if focus.is_bool() => {
                    out.push(("Space", "toggle"));
                    out.push(("Enter", "toggle"));
                }
                Some(_) => out.push(("Enter", "next")),
                None => {}
            }
            out.push(("Esc", "cancel"));
            out
        }
        Mode::AddChooseKind => vec![
            ("←/→ or Tab", "choose kind"),
            ("Enter", "continue"),
            ("Esc", "cancel"),
        ],
        Mode::AddField => {
            let mut out = vec![("Tab", "navigate")];
            match app.managed.add_field.as_ref().map(|draft| draft.focused) {
                Some(AddFieldFocus::Save) => out.push(("Enter", "add")),
                Some(AddFieldFocus::Type) => out.push(("←/→", "change type")),
                Some(focus) if focus.is_bool() => out.push(("Space", "toggle")),
                Some(_) => out.push(("Enter", "next")),
                None => {}
            }
            out.push(("Esc", "cancel"));
            out
        }
        Mode::Relationship => {
            let mut out = vec![("Tab", "navigate")];
            match app
                .managed
                .relationship_form
                .as_ref()
                .map(|draft| draft.focused)
            {
                Some(RelationshipFocus::Save) => out.push(("Enter", "save")),
                Some(RelationshipFocus::Target) => out.push(("Enter", "pick target")),
                Some(RelationshipFocus::Forward | RelationshipFocus::Reverse) => {
                    out.push(("←/→", "change"))
                }
                Some(RelationshipFocus::RefProperties) => {
                    out.extend([("Ctrl-A", "add"), ("Enter", "edit"), ("d", "delete")]);
                }
                Some(focus) if focus.is_bool() => out.push(("Space", "toggle")),
                Some(_) => out.push(("Enter", "next")),
                None => {}
            }
            out.push(("Esc", "cancel"));
            out
        }
        Mode::RelationshipTarget => vec![
            ("Enter", "choose target"),
            ("↑/↓", "navigate"),
            ("Esc", "back"),
        ],
        Mode::RefProp => vec![
            ("Tab", "navigate"),
            ("Enter", "save/next"),
            ("←/→", "type"),
            ("Esc", "cancel"),
        ],
        Mode::AddHook => vec![
            ("Enter", "register hook"),
            ("↑/↓", "navigate"),
            ("Esc", "cancel"),
        ],
        Mode::DeleteFieldConfirm => vec![("y", "delete"), ("n/Esc", "cancel")],
        Mode::DeleteObjectConfirm => vec![("y", "delete"), ("n/Esc", "cancel")],
        Mode::RenameField => vec![("Enter", "rename"), ("Esc", "cancel")],
        Mode::RenameObject => vec![("Enter", "continue"), ("Esc", "cancel")],
        Mode::RenameObjectConfirm => vec![("y", "rename"), ("n/Esc", "cancel")],
        Mode::NewObject => vec![
            ("Tab", "navigate"),
            ("Enter", "create/next"),
            ("Esc", "cancel"),
        ],
    }
}

pub fn help_lines(mode: Mode, app: &App) -> Option<Vec<(&'static str, &'static str)>> {
    match mode {
        Mode::Search => Some(vec![
            ("Enter", "keep filter"),
            ("Esc", "clear + exit"),
            ("↑/↓", "move selection"),
            ("PgUp/PgDn", "move by page"),
            ("F1", "show keybinds"),
        ]),
        Mode::EditField => {
            let mut out = vec![("Tab", "navigate")];
            match app.managed.editing.as_ref().map(|edit| edit.focused) {
                Some(EditFieldFocus::Save) => out.push(("Enter", "save")),
                Some(focus) if focus.is_bool() => {
                    out.push(("Space", "toggle"));
                    out.push(("Enter", "toggle"));
                }
                Some(_) => out.push(("Enter", "next")),
                None => {}
            }
            out.push(("Esc", "cancel"));
            Some(out)
        }
        Mode::AddChooseKind => Some(vec![
            ("←/→ or Tab", "choose kind"),
            ("Enter", "continue"),
            ("Esc", "cancel"),
        ]),
        Mode::AddField => {
            let mut out = vec![("Tab", "navigate")];
            match app.managed.add_field.as_ref().map(|draft| draft.focused) {
                Some(AddFieldFocus::Save) => out.push(("Enter", "add")),
                Some(AddFieldFocus::Type) => out.push(("←/→", "change type")),
                Some(focus) if focus.is_bool() => out.push(("Space", "toggle")),
                Some(_) => out.push(("Enter", "next")),
                None => {}
            }
            out.push(("Esc", "cancel"));
            Some(out)
        }
        Mode::Relationship => {
            let mut out = vec![("Tab", "navigate")];
            match app
                .managed
                .relationship_form
                .as_ref()
                .map(|draft| draft.focused)
            {
                Some(RelationshipFocus::Save) => out.push(("Enter", "save")),
                Some(RelationshipFocus::Target) => out.push(("Enter", "pick target")),
                Some(RelationshipFocus::Forward | RelationshipFocus::Reverse) => {
                    out.push(("←/→", "change"))
                }
                Some(RelationshipFocus::RefProperties) => {
                    out.extend([("Ctrl-A", "add"), ("Enter", "edit"), ("d", "delete")]);
                }
                Some(focus) if focus.is_bool() => out.push(("Space", "toggle")),
                Some(_) => out.push(("Enter", "next")),
                None => {}
            }
            out.push(("Esc", "cancel"));
            Some(out)
        }
        Mode::RelationshipTarget => Some(vec![
            ("Enter", "choose target"),
            ("↑/↓", "navigate"),
            ("Esc", "back"),
        ]),
        Mode::RefProp => Some(vec![
            ("Tab", "navigate"),
            ("Enter", "save/next"),
            ("←/→", "type"),
            ("Esc", "cancel"),
        ]),
        Mode::AddHook => Some(vec![
            ("Enter", "register hook"),
            ("↑/↓", "navigate"),
            ("Esc", "cancel"),
        ]),
        Mode::DeleteFieldConfirm => Some(vec![("y", "delete"), ("n/Esc", "cancel")]),
        Mode::DeleteObjectConfirm => Some(vec![("y", "delete"), ("n/Esc", "cancel")]),
        Mode::RenameField => Some(vec![("Enter", "rename"), ("Esc", "cancel")]),
        Mode::RenameObject => Some(vec![("Enter", "continue"), ("Esc", "cancel")]),
        Mode::RenameObjectConfirm => Some(vec![("y", "rename"), ("n/Esc", "cancel")]),
        Mode::NewObject => Some(vec![
            ("Tab", "navigate"),
            ("Enter", "create/next"),
            ("Esc", "cancel"),
        ]),
    }
}

pub fn editing_field_active(app: &App) -> bool {
    app.managed.editing.is_some()
}

pub fn add_field_active(app: &App) -> bool {
    app.managed.add_field.is_some()
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

fn handle_search_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.managed.reset_view();
            app.input_mode = InputMode::Normal;
            return;
        }
        KeyCode::Enter => {
            app.input_mode = InputMode::Normal;
            return;
        }
        KeyCode::Up => return crate::app::keymap::move_selection(app, -1),
        KeyCode::Down => return crate::app::keymap::move_selection(app, 1),
        KeyCode::PageUp => return crate::app::keymap::move_selection(app, -10),
        KeyCode::PageDown => return crate::app::keymap::move_selection(app, 10),
        _ => {}
    }

    let before = app.managed.query.value().to_string();
    if app.managed.query.handle_key(&key) && app.managed.query.value() != before {
        app.managed.selected = 0;
        app.managed.scroll = 0;
        app.managed.property_selected = 0;
    }
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

fn handle_add_choose_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => ops::cancel_active_draft(app),
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
            if let Some(draft) = app.managed.add_choose.as_mut() {
                draft.kind.toggle();
            }
        }
        KeyCode::Enter => {
            let kind = app.managed.add_choose.take().map(|draft| draft.kind);
            match kind {
                Some(AddKind::Field) => start_add_field(app),
                Some(AddKind::Relationship) => start_relationship_create(app),
                None => app.input_mode = InputMode::Normal,
            }
        }
        _ => {}
    }
}

fn handle_rename_field_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => ops::cancel_active_draft(app),
        KeyCode::Tab | KeyCode::BackTab => {
            if let Some(rename) = app.managed.renaming.as_mut() {
                ops::advance_rename_field_focus(rename, key.code == KeyCode::Tab);
            }
        }
        KeyCode::Enter => ops::commit_rename_field(app),
        _ => {
            if let Some(rename) = app.managed.renaming.as_mut() {
                rename.error = None;
                rename.key.handle_key(&key);
            }
        }
    }
}

fn handle_rename_object_key(app: &mut App, key: KeyEvent) {
    if key.code == KeyCode::Esc {
        ops::cancel_active_draft(app);
        return;
    }
    if key.code == KeyCode::Enter {
        let Some(mut draft) = app.managed.renaming_object.take() else {
            return;
        };
        let existing = crate::managed::api::objects(&draft.original_doc)
            .map(|objects| {
                objects
                    .iter()
                    .filter_map(|object| {
                        object
                            .get("name")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    })
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        if let Err(error) = crate::managed::state::validate_object_name(
            &draft.key.value,
            &existing,
            &draft.old_name,
        ) {
            draft.error = Some(error);
            app.managed.renaming_object = Some(draft);
            return;
        }
        ops::start_record_count(app, draft);
        return;
    }
    if let Some(draft) = app.managed.renaming_object.as_mut() {
        draft.error = None;
        draft.key.handle_key(&key);
    }
}

fn handle_rename_object_confirm_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') => ops::cancel_active_draft(app),
        KeyCode::Char('y') => {
            if app.active_tenant().is_some_and(|tenant| {
                tenant.theme == crate::config::tenant::TenantTheme::Production
            }) {
                let Some(confirm) = app.managed.rename_object_confirm.as_ref() else {
                    return;
                };
                let request = ops::RenameObjectRequest {
                    tenant_name: confirm.draft.tenant_name.clone(),
                    old_name: confirm.draft.old_name.clone(),
                    new_name: confirm.draft.key.value.clone(),
                    previous_doc: confirm.draft.original_doc.clone(),
                };
                app.prod_confirm.pending =
                    Some(crate::app::prod_confirm::PendingProdAction::Managed(
                        ops::ProdAction::RenameObject(Box::new(request)),
                    ));
                app.input_mode = InputMode::ProdConfirm;
            } else {
                let Some(confirm) = app.managed.rename_object_confirm.take() else {
                    return;
                };
                let request = ops::RenameObjectRequest {
                    tenant_name: confirm.draft.tenant_name,
                    old_name: confirm.draft.old_name,
                    new_name: confirm.draft.key.value,
                    previous_doc: confirm.draft.original_doc,
                };
                ops::execute_rename_object(app, request, false);
            }
        }
        _ => {}
    }
}

fn handle_new_object_key(app: &mut App, key: KeyEvent) {
    let Some(focused) = app.managed.new_object.as_ref().map(|draft| draft.focused) else {
        app.input_mode = InputMode::Normal;
        return;
    };
    match key.code {
        KeyCode::Esc => {
            ops::cancel_active_draft(app);
            return;
        }
        KeyCode::Tab => {
            if let Some(draft) = app.managed.new_object.as_mut() {
                draft.focused = draft.focused.next();
            }
            return;
        }
        KeyCode::BackTab => {
            if let Some(draft) = app.managed.new_object.as_mut() {
                draft.focused = draft.focused.prev();
            }
            return;
        }
        KeyCode::Enter if focused == NewObjectFocus::Save => {
            let Some(mut draft) = app.managed.new_object.take() else {
                return;
            };
            let existing = crate::managed::api::objects(&draft.original_doc)
                .map(|objects| {
                    objects
                        .iter()
                        .filter_map(|object| {
                            object
                                .get("name")
                                .and_then(Value::as_str)
                                .map(ToString::to_string)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if let Err(error) =
                crate::managed::state::validate_object_name(&draft.name.value, &existing, "")
            {
                draft.error = Some(error);
                app.managed.new_object = Some(draft);
                return;
            }
            let request = ops::CreateObjectRequest {
                tenant_name: draft.tenant_name.clone(),
                name: draft.name.value.clone(),
                title: draft.title.value.clone(),
                description: draft.description.value.clone(),
                previous_doc: draft.original_doc.clone(),
            };
            if app.active_tenant().is_some_and(|tenant| {
                tenant.theme == crate::config::tenant::TenantTheme::Production
            }) {
                app.managed.new_object = Some(draft);
                app.prod_confirm.pending =
                    Some(crate::app::prod_confirm::PendingProdAction::Managed(
                        ops::ProdAction::CreateObject(Box::new(request)),
                    ));
                app.input_mode = InputMode::ProdConfirm;
            } else {
                ops::execute_create_object(app, request, false);
            }
            return;
        }
        KeyCode::Enter => {
            if let Some(draft) = app.managed.new_object.as_mut() {
                draft.focused = draft.focused.next();
            }
            return;
        }
        _ => {}
    }
    let Some(draft) = app.managed.new_object.as_mut() else {
        return;
    };
    draft.error = None;
    match focused {
        NewObjectFocus::Name => {
            draft.name.handle_key(&key);
        }
        NewObjectFocus::Title => {
            draft.title.handle_key(&key);
        }
        NewObjectFocus::Description => {
            draft.description.handle_key(&key);
        }
        NewObjectFocus::Save => {}
    }
}

fn handle_edit_key(app: &mut App, key: KeyEvent) {
    let Some(focused) = app.managed.editing.as_ref().map(|edit| edit.focused) else {
        app.input_mode = InputMode::Normal;
        return;
    };

    match key.code {
        KeyCode::Esc => {
            ops::cancel_active_draft(app);
            return;
        }
        KeyCode::Tab => {
            if let Some(edit) = app.managed.editing.as_mut() {
                ops::advance_focus(edit, true);
            }
            return;
        }
        KeyCode::BackTab => {
            if let Some(edit) = app.managed.editing.as_mut() {
                ops::advance_focus(edit, false);
            }
            return;
        }
        KeyCode::Enter if focused == EditFieldFocus::Save => {
            ops::commit_edit(app);
            return;
        }
        KeyCode::Enter if focused.is_bool() => {
            if let Some(edit) = app.managed.editing.as_mut() {
                edit.toggle_focused_bool();
                edit.error = None;
            }
            return;
        }
        KeyCode::Enter => {
            if let Some(edit) = app.managed.editing.as_mut() {
                ops::advance_focus(edit, true);
            }
            return;
        }
        KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if focused.is_bool() => {
            if let Some(edit) = app.managed.editing.as_mut() {
                edit.toggle_focused_bool();
                edit.error = None;
            }
            return;
        }
        _ => {}
    }

    let Some(edit) = app.managed.editing.as_mut() else {
        return;
    };
    edit.error = None;
    match focused {
        EditFieldFocus::Key if edit.caps.rename_key => {
            edit.key.handle_key(&key);
        }
        EditFieldFocus::Title => {
            edit.title.handle_key(&key);
        }
        EditFieldFocus::Description => {
            edit.description.handle_key(&key);
        }
        _ => {}
    }
}

fn handle_add_field_key(app: &mut App, key: KeyEvent) {
    let Some(focused) = app.managed.add_field.as_ref().map(|draft| draft.focused) else {
        app.input_mode = InputMode::Normal;
        return;
    };
    match key.code {
        KeyCode::Esc => {
            ops::cancel_active_draft(app);
            return;
        }
        KeyCode::Tab => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                ops::advance_add_field_focus(draft, true);
            }
            return;
        }
        KeyCode::BackTab => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                ops::advance_add_field_focus(draft, false);
            }
            return;
        }
        KeyCode::Enter if focused == AddFieldFocus::Save => {
            ops::commit_add_field(app);
            return;
        }
        KeyCode::Enter if focused == AddFieldFocus::Type => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                draft.field_type = draft.field_type.next();
                draft.error = None;
            }
            return;
        }
        KeyCode::Left if focused == AddFieldFocus::Type => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                draft.field_type = draft.field_type.prev();
                draft.error = None;
            }
            return;
        }
        KeyCode::Right if focused == AddFieldFocus::Type => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                draft.field_type = draft.field_type.next();
                draft.error = None;
            }
            return;
        }
        KeyCode::Enter if focused.is_bool() => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                draft.toggle_focused_bool();
                draft.error = None;
            }
            return;
        }
        KeyCode::Char(' ') if focused == AddFieldFocus::Type => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                draft.field_type = draft.field_type.next();
                draft.error = None;
            }
            return;
        }
        KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if focused.is_bool() => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                draft.toggle_focused_bool();
                draft.error = None;
            }
            return;
        }
        KeyCode::Enter => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                ops::advance_add_field_focus(draft, true);
            }
            return;
        }
        _ => {}
    }

    let Some(draft) = app.managed.add_field.as_mut() else {
        return;
    };
    draft.error = None;
    match focused {
        AddFieldFocus::Key => {
            draft.key.handle_key(&key);
        }
        AddFieldFocus::Title => {
            draft.title.handle_key(&key);
        }
        AddFieldFocus::Description => {
            draft.description.handle_key(&key);
        }
        _ => {}
    }
}

fn handle_relationship_key(app: &mut App, key: KeyEvent) {
    let Some(focused) = app
        .managed
        .relationship_form
        .as_ref()
        .map(|draft| draft.focused)
    else {
        app.input_mode = InputMode::Normal;
        return;
    };
    if key.code == KeyCode::Char('a') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.managed.ref_prop_draft = Some(RefPropDraft::new_add());
        app.input_mode = InputMode::Managed(Mode::RefProp);
        return;
    }
    if focused == RelationshipFocus::RefProperties {
        match key.code {
            KeyCode::Esc => ops::cancel_active_draft(app),
            KeyCode::Tab => {
                if let Some(draft) = app.managed.relationship_form.as_mut() {
                    draft.focused = draft.focused.next(draft.reverse);
                }
            }
            KeyCode::BackTab => {
                if let Some(draft) = app.managed.relationship_form.as_mut() {
                    draft.focused = draft.focused.prev(draft.reverse);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(draft) = app.managed.relationship_form.as_mut() {
                    draft.ref_selected = draft.ref_selected.saturating_sub(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(draft) = app.managed.relationship_form.as_mut() {
                    draft.ref_selected =
                        (draft.ref_selected + 1).min(draft.ref_properties.len().saturating_sub(1));
                }
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(draft) = app.managed.relationship_form.as_mut() {
                    draft.remove_selected_ref_property();
                    draft.error = None;
                }
            }
            KeyCode::Enter => {
                let ref_prop_draft = app.managed.relationship_form.as_ref().and_then(|draft| {
                    draft
                        .ref_properties
                        .get(draft.ref_selected)
                        .map(|property| RefPropDraft::edit(draft.ref_selected, property))
                });
                if let Some(ref_prop_draft) = ref_prop_draft {
                    app.managed.ref_prop_draft = Some(ref_prop_draft);
                    app.input_mode = InputMode::Managed(Mode::RefProp);
                } else if let Some(draft) = app.managed.relationship_form.as_mut() {
                    draft.focused = draft.focused.next(draft.reverse);
                }
            }
            _ => {}
        }
        return;
    }
    match key.code {
        KeyCode::Esc => {
            ops::cancel_active_draft(app);
            return;
        }
        KeyCode::Tab => {
            if let Some(draft) = app.managed.relationship_form.as_mut() {
                draft.focused = draft.focused.next(draft.reverse);
            }
            return;
        }
        KeyCode::BackTab => {
            if let Some(draft) = app.managed.relationship_form.as_mut() {
                draft.focused = draft.focused.prev(draft.reverse);
            }
            return;
        }
        KeyCode::Enter if focused == RelationshipFocus::Save => {
            ops::commit_relationship(app);
            return;
        }
        KeyCode::Enter if focused == RelationshipFocus::Target => {
            app.input_mode = InputMode::Managed(Mode::RelationshipTarget);
            return;
        }
        KeyCode::Enter if focused.is_bool() => {
            if let Some(draft) = app.managed.relationship_form.as_mut() {
                toggle_relationship_bool(draft);
                draft.error = None;
            }
            return;
        }
        KeyCode::Char(' ') if focused.is_bool() => {
            if let Some(draft) = app.managed.relationship_form.as_mut() {
                toggle_relationship_bool(draft);
                draft.error = None;
            }
            return;
        }
        KeyCode::Left | KeyCode::Right
            if matches!(
                focused,
                RelationshipFocus::Forward | RelationshipFocus::Reverse
            ) =>
        {
            if let Some(draft) = app.managed.relationship_form.as_mut() {
                if focused == RelationshipFocus::Forward {
                    draft.forward = if key.code == KeyCode::Left {
                        draft.forward.prev()
                    } else {
                        draft.forward.next()
                    };
                } else {
                    draft.reverse = if key.code == KeyCode::Left {
                        draft.reverse.prev()
                    } else {
                        draft.reverse.next()
                    };
                    if draft.reverse == crate::managed::state::ReverseCardinality::None
                        && draft.focused == RelationshipFocus::ReverseKey
                    {
                        draft.focused = RelationshipFocus::Searchable;
                    }
                }
                draft.error = None;
            }
            return;
        }
        KeyCode::Char(' ')
            if matches!(
                focused,
                RelationshipFocus::Forward | RelationshipFocus::Reverse
            ) =>
        {
            if let Some(draft) = app.managed.relationship_form.as_mut() {
                if focused == RelationshipFocus::Forward {
                    draft.forward = draft.forward.next();
                } else {
                    draft.reverse = draft.reverse.next();
                }
                draft.error = None;
            }
            return;
        }
        KeyCode::Enter => {
            if let Some(draft) = app.managed.relationship_form.as_mut() {
                draft.focused = draft.focused.next(draft.reverse);
            }
            return;
        }
        _ => {}
    }

    let Some(draft) = app.managed.relationship_form.as_mut() else {
        return;
    };
    draft.error = None;
    match focused {
        RelationshipFocus::Key => {
            draft.key.handle_key(&key);
        }
        RelationshipFocus::Title => {
            draft.title.handle_key(&key);
        }
        RelationshipFocus::Description => {
            draft.description.handle_key(&key);
        }
        RelationshipFocus::ReverseKey => {
            draft.reverse_key.handle_key(&key);
        }
        _ => {}
    }
}

fn handle_ref_prop_key(app: &mut App, key: KeyEvent) {
    let Some(focused) = app
        .managed
        .ref_prop_draft
        .as_ref()
        .map(|draft| draft.focused)
    else {
        app.input_mode = InputMode::Managed(Mode::Relationship);
        return;
    };
    match key.code {
        KeyCode::Esc => {
            app.managed.ref_prop_draft = None;
            app.input_mode = InputMode::Managed(Mode::Relationship);
            return;
        }
        KeyCode::Tab => {
            if let Some(draft) = app.managed.ref_prop_draft.as_mut() {
                draft.focused = draft.focused.next();
            }
            return;
        }
        KeyCode::BackTab => {
            if let Some(draft) = app.managed.ref_prop_draft.as_mut() {
                draft.focused = draft.focused.prev();
            }
            return;
        }
        KeyCode::Left if focused == RefPropFocus::Type => {
            if let Some(draft) = app.managed.ref_prop_draft.as_mut() {
                draft.kind = draft.kind.prev();
                draft.error = None;
            }
            return;
        }
        KeyCode::Right | KeyCode::Char(' ') if focused == RefPropFocus::Type => {
            if let Some(draft) = app.managed.ref_prop_draft.as_mut() {
                draft.kind = draft.kind.next();
                draft.error = None;
            }
            return;
        }
        KeyCode::Enter if focused == RefPropFocus::Save => {
            let Some(mut draft) = app.managed.ref_prop_draft.take() else {
                return;
            };
            let result = app
                .managed
                .relationship_form
                .as_mut()
                .ok_or_else(|| "Relationship form is no longer active".to_string())
                .and_then(|form| ops::commit_ref_prop(form, &draft));
            match result {
                Ok(()) => app.input_mode = InputMode::Managed(Mode::Relationship),
                Err(error) => {
                    draft.error = Some(error);
                    app.managed.ref_prop_draft = Some(draft);
                }
            }
            return;
        }
        KeyCode::Enter => {
            if let Some(draft) = app.managed.ref_prop_draft.as_mut() {
                draft.focused = draft.focused.next();
            }
            return;
        }
        _ => {}
    }

    let Some(draft) = app.managed.ref_prop_draft.as_mut() else {
        return;
    };
    draft.error = None;
    match focused {
        RefPropFocus::Name => {
            draft.name.handle_key(&key);
        }
        RefPropFocus::Label => {
            draft.label.handle_key(&key);
        }
        RefPropFocus::Type | RefPropFocus::Save => {}
    }
}

fn toggle_relationship_bool(draft: &mut RelationshipFormState) {
    match draft.focused {
        RelationshipFocus::Searchable => draft.searchable = !draft.searchable,
        RelationshipFocus::Viewable => draft.viewable = !draft.viewable,
        RelationshipFocus::UserEditable => draft.user_editable = !draft.user_editable,
        RelationshipFocus::Required => draft.required = !draft.required,
        RelationshipFocus::Validate => draft.validate = !draft.validate,
        _ => {}
    }
}

fn handle_relationship_target_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Managed(Mode::Relationship);
            return;
        }
        KeyCode::Enter => {
            let matches = relationship_target_matches(app);
            let selected = app
                .managed
                .relationship_form
                .as_ref()
                .map(|draft| draft.target_selected.min(matches.len().saturating_sub(1)))
                .unwrap_or(0);
            if let Some(name) = matches.get(selected).cloned() {
                if let Some(draft) = app.managed.relationship_form.as_mut() {
                    draft.target_name = Some(name);
                    draft.focused = RelationshipFocus::Target;
                    draft.error = None;
                }
                app.input_mode = InputMode::Managed(Mode::Relationship);
            }
            return;
        }
        KeyCode::Up => {
            move_relationship_target(app, -1);
            return;
        }
        KeyCode::Down => {
            move_relationship_target(app, 1);
            return;
        }
        KeyCode::PageUp => {
            move_relationship_target(app, -10);
            return;
        }
        KeyCode::PageDown => {
            move_relationship_target(app, 10);
            return;
        }
        _ => {}
    }

    let before = app
        .managed
        .relationship_form
        .as_ref()
        .map(|draft| draft.target_query.value().to_string())
        .unwrap_or_default();
    if let Some(draft) = app.managed.relationship_form.as_mut() {
        if draft.target_query.handle_key(&key) && draft.target_query.value() != before {
            draft.target_selected = 0;
        }
    }
}

fn move_relationship_target(app: &mut App, delta: isize) {
    let n = relationship_target_matches(app).len();
    if n == 0 {
        return;
    }
    if let Some(draft) = app.managed.relationship_form.as_mut() {
        let cur = draft.target_selected.min(n - 1) as isize;
        draft.target_selected = (cur + delta).clamp(0, n as isize - 1) as usize;
    }
}

fn handle_add_hook_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            ops::cancel_active_draft(app);
        }
        KeyCode::Enter => {
            ops::commit_add_hook(app);
        }
        KeyCode::Up | KeyCode::Char('k') => move_hook(app, -1),
        KeyCode::Down | KeyCode::Char('j') => move_hook(app, 1),
        KeyCode::PageUp => move_hook(app, -10),
        KeyCode::PageDown => move_hook(app, 10),
        _ => {}
    }
}

fn move_hook(app: &mut App, delta: isize) {
    let Some(draft) = app.managed.add_hook.as_mut() else {
        return;
    };
    if draft.events.is_empty() {
        return;
    }
    let cur = draft.selected.min(draft.events.len() - 1) as isize;
    draft.selected = (cur + delta).clamp(0, draft.events.len() as isize - 1) as usize;
}

fn handle_delete_confirm_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => ops::commit_delete_field(app),
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.managed.pending_delete = None;
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
}

fn handle_delete_object_confirm_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let Some(draft) = app.managed.pending_object_delete.as_ref() else {
                return;
            };
            let request = ops::DeleteObjectRequest {
                tenant_name: draft.tenant_name.clone(),
                object_name: draft.object_name.clone(),
                previous_doc: draft.original_doc.clone(),
            };
            if app.active_tenant().is_some_and(|tenant| {
                tenant.theme == crate::config::tenant::TenantTheme::Production
            }) {
                app.prod_confirm.pending =
                    Some(crate::app::prod_confirm::PendingProdAction::Managed(
                        ops::ProdAction::DeleteObject(Box::new(request)),
                    ));
                app.input_mode = InputMode::ProdConfirm;
            } else {
                ops::execute_delete_object(app, request, false);
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => ops::cancel_active_draft(app),
        _ => {}
    }
}
