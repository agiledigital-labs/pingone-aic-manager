//! Managed-objects tab interaction: lazy loading, fuzzy search, and selection.

use crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;

use crate::app::event::AppEvent;
use crate::app::{App, InputMode};
use crate::managed::state::LoadState;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Search,
}

#[derive(Debug)]
pub enum Event {
    Listed {
        tenant: String,
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
            }
        }
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) {
    match mode {
        Mode::Search => handle_search_key(app, key),
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
    }
}
