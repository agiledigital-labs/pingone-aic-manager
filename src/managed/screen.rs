//! Managed-objects tab interaction: lazy loading, fuzzy search, selection, and
//! in-TUI schema edits routed through the managed object-replace engine.

use crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;

use crate::app::event::AppEvent;
use crate::app::event::ToastKind;
use crate::app::{App, InputMode};
use crate::managed::ops;
use crate::managed::state::{
    AddChooseState, AddFieldFocus, AddFieldState, AddHookState, AddKind, AddRelationshipFocus,
    AddRelationshipState, DeleteFieldState, EditFieldFocus, FieldEditState, LoadState,
    RenameFieldState,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Search,
    EditField,
    AddChooseKind,
    AddField,
    AddRelationship,
    PickRelationshipTarget,
    AddHook,
    DeleteFieldConfirm,
    RenameField,
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
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) {
    match mode {
        Mode::Search => handle_search_key(app, key),
        Mode::EditField => handle_edit_key(app, key),
        Mode::AddChooseKind => handle_add_choose_key(app, key),
        Mode::AddField => handle_add_field_key(app, key),
        Mode::AddRelationship => handle_add_relationship_key(app, key),
        Mode::PickRelationshipTarget => handle_relationship_target_key(app, key),
        Mode::AddHook => handle_add_hook_key(app, key),
        Mode::DeleteFieldConfirm => handle_delete_confirm_key(app, key),
        Mode::RenameField => handle_rename_field_key(app, key),
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
        Mode::AddRelationship => {
            let mut out = vec![("Tab", "navigate")];
            match app
                .managed
                .add_relationship
                .as_ref()
                .map(|draft| draft.focused)
            {
                Some(AddRelationshipFocus::Save) => out.push(("Enter", "add")),
                Some(AddRelationshipFocus::Target) => out.push(("Enter", "pick target")),
                Some(focus) if focus.is_bool() => out.push(("Space", "toggle")),
                Some(_) => out.push(("Enter", "next")),
                None => {}
            }
            out.push(("Esc", "cancel"));
            out
        }
        Mode::PickRelationshipTarget => vec![
            ("Enter", "choose target"),
            ("↑/↓", "navigate"),
            ("Esc", "back"),
        ],
        Mode::AddHook => vec![
            ("Enter", "register hook"),
            ("↑/↓", "navigate"),
            ("Esc", "cancel"),
        ],
        Mode::DeleteFieldConfirm => vec![("y", "delete"), ("n/Esc", "cancel")],
        Mode::RenameField => vec![("Enter", "rename"), ("Esc", "cancel")],
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
        Mode::AddRelationship => {
            let mut out = vec![("Tab", "navigate")];
            match app
                .managed
                .add_relationship
                .as_ref()
                .map(|draft| draft.focused)
            {
                Some(AddRelationshipFocus::Save) => out.push(("Enter", "add")),
                Some(AddRelationshipFocus::Target) => out.push(("Enter", "pick target")),
                Some(focus) if focus.is_bool() => out.push(("Space", "toggle")),
                Some(_) => out.push(("Enter", "next")),
                None => {}
            }
            out.push(("Esc", "cancel"));
            Some(out)
        }
        Mode::PickRelationshipTarget => Some(vec![
            ("Enter", "choose target"),
            ("↑/↓", "navigate"),
            ("Esc", "back"),
        ]),
        Mode::AddHook => Some(vec![
            ("Enter", "register hook"),
            ("↑/↓", "navigate"),
            ("Esc", "cancel"),
        ]),
        Mode::DeleteFieldConfirm => Some(vec![("y", "delete"), ("n/Esc", "cancel")]),
        Mode::RenameField => Some(vec![("Enter", "rename"), ("Esc", "cancel")]),
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
    } else if app.managed.add_relationship.is_some() {
        Some(Mode::AddRelationship)
    } else if app.managed.add_hook.is_some() {
        Some(Mode::AddHook)
    } else if app.managed.pending_delete.is_some() {
        Some(Mode::DeleteFieldConfirm)
    } else if app.managed.renaming.is_some() {
        Some(Mode::RenameField)
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
    if crate::managed::state::is_relationship_property(&property)
        && crate::managed::state::field_capability(&object, &field_key).rename_key
    {
        app.push_toast(
            ToastKind::Info,
            "Relationship keys cannot be renamed; delete and recreate the relationship",
        );
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

pub fn start_add_relationship(app: &mut App) {
    let Some((tenant_name, object_name, object)) = selected_object_with_tenant(app) else {
        return;
    };
    if write_in_flight(app, &tenant_name, &object_name) {
        return;
    }
    app.managed.add_relationship =
        Some(AddRelationshipState::new(tenant_name, object_name, object));
    app.input_mode = InputMode::Managed(Mode::AddRelationship);
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
        .add_relationship
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
                Some(AddKind::Relationship) => start_add_relationship(app),
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

fn handle_add_relationship_key(app: &mut App, key: KeyEvent) {
    let Some(focused) = app
        .managed
        .add_relationship
        .as_ref()
        .map(|draft| draft.focused)
    else {
        app.input_mode = InputMode::Normal;
        return;
    };
    match key.code {
        KeyCode::Esc => {
            ops::cancel_active_draft(app);
            return;
        }
        KeyCode::Tab => {
            if let Some(draft) = app.managed.add_relationship.as_mut() {
                ops::advance_add_relationship_focus(draft, true);
            }
            return;
        }
        KeyCode::BackTab => {
            if let Some(draft) = app.managed.add_relationship.as_mut() {
                ops::advance_add_relationship_focus(draft, false);
            }
            return;
        }
        KeyCode::Enter if focused == AddRelationshipFocus::Save => {
            ops::commit_add_relationship(app);
            return;
        }
        KeyCode::Enter if focused == AddRelationshipFocus::Target => {
            app.input_mode = InputMode::Managed(Mode::PickRelationshipTarget);
            return;
        }
        KeyCode::Enter if focused.is_bool() => {
            if let Some(draft) = app.managed.add_relationship.as_mut() {
                draft.toggle_focused_bool();
                draft.error = None;
            }
            return;
        }
        KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if focused.is_bool() => {
            if let Some(draft) = app.managed.add_relationship.as_mut() {
                draft.toggle_focused_bool();
                draft.error = None;
            }
            return;
        }
        KeyCode::Enter => {
            if let Some(draft) = app.managed.add_relationship.as_mut() {
                ops::advance_add_relationship_focus(draft, true);
            }
            return;
        }
        _ => {}
    }

    let Some(draft) = app.managed.add_relationship.as_mut() else {
        return;
    };
    draft.error = None;
    match focused {
        AddRelationshipFocus::Key => {
            draft.key.handle_key(&key);
        }
        AddRelationshipFocus::Title => {
            draft.title.handle_key(&key);
        }
        AddRelationshipFocus::Description => {
            draft.description.handle_key(&key);
        }
        AddRelationshipFocus::ReversePropertyName => {
            draft.reverse_property_name.handle_key(&key);
        }
        _ => {}
    }
}

fn handle_relationship_target_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Managed(Mode::AddRelationship);
            return;
        }
        KeyCode::Enter => {
            let matches = relationship_target_matches(app);
            let selected = app
                .managed
                .add_relationship
                .as_ref()
                .map(|draft| draft.target_selected.min(matches.len().saturating_sub(1)))
                .unwrap_or(0);
            if let Some(name) = matches.get(selected).cloned() {
                if let Some(draft) = app.managed.add_relationship.as_mut() {
                    draft.target_name = Some(name);
                    draft.focused = AddRelationshipFocus::Target;
                    draft.error = None;
                }
                app.input_mode = InputMode::Managed(Mode::AddRelationship);
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
        .add_relationship
        .as_ref()
        .map(|draft| draft.target_query.value().to_string())
        .unwrap_or_default();
    if let Some(draft) = app.managed.add_relationship.as_mut() {
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
    if let Some(draft) = app.managed.add_relationship.as_mut() {
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
