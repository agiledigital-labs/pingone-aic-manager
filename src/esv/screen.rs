//! ESV tab input modes, background events, and key handling.
//!
//! The state struct lives on `App` as `app.esv`; handlers remain free
//! functions so global dispatch keeps one arm for the whole feature.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use crossterm::event::{KeyCode, KeyEvent};

use crate::app::event::ToastKind;
use crate::app::prod_confirm::PendingProdAction;
use crate::app::{App, InputMode};
use crate::tui::is_save_chord;
#[derive(Debug)]
pub enum ProdAction {
    Save(crate::esv::state::SavePlan),
    Delete(crate::esv::state::DeletePlan),
    Undo(crate::undo::UndoId),
    Restart { tenant_name: String },
}
pub fn execute_prod_action(app: &mut App, action: ProdAction) {
    match action {
        ProdAction::Save(plan) => crate::esv::ops::execute_save_plan(app, plan, true),
        ProdAction::Delete(plan) => crate::esv::ops::execute_delete_plan(app, plan, true),
        ProdAction::Undo(undo_id) => crate::esv::ops::execute_undo(app, undo_id, true),
        ProdAction::Restart { tenant_name } => {
            crate::esv::ops::trigger_restart_confirmed(app, tenant_name, true)
        }
    }
}

pub fn resume_mode(app: &App, action: &ProdAction) -> InputMode {
    match action {
        ProdAction::Save(_) if app.esv.editing.is_some() => InputMode::Esv(Mode::Edit),
        _ => InputMode::Normal,
    }
}

pub fn describe_prod_action(_action: &ProdAction) -> Option<String> {
    None
}

use crate::config::tenant::TenantTheme;
use crate::esv::ops;
use crate::esv::state::{
    DeleteOutcome, EditField, EditState, EsvView, ExpressionType, LoadState, RefreshOutcome,
    SaveOutcome, UndoFailure, UndoOutcome,
};
use crate::tui::widgets::TextField;
use crate::undo::UndoId;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Search,
    Edit,
    RestartConfirm,
    DeleteConfirm,
}

#[derive(Debug)]
pub enum Event {
    Listed {
        tenant: String,
        outcome: RefreshOutcome,
    },
    SaveResult {
        tenant: String,
        id: String,
        result: std::result::Result<SaveOutcome, String>,
    },
    DeleteResult {
        tenant: String,
        id: String,
        result: std::result::Result<DeleteOutcome, String>,
    },
    UndoResult {
        undo_id: UndoId,
        tenant: String,
        result: std::result::Result<UndoOutcome, UndoFailure>,
    },
    RestartResult {
        tenant: String,
        result: std::result::Result<serde_json::Value, String>,
    },
}

pub fn apply_event(app: &mut App, event: Event) {
    match event {
        Event::Listed { tenant, outcome } => ops::apply_refresh(app, tenant, outcome),
        Event::SaveResult { tenant, id, result } => ops::apply_save_result(app, tenant, id, result),
        Event::DeleteResult { tenant, id, result } => {
            ops::apply_delete_result(app, tenant, id, result)
        }
        Event::UndoResult {
            undo_id,
            tenant,
            result,
        } => ops::apply_undo_result(app, undo_id, tenant, result),
        Event::RestartResult { tenant, result } => ops::apply_restart_result(app, tenant, result),
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) -> crate::Result<()> {
    match mode {
        Mode::Search => {
            handle_search_key(app, key);
            Ok(())
        }
        Mode::Edit => handle_edit_key(app, key),
        Mode::RestartConfirm => handle_restart_confirm_key(app, key),
        Mode::DeleteConfirm => handle_delete_confirm_key(app, key),
    }
}

/// Dispatched from the y/n delete popup. `y` may still route through the
/// shared production confirmation before executing the delete.
pub fn handle_delete_confirm_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let Some(plan) = app.esv.pending_delete.take() else {
                app.input_mode = InputMode::Normal;
                return Ok(());
            };
            let is_prod = app
                .active_tenant()
                .is_some_and(|t| t.theme == TenantTheme::Production);
            if is_prod {
                app.prod_confirm.pending = Some(PendingProdAction::Esv(ProdAction::Delete(plan)));
                app.input_mode = InputMode::ProdConfirm;
            } else {
                ops::execute_delete_plan(app, plan, false);
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.esv.pending_delete = None;
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
    Ok(())
}

/// Dispatched from the y/n popup. `y` triggers the restart, `n`/`Esc`
/// closes the popup.
pub fn handle_restart_confirm_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            ops::trigger_restart(app);
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
    Ok(())
}

/// ESV search keys: chars/backspace/cursor → editor; ↑/↓/PgUp/PgDn keep
/// scrolling the results list while the user is still typing; Esc clears
/// the filter; Enter commits and returns to Normal mode.
pub fn handle_search_key(app: &mut App, key: KeyEvent) {
    // The search box edits whichever half's query is showing.
    if app.esv.view == EsvView::Mappings {
        crate::secretmap::screen::handle_key(app, key, crate::secretmap::screen::Mode::Search);
        return;
    }
    if app.esv.view == EsvView::Secrets {
        match key.code {
            KeyCode::Esc => {
                app.secret.list.query.clear();
                app.secret.list.selected = 0;
                app.secret.list.scroll = 0;
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Enter => {
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Up => crate::app::keymap::move_selection(app, -1),
            KeyCode::Down => crate::app::keymap::move_selection(app, 1),
            KeyCode::PageUp => crate::app::keymap::move_selection(app, -10),
            KeyCode::PageDown => crate::app::keymap::move_selection(app, 10),
            _ => {
                let before = app.secret.list.query.value().to_string();
                if app.secret.list.query.handle_key(&key) && app.secret.list.query.value() != before
                {
                    app.secret.list.selected = 0;
                    app.secret.list.scroll = 0;
                }
            }
        }
        return;
    }
    match key.code {
        KeyCode::Esc => {
            app.esv.reset_view();
            app.input_mode = InputMode::Normal;
            return;
        }
        KeyCode::Enter => {
            app.input_mode = InputMode::Normal;
            return;
        }
        KeyCode::Up => {
            crate::app::keymap::move_selection(app, -1);
            return;
        }
        KeyCode::Down => {
            crate::app::keymap::move_selection(app, 1);
            return;
        }
        KeyCode::PageUp => {
            crate::app::keymap::move_selection(app, -10);
            return;
        }
        KeyCode::PageDown => {
            crate::app::keymap::move_selection(app, 10);
            return;
        }
        _ => {}
    }
    let before = app.esv.list.query.value().to_string();
    if app.esv.list.query.handle_key(&key) && app.esv.list.query.value() != before {
        app.esv.list.selected = 0;
        app.esv.list.scroll = 0;
    }
}

/// Open the edit form for the currently-selected list row. Snapshots the
/// variable so we have something to diff against on save, decodes the
/// base64 value, and switches input mode.
pub fn start_edit(app: &mut App) {
    let Some(tenant) = app.active_tenant() else {
        return;
    };
    let tenant_name = tenant.name.clone();
    let matches = app.esv.matches(Some(&tenant_name));
    let Some(m) = matches.get(app.esv.list.selected) else {
        return;
    };
    if m.deleted {
        app.push_toast(ToastKind::Info, "Deleted variable; press ^Z to restore it");
        return;
    }
    if app
        .esv
        .in_flight_writes
        .contains(&(tenant_name.clone(), m.id.clone()))
    {
        app.push_toast(
            ToastKind::Info,
            format!("Save already in progress: {}", m.id),
        );
        return;
    }
    let Some(LoadState::Loaded(items)) = app.esv.list.data.get(&tenant_name) else {
        return;
    };
    let Some(idx) = m.idx else { return };
    let Some(v) = items.get(idx).cloned() else {
        return;
    };

    let description = v
        .get("description")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let expr_type = ExpressionType::parse(
        v.get("expressionType")
            .and_then(|x| x.as_str())
            .unwrap_or(""),
    );
    let value_b64 = v.get("valueBase64").and_then(|x| x.as_str()).unwrap_or("");
    // Try to render the value as UTF-8 text. Binary values fall back to
    // the base64 string itself — they can still be edited (the save path
    // re-encodes whatever we display), they just won't look pretty.
    let value_str = match B64.decode(value_b64) {
        Ok(bytes) => String::from_utf8(bytes).unwrap_or_else(|e| {
            tracing::debug!(id = %m.id, "value is not UTF-8: {e}");
            value_b64.to_string()
        }),
        Err(_) => value_b64.to_string(),
    };

    app.esv.editing = Some(EditState {
        id: m.id.clone(),
        original: v,
        creating: false,
        id_input: TextField::single_line("_id"),
        description: TextField::single_line("Description").with_initial(description),
        expr_type,
        value: TextField::textarea("Value").with_initial(value_str),
        focused: EditField::Description,
        error: None,
    });
    app.input_mode = InputMode::Esv(Mode::Edit);
}

/// Open the create-new-variable form. Same fields as edit, except `_id`
/// is now an editable field at the top and the save path is a plain PUT
/// (no DELETE step, no conflict refetch — server creates if absent).
pub fn start_create(app: &mut App) {
    if !app.is_unlocked() {
        return;
    }
    if app.active_tenant().is_none() {
        return;
    }
    app.esv.editing = Some(EditState {
        id: String::new(),
        original: serde_json::Value::Null,
        creating: true,
        // The `esv-` prefix is required by AIC, so lock it in: the user
        // types only the suffix and can't delete the prefix.
        id_input: TextField::single_line("_id").with_locked_prefix("esv-"),
        description: TextField::single_line("Description"),
        expr_type: ExpressionType::String,
        value: TextField::textarea("Value"),
        focused: EditField::Id,
        error: None,
    });
    app.input_mode = InputMode::Esv(Mode::Edit);
}

pub fn row_count(app: &App) -> usize {
    app.esv_matches().len()
}

pub fn current_selection(app: &App) -> usize {
    app.esv.list.selected
}

pub fn set_selection(app: &mut App, idx: usize) {
    app.esv.list.selected = idx;
}

pub fn filter_active(app: &App) -> bool {
    !app.esv.list.query.is_empty()
}

pub fn clear_filter(app: &mut App) {
    app.esv.reset_view();
}

pub fn primary(app: &mut App) {
    start_edit(app);
}

pub fn delete(app: &mut App) {
    ops::request_delete(app);
}

pub fn new_item(app: &mut App) {
    start_create(app);
}

pub fn help_lines(mode: Mode, app: &App) -> Option<Vec<(&'static str, &'static str)>> {
    match mode {
        Mode::Search => Some(vec![
            ("Type", "edit search query"),
            ("Backspace", "delete character"),
            ("Enter", "keep filter and return to list"),
            ("Esc", "clear filter and return to list"),
            ("↑/↓", "move selection"),
            ("PgUp/PgDn", "move by page"),
            ("F1", "show keybinds"),
        ]),
        Mode::Edit => {
            let mut out = vec![("Tab", "navigate")];
            let focused = app.esv.editing.as_ref().map(|edit| edit.focused);
            match focused {
                Some(EditField::Id | EditField::Description | EditField::Type) => {
                    out.push(("Enter", "next"));
                }
                Some(EditField::Save) => out.push(("Enter", "save")),
                _ => {}
            }
            if focused == Some(EditField::Type) {
                out.push(("←/→", "change type"));
            }
            out.push(("Esc", "cancel"));
            Some(out)
        }
        Mode::RestartConfirm => Some(vec![("y", "restart tenant runtime"), ("n/Esc", "cancel")]),
        Mode::DeleteConfirm => Some(vec![("y", "delete variable"), ("n/Esc", "cancel")]),
    }
}

pub fn mappings_subview_active(app: &App) -> bool {
    app.active_view == crate::app::View::Esvs
        && app.esv.view.clamp(
            app.active_tenant()
                .is_some_and(|tenant| tenant.allows_secret_mappings()),
        ) == EsvView::Mappings
}

pub fn edit_field_active(app: &App) -> bool {
    app.esv.editing.is_some()
}

pub fn current_view(app: &App) -> EsvView {
    app.esv.view.clamp(
        app.active_tenant()
            .is_some_and(|tenant| tenant.allows_secret_mappings()),
    )
}

pub fn edit_focused(app: &App) -> Option<EditField> {
    app.esv.editing.as_ref().map(|edit| edit.focused)
}

/// Discard the in-flight edit and return to preview mode.
pub fn cancel_edit(app: &mut App) {
    app.esv.editing = None;
    app.input_mode = InputMode::Normal;
}

pub fn handle_edit_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    if is_save_chord(&key) {
        commit_save(app);
        return Ok(());
    }
    let Some(edit) = app.esv.editing.as_mut() else {
        return Ok(());
    };
    let creating = edit.creating;
    // Keys that the form owns regardless of which field is focused.
    match key.code {
        KeyCode::Esc => {
            cancel_edit(app);
            return Ok(());
        }
        KeyCode::Tab => {
            edit.focused = edit.focused.next(creating);
            return Ok(());
        }
        KeyCode::BackTab => {
            edit.focused = edit.focused.prev(creating);
            return Ok(());
        }
        KeyCode::Enter => {
            match edit.focused {
                EditField::Save => commit_save(app),
                EditField::Value => edit.value.push_newline(),
                // Enter on a non-textarea field advances focus.
                _ => edit.focused = edit.focused.next(creating),
            }
            return Ok(());
        }
        // ←/→ cycle the chip on the Type row; on any other field they
        // fall through to the TextField's cursor nav below.
        KeyCode::Left if edit.focused == EditField::Type => {
            edit.expr_type = edit.expr_type.cycle(-1);
            return Ok(());
        }
        KeyCode::Right if edit.focused == EditField::Type => {
            edit.expr_type = edit.expr_type.cycle(1);
            return Ok(());
        }
        _ => {}
    }

    // Everything else is per-field text editing — cursor moves, char
    // inserts, backspace, delete-forward.
    match edit.focused {
        EditField::Id if creating => {
            edit.id_input.handle_key(&key);
        }
        EditField::Description => {
            edit.description.handle_key(&key);
        }
        EditField::Value => {
            edit.value.handle_key(&key);
        }
        _ => {}
    }
    Ok(())
}

fn commit_save(app: &mut App) {
    let Some(plan) = ops::build_save_plan(app) else {
        return;
    };
    let is_prod = app
        .active_tenant()
        .is_some_and(|t| t.theme == TenantTheme::Production);
    if is_prod {
        app.prod_confirm.pending = Some(PendingProdAction::Esv(ProdAction::Save(plan)));
        app.input_mode = InputMode::ProdConfirm;
        return;
    }
    ops::execute_save_plan(app, plan, false);
}
