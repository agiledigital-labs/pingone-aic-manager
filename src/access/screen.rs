//! Access-tab interaction: lazy whole-document refresh, search, and selection.

use crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;

use crate::access::state::{Document, LoadState};
use crate::app::event::{AppEvent, ToastKind};
use crate::app::{App, InputMode, View};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Search,
}

#[derive(Debug)]
pub enum Event {
    Loaded {
        tenant: String,
        result: std::result::Result<Value, String>,
    },
}

pub fn apply_event(app: &mut App, event: Event) {
    match event {
        Event::Loaded { tenant, result } => {
            app.access.refreshing.remove(&tenant);
            apply_refresh(app, tenant, result);
        }
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) {
    match mode {
        Mode::Search => handle_search_key(app, key),
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
        _ => Vec::new(),
    }
}

pub fn help_lines(mode: Mode) -> Option<Vec<(&'static str, &'static str)>> {
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

pub fn filter_active(app: &App) -> bool {
    !app.access.query.is_empty()
}

pub fn clear_filter(app: &mut App) {
    app.access.reset_view();
}

pub fn primary(_app: &mut App) {}

pub fn delete(_app: &mut App) {}

pub fn new_item(_app: &mut App) {}

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
    }
}

fn move_selection(app: &mut App, delta: isize) {
    let count = row_count(app);
    app.access.move_selection(count, delta);
}
