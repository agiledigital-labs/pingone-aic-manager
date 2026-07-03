//! IDM record store tab interaction.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, InputMode};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Search,
}

#[derive(Debug)]
pub enum Event {}

pub fn apply_event(_app: &mut App, event: Event) {
    match event {}
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) {
    match mode {
        Mode::Search => handle_search_key(app, key),
    }
}

pub fn footer_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    match app.input_mode {
        InputMode::IdmStore(Mode::Search) => vec![("Enter", "keep filter"), ("Esc", "back")],
        _ => Vec::new(),
    }
}

pub fn help_lines(mode: Mode) -> Option<Vec<(&'static str, &'static str)>> {
    match mode {
        Mode::Search => Some(vec![
            ("Enter", "keep filter"),
            ("Esc", "back"),
            ("↑/↓", "move selection"),
            ("PgUp/PgDn", "move by page"),
            ("F1", "show keybinds"),
        ]),
    }
}

pub fn refresh(app: &mut App, force: bool) {
    crate::idmstore::ops::refresh(app, force);
}

pub fn row_count(_app: &App) -> usize {
    0
}

pub fn current_selection(_app: &App) -> usize {
    0
}

pub fn select(_app: &mut App, _idx: usize) {}

pub fn clear_filter(app: &mut App) {
    app.idmstore.reset_view();
}

pub fn filter_active(_app: &App) -> bool {
    false
}

pub fn primary(_app: &mut App) {}

pub fn delete(_app: &mut App) {}

pub fn new_item(_app: &mut App) {}

fn handle_search_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => app.input_mode = InputMode::Normal,
        _ => {}
    }
}
