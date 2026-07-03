//! OAuth2 read-only tab interaction: list refresh, incremental search,
//! selection, and lazy client-detail loading.

use crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;

use crate::app::event::ToastKind;
use crate::app::{App, InputMode};
use crate::oauth::ops;
use crate::oauth::state::{LoadState, State};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    Search,
}

#[derive(Debug)]
pub enum Event {
    ListLoaded {
        tenant: String,
        clients: Vec<String>,
    },
    ClientLoaded {
        tenant: String,
        id: String,
        client: Value,
    },
    LoadFailed {
        tenant: String,
        id: Option<String>,
        message: String,
    },
}

pub fn apply_event(app: &mut App, event: Event) {
    match event {
        Event::ListLoaded { tenant, clients } => {
            app.oauth.refreshing.remove(&tenant);
            app.oauth
                .data
                .insert(tenant.clone(), LoadState::Loaded(clients));
            if app
                .active_tenant()
                .is_some_and(|active| active.name == tenant)
            {
                let count = row_count(app);
                app.oauth.clamp_selection(count);
                load_selected(app);
            }
        }
        Event::ClientLoaded { tenant, id, client } => {
            let key = State::detail_key(&tenant, &id);
            app.oauth.detail_loading.remove(&key);
            app.oauth.detail_failed.remove(&key);
            app.oauth.detail_cache.insert(key, client);
        }
        Event::LoadFailed {
            tenant,
            id,
            message,
        } => {
            if let Some(id) = id {
                let key = State::detail_key(&tenant, &id);
                app.oauth.detail_loading.remove(&key);
                app.oauth.detail_failed.insert(key, message.clone());
                if selected_id(app).as_deref() == Some(id.as_str()) {
                    app.push_toast(
                        ToastKind::Error,
                        format!("OAuth client load failed: {message}"),
                    );
                }
            } else {
                app.oauth.refreshing.remove(&tenant);
                app.oauth
                    .data
                    .insert(tenant.clone(), LoadState::Failed(message.clone()));
                if app
                    .active_tenant()
                    .is_some_and(|active| active.name == tenant)
                {
                    app.push_toast(ToastKind::Error, format!("OAuth list failed: {message}"));
                }
            }
        }
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) {
    match mode {
        Mode::Normal => handle_normal_key(app, key),
        Mode::Search => handle_search_key(app, key),
    }
}

pub fn footer_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let InputMode::Oauth(mode) = app.input_mode else {
        return Vec::new();
    };
    match mode {
        Mode::Normal => vec![("Esc", "back")],
        Mode::Search => vec![("Enter", "keep filter"), ("Esc", "clear + exit")],
    }
}

pub fn help_lines(mode: Mode) -> Option<Vec<(&'static str, &'static str)>> {
    match mode {
        Mode::Normal => Some(vec![
            ("↑/↓", "move selection"),
            ("Enter", "load selected client"),
            ("^U/^D", "scroll detail"),
            ("R", "refresh"),
            ("Ctrl-P", "open function selector"),
            ("Esc", "back"),
        ]),
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
    if force {
        if let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) {
            app.oauth.clear_tenant_details(&tenant);
        }
    }
    ops::load_list(app, force);
}

pub fn start_search(app: &mut App) {
    app.input_mode = InputMode::Oauth(Mode::Search);
}

pub fn row_count(app: &App) -> usize {
    app.oauth
        .matches(app.active_tenant().map(|tenant| tenant.name.as_str()))
        .len()
}

pub fn current_selection(app: &App) -> usize {
    app.oauth.selected
}

pub fn select(app: &mut App, idx: usize) {
    app.oauth.select(idx);
    load_selected(app);
}

pub fn selected_id(app: &App) -> Option<String> {
    app.oauth
        .selected_id(app.active_tenant().map(|tenant| tenant.name.as_str()))
}

pub fn load_selected(app: &mut App) {
    let Some(id) = selected_id(app) else {
        return;
    };
    ops::load_client(app, id, false);
}

pub fn scroll_detail(app: &mut App, delta: isize) {
    if delta.is_negative() {
        app.oauth.detail_scroll = app.oauth.detail_scroll.saturating_sub(delta.unsigned_abs());
    } else {
        app.oauth.detail_scroll = app.oauth.detail_scroll.saturating_add(delta as usize);
    }
}

pub fn clear_filter(app: &mut App) {
    app.oauth.reset_view();
    load_selected(app);
}

pub fn filter_active(app: &App) -> bool {
    !app.oauth.query.is_empty()
}

pub fn primary(app: &mut App) {
    load_selected(app);
}

pub fn delete(_app: &mut App) {}

pub fn new_item(_app: &mut App) {}

fn handle_normal_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.input_mode = InputMode::Normal,
        KeyCode::PageUp => scroll_detail(app, -10),
        KeyCode::PageDown => scroll_detail(app, 10),
        KeyCode::Up | KeyCode::Char('k') => crate::app::keymap::move_selection(app, -1),
        KeyCode::Down | KeyCode::Char('j') => crate::app::keymap::move_selection(app, 1),
        KeyCode::Enter => load_selected(app),
        KeyCode::Char('p')
            if key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL) =>
        {
            crate::app::selector::open(app);
        }
        KeyCode::Char('R') => refresh(app, true),
        _ => {}
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
        KeyCode::Up => return crate::app::keymap::move_selection(app, -1),
        KeyCode::Down => return crate::app::keymap::move_selection(app, 1),
        KeyCode::PageUp => return crate::app::keymap::move_selection(app, -10),
        KeyCode::PageDown => return crate::app::keymap::move_selection(app, 10),
        _ => {}
    }

    let before = app.oauth.query.value().to_string();
    if app.oauth.query.handle_key(&key) && app.oauth.query.value() != before {
        app.oauth.selected = 0;
        app.oauth.scroll = 0;
        app.oauth.detail_scroll = 0;
        load_selected(app);
    }
}
