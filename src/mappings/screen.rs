//! Mappings view interaction: lazy list refresh, incremental search, and
//! selection.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::event::ToastKind;
use crate::app::{App, InputMode, View};
use crate::mappings::api::{self, MappingSummary};
use crate::mappings::state::ReconView;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Search,
}

#[derive(Debug)]
pub enum Event {
    Listed {
        tenant: String,
        result: std::result::Result<Vec<MappingSummary>, String>,
    },
    ReconStatus {
        tenant: String,
        mapping: String,
        status: std::result::Result<api::ReconStatus, String>,
    },
    PullResult {
        tenant: String,
        mapping: String,
        result: std::result::Result<String, String>,
    },
}

pub fn apply_event(app: &mut App, event: Event) {
    match event {
        Event::Listed { tenant, result } => {
            app.mappings.refreshing.remove(&tenant);
            crate::mappings::ops::apply_refresh(app, tenant, result);
        }
        Event::ReconStatus {
            tenant,
            mapping,
            status,
        } => apply_recon_status(app, tenant, mapping, status),
        Event::PullResult {
            tenant,
            mapping,
            result,
        } => apply_pull_result(app, tenant, mapping, result),
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) {
    match mode {
        Mode::Search => handle_search_key(app, key),
    }
}

pub fn footer_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    match app.input_mode {
        InputMode::Normal if app.active_view == View::Mappings => {
            vec![("r", "reconcile"), ("p", "pull scripts"), ("R", "refresh")]
        }
        InputMode::Mappings(Mode::Search) => vec![
            ("↑/↓", "navigate"),
            ("Enter", "keep filter"),
            ("Esc", "clear + exit"),
        ],
        _ => Vec::new(),
    }
}

pub fn refresh(app: &mut App, force: bool) {
    crate::mappings::ops::refresh(app, force);
}

pub fn apply_refresh(
    app: &mut App,
    tenant: String,
    result: std::result::Result<Vec<MappingSummary>, String>,
) {
    crate::mappings::ops::apply_refresh(app, tenant, result);
}

pub fn run_recon(app: &mut App) {
    crate::mappings::ops::run_recon(app);
}

pub fn pull_scripts(app: &mut App) {
    crate::mappings::ops::pull_scripts(app);
}

pub fn row_count(app: &App) -> usize {
    app.mappings
        .matches(app.active_tenant().map(|tenant| tenant.name.as_str()))
        .len()
}

pub fn current_selection(app: &App) -> usize {
    app.mappings.selected
}

pub fn select(app: &mut App, idx: usize) {
    app.mappings.select(idx);
}

pub fn clear_filter(app: &mut App) {
    app.mappings.reset_view();
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

    let before = app.mappings.query.value().to_string();
    if app.mappings.query.handle_key(&key) && app.mappings.query.value() != before {
        app.mappings.selected = 0;
        app.mappings.scroll = 0;
    }
}

fn apply_recon_status(
    app: &mut App,
    tenant: String,
    mapping: String,
    status: std::result::Result<api::ReconStatus, String>,
) {
    let key = (tenant.clone(), mapping.clone());
    match status {
        Ok(status) => {
            let terminal = api::state_is_terminal(&status.state);
            let message = terminal_recon_message(&mapping, &status);
            let toast_kind = recon_toast_kind(&status.state);
            app.mappings
                .recon
                .insert(key.clone(), ReconView { last: status });
            if terminal {
                app.mappings.in_flight_recon.remove(&key);
                app.push_toast(toast_kind, message);
            }
        }
        Err(error) => {
            app.mappings.in_flight_recon.remove(&key);
            app.push_toast(
                ToastKind::Error,
                format!("reconciliation failed for {mapping}: {error}"),
            );
        }
    }
}

fn apply_pull_result(
    app: &mut App,
    tenant: String,
    mapping: String,
    result: std::result::Result<String, String>,
) {
    let key = (tenant.clone(), mapping.clone());
    app.mappings.in_flight_pull.remove(&key);
    match result {
        Ok(message) => {
            crate::scripts::screen::invalidate_tenant(app, &tenant);
            let kind = if message.contains("no inline scripts") {
                ToastKind::Info
            } else {
                ToastKind::Success
            };
            app.push_toast(kind, message);
        }
        Err(error) => {
            app.push_toast(
                ToastKind::Error,
                format!("pull scripts failed for {mapping}: {error}"),
            );
        }
    }
}

fn recon_toast_kind(state: &str) -> ToastKind {
    match state {
        "SUCCESS" => ToastKind::Success,
        "CANCELED" => ToastKind::Warning,
        _ => ToastKind::Error,
    }
}

fn terminal_recon_message(mapping: &str, status: &api::ReconStatus) -> String {
    // The server's stage description already reads as a sentence
    // ("reconciliation completed." / "reconciliation failed"), so lead with the
    // mapping and use it verbatim rather than prefixing a redundant
    // "reconciliation {STATE}:". Fall back to the state when it's empty.
    if status.stage_description.is_empty() {
        format!("{mapping}: reconciliation {}", status.state.to_lowercase())
    } else {
        format!(
            "{mapping}: {}",
            status.stage_description.trim_end_matches('.')
        )
    }
}
