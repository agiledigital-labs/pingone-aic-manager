//! Access-tab interaction: lazy refresh, selection, editable forms, and confirms.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;

use crate::access::ops;
use crate::access::state::{Document, LoadState};
use crate::app::event::{AppEvent, ToastKind};
use crate::app::{App, InputMode, View};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Search,
    Create,
    Edit,
    DeleteConfirm,
}

pub const FLAGS_LEGEND: [(&str, &str); 2] = [
    ("A", "grant is gated by a customAuthz script"),
    ("D", "rule is byte-identical to another rule"),
];

#[derive(Debug)]
pub enum Event {
    Loaded {
        tenant: String,
        result: std::result::Result<Value, String>,
    },
    WriteResult {
        tenant: String,
        after: Value,
        undo_id: crate::undo::UndoId,
        resume_mode: ops::ResumeMode,
        result: std::result::Result<(), ops::WriteFailure>,
    },
    UndoResult {
        tenant: String,
        undo_id: crate::undo::UndoId,
        result: std::result::Result<Value, ops::UndoFailure>,
    },
}

pub fn apply_event(app: &mut App, event: Event) {
    match event {
        Event::Loaded { tenant, result } => {
            app.access.refreshing.remove(&tenant);
            apply_refresh(app, tenant, result);
        }
        Event::WriteResult {
            tenant,
            after,
            undo_id,
            resume_mode,
            result,
        } => ops::apply_write_result(app, tenant, after, undo_id, resume_mode, result),
        Event::UndoResult {
            tenant,
            undo_id,
            result,
        } => ops::apply_undo_result(app, tenant, undo_id, result),
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) {
    match mode {
        Mode::Search => handle_search_key(app, key),
        Mode::Create | Mode::Edit => handle_form_key(app, key, mode),
        Mode::DeleteConfirm => handle_delete_confirm_key(app, key),
    }
}

pub fn footer_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    match app.input_mode {
        InputMode::Normal if app.active_view == View::Access => vec![("^R", "refresh")],
        InputMode::Access(Mode::Search) => vec![
            ("↑/↓", "navigate"),
            ("Enter", "keep filter"),
            ("Esc", "clear + exit"),
        ],
        InputMode::Access(Mode::Create | Mode::Edit) => Vec::new(),
        InputMode::Access(Mode::DeleteConfirm) => Vec::new(),
        _ => Vec::new(),
    }
}

pub fn help_lines(mode: Mode) -> Option<Vec<(&'static str, &'static str)>> {
    match mode {
        Mode::Search => Some(vec![
            FLAGS_LEGEND[0],
            FLAGS_LEGEND[1],
            ("Type", "edit search query"),
            ("Backspace", "delete character"),
            ("Enter", "keep filter and return to list"),
            ("Esc", "clear filter and return to list"),
            ("↑/↓", "move selection"),
            ("PgUp/PgDn", "move by page"),
            ("F1", "show keybinds"),
        ]),
        Mode::Create => Some(vec![
            ("Type", "edit focused field; ? is text"),
            ("Tab/Shift-Tab", "move between fields"),
            ("Enter", "advance, or save on the Save row"),
            ("^S", "review the change"),
            ("y", "confirm after review"),
            ("n", "return from review to the form"),
            ("Esc", "cancel; from review, return to the form"),
            ("F1", "show keybinds"),
        ]),
        Mode::Edit => Some(vec![
            ("Type", "edit focused field; ? is text"),
            ("Tab/Shift-Tab", "move between fields"),
            ("Enter", "advance, or save on the Save row"),
            ("^S", "review the change"),
            ("^X", "clear the focused optional key"),
            ("^U", "leave the focused optional key unchanged"),
            ("y", "confirm after review"),
            ("n", "return from review to the form"),
            ("Esc", "cancel; from review, return to the form"),
            ("F1", "show keybinds"),
        ]),
        Mode::DeleteConfirm => Some(vec![
            ("y", "delete the selected indexed rule"),
            ("n/Esc", "cancel"),
            ("F1", "show keybinds"),
        ]),
    }
}

pub fn refresh(app: &mut App, force: bool) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if !app.is_unlocked()
        || app.access.refreshing.contains(&tenant)
        || (!force && app.access.data.contains_key(&tenant))
    {
        return;
    }

    app.access.data.insert(tenant.clone(), LoadState::Loading);
    app.access.refreshing.insert(tenant.clone());
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::access::api::get_access(&tenant)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Access(Event::Loaded { tenant, result }));
    });
}

pub fn row_count(app: &App) -> usize {
    app.access
        .matches(app.active_tenant().map(|tenant| tenant.name.as_str()))
        .len()
}

pub fn current_selection(app: &App) -> usize {
    app.access.selected
}

pub fn select(app: &mut App, index: usize) {
    let count = row_count(app);
    app.access.select(index, count);
}

pub fn scroll_detail(app: &mut App, delta: isize) {
    app.access.detail_scroll.scroll(delta);
}

pub fn filter_active(app: &App) -> bool {
    !app.access.query.is_empty()
}

pub fn clear_filter(app: &mut App) {
    app.access.reset_view();
}

pub fn primary(app: &mut App) {
    start_edit(app);
}

pub fn delete(app: &mut App) {
    start_delete(app);
}

pub fn new_item(app: &mut App) {
    start_create(app);
}

fn start_create(app: &mut App) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if app.access.in_flight_writes.contains(&tenant) {
        app.push_toast(ToastKind::Info, "An Access write is already in progress");
        return;
    }
    let Some(document) = app.access.document(&tenant) else {
        return;
    };
    app.access.form = Some(crate::access::state::RuleFormState::create(
        tenant, document,
    ));
    app.input_mode = InputMode::Access(Mode::Create);
}

fn start_edit(app: &mut App) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if app.access.in_flight_writes.contains(&tenant) {
        app.push_toast(ToastKind::Info, "An Access write is already in progress");
        return;
    }
    let matches = app.access.matches(Some(&tenant));
    let selected = app.access.selected.min(matches.len().saturating_sub(1));
    let Some(row) = matches.get(selected).map(|item| item.row.clone()) else {
        return;
    };
    let Some(document) = app.access.document(&tenant) else {
        return;
    };
    app.access.form = Some(crate::access::state::RuleFormState::edit(
        tenant, document, &row,
    ));
    app.input_mode = InputMode::Access(Mode::Edit);
}

fn start_delete(app: &mut App) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if app.access.in_flight_writes.contains(&tenant) {
        app.push_toast(ToastKind::Info, "An Access write is already in progress");
        return;
    }
    let matches = app.access.matches(Some(&tenant));
    let selected = app.access.selected.min(matches.len().saturating_sub(1));
    let Some(row) = matches.get(selected).map(|item| item.row.clone()) else {
        return;
    };
    let Some(document) = app.access.document(&tenant) else {
        return;
    };
    app.access.pending_delete = Some(crate::access::state::DeleteState::new(
        tenant, document, &row,
    ));
    app.input_mode = InputMode::Access(Mode::DeleteConfirm);
}

fn apply_refresh(app: &mut App, tenant: String, result: std::result::Result<Value, String>) {
    let result =
        result.and_then(|value| Document::from_value(value).map_err(|error| error.to_string()));
    match result {
        Ok(document) => {
            app.access
                .data
                .insert(tenant.clone(), LoadState::Loaded(document));
        }
        Err(error) => {
            app.access
                .data
                .insert(tenant.clone(), LoadState::Failed(error.clone()));
            if app
                .active_tenant()
                .is_some_and(|active| active.name == tenant)
            {
                app.push_toast(ToastKind::Error, format!("Access rules failed: {error}"));
            }
        }
    }

    if app
        .active_tenant()
        .is_some_and(|active| active.name == tenant)
    {
        let count = row_count(app);
        app.access.clamp_selection(count);
    }
}

fn handle_search_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            clear_filter(app);
            app.input_mode = InputMode::Normal;
            return;
        }
        KeyCode::Enter => {
            app.input_mode = InputMode::Normal;
            return;
        }
        KeyCode::Up => return move_selection(app, -1),
        KeyCode::Down => return move_selection(app, 1),
        KeyCode::PageUp => return move_selection(app, -10),
        KeyCode::PageDown => return move_selection(app, 10),
        _ => {}
    }

    let before = app.access.query.value().to_string();
    if app.access.query.handle_key(&key) && app.access.query.value() != before {
        app.access.selected = 0;
        app.access.scroll = 0;
        app.access.detail_scroll.reset();
    }
}

fn handle_form_key(app: &mut App, key: KeyEvent, mode: Mode) {
    let confirming = app.access.form.as_ref().is_some_and(|form| form.confirming);
    if confirming {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let request = app.access.form.as_ref().map(ops::request_from_form);
                match request {
                    Some(Ok(request)) => ops::submit_write(app, request),
                    Some(Err(error)) => {
                        if let Some(form) = app.access.form.as_mut() {
                            form.confirming = false;
                            form.error = Some(error.to_string());
                        }
                    }
                    None => app.input_mode = InputMode::Normal,
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                if let Some(form) = app.access.form.as_mut() {
                    form.confirming = false;
                }
            }
            _ => {}
        }
        return;
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Esc => {
            app.access.form = None;
            app.input_mode = InputMode::Normal;
            return;
        }
        KeyCode::Tab => {
            if let Some(form) = app.access.form.as_mut() {
                form.focused = form.focused.next();
            }
            return;
        }
        KeyCode::BackTab => {
            if let Some(form) = app.access.form.as_mut() {
                form.focused = form.focused.prev();
            }
            return;
        }
        KeyCode::Char('x') if ctrl && mode == Mode::Edit => {
            if let Some(field) = app
                .access
                .form
                .as_mut()
                .and_then(|form| form.optional_mut())
            {
                field.set_clear();
            }
            return;
        }
        KeyCode::Char('u') if ctrl && mode == Mode::Edit => {
            if let Some(field) = app
                .access
                .form
                .as_mut()
                .and_then(|form| form.optional_mut())
            {
                field.set_unchanged();
            }
            return;
        }
        KeyCode::Char('s') if ctrl => {
            review_form(app);
            return;
        }
        KeyCode::Enter => {
            if app
                .access
                .form
                .as_ref()
                .is_some_and(|form| form.focused == crate::access::state::FormFocus::Save)
            {
                review_form(app);
            } else if let Some(form) = app.access.form.as_mut() {
                form.focused = form.focused.next();
            }
            return;
        }
        _ => {}
    }

    let Some(form) = app.access.form.as_mut() else {
        return;
    };
    match form.focused {
        crate::access::state::FormFocus::Pattern => {
            form.pattern.handle_key(&key);
        }
        crate::access::state::FormFocus::Roles => {
            form.roles.handle_key(&key);
        }
        crate::access::state::FormFocus::Methods => {
            form.methods.handle_key(&key);
        }
        crate::access::state::FormFocus::Actions
        | crate::access::state::FormFocus::CustomAuthz
        | crate::access::state::FormFocus::ExcludePatterns => {
            if let Some(field) = form.optional_mut() {
                field.handle_key(&key);
            }
        }
        crate::access::state::FormFocus::Save => {}
    }
}

fn review_form(app: &mut App) {
    let Some(form) = app.access.form.as_mut() else {
        return;
    };
    match ops::request_from_form(form) {
        Ok(_) => {
            form.error = None;
            form.confirming = true;
        }
        Err(error) => form.error = Some(error.to_string()),
    }
}

fn handle_delete_confirm_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let request = if let Some(delete) = app.access.pending_delete.as_mut() {
                delete.confirm();
                ops::request_from_delete(delete)
            } else {
                return;
            };
            match request {
                Ok(request) => ops::submit_write(app, request),
                Err(error) => app.push_toast(ToastKind::Error, error.to_string()),
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.access.pending_delete = None;
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
}

fn move_selection(app: &mut App, delta: isize) {
    let count = row_count(app);
    app.access.move_selection(count, delta);
}
