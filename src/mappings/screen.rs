//! Mappings view interaction: lazy list refresh, incremental search, and
//! selection.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::event::ToastKind;
use crate::app::{App, InputMode, View};
use crate::mappings::api::{self, MappingSummary};
use crate::mappings::state::{PendingPull, ReconView};
use crate::scripts::sync;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Search,
    PullConfirm,
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
    PullPrepared {
        tenant: String,
        mapping: String,
        result: std::result::Result<sync::PullPlan, String>,
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
        Event::PullPrepared {
            tenant,
            mapping,
            result,
        } => apply_pull_prepared(app, tenant, mapping, result),
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) {
    match mode {
        Mode::Search => handle_search_key(app, key),
        Mode::PullConfirm => handle_pull_confirm_key(app, key),
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
        InputMode::Mappings(Mode::PullConfirm) => vec![
            ("y", "overwrite every listed local edit with backups"),
            ("n/Enter/Esc", "cancel the whole pull (default)"),
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
        Mode::PullConfirm => Some(vec![
            ("y", "overwrite every listed local edit with backups"),
            ("n/Enter/Esc", "cancel the whole pull (default)"),
            ("F1", "show keybinds"),
        ]),
    }
}

pub fn mappings_view_active(app: &App) -> bool {
    app.active_view == View::Mappings
}

pub fn mappings_subview_active(app: &App) -> bool {
    app.active_view == View::Esvs
        && app.esv.view.clamp(is_mappings_allowed(app)) == crate::esv::state::EsvView::Mappings
}

fn is_mappings_allowed(app: &App) -> bool {
    app.active_tenant()
        .is_some_and(|tenant| tenant.allows_secret_mappings())
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

pub fn filter_active(app: &App) -> bool {
    !app.mappings.query.is_empty()
}

pub fn primary(app: &mut App) {
    crate::mappings::ops::run_recon(app);
}

pub fn delete(_app: &mut App) {}

pub fn new_item(_app: &mut App) {}

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

fn apply_pull_prepared(
    app: &mut App,
    tenant: String,
    mapping: String,
    result: std::result::Result<sync::PullPlan, String>,
) {
    if app
        .active_tenant()
        .is_none_or(|active| active.name != tenant)
    {
        app.mappings.in_flight_pull.remove(&(tenant, mapping));
        return;
    }
    let plan = match result {
        Ok(plan) => plan,
        Err(error) => {
            apply_pull_result(app, tenant, mapping, Err(error));
            return;
        }
    };
    if plan.protected_refs().is_empty() {
        apply_pull_plan(app, tenant, mapping, plan);
        return;
    }
    // The confirm modal's accept key is `y`. Opening it from this
    // async handler would steal a keystroke from Search (or any other
    // mode). Store the plan; `promote_pending_pulls` opens the modal
    // only while `input_mode` is already Normal.
    let waiting = app.input_mode != InputMode::Normal;
    app.mappings.pending_pull = Some(PendingPull {
        tenant,
        mapping,
        plan,
    });
    if waiting {
        app.push_toast(
            ToastKind::Info,
            "Protected pull is waiting for confirmation",
        );
    }
}

pub(crate) fn promote_pending_pull(app: &mut App) {
    if app.input_mode != InputMode::Normal || app.mappings.pending_pull.is_none() {
        return;
    }
    app.input_mode = InputMode::Mappings(Mode::PullConfirm);
}

fn apply_pull_plan(app: &mut App, tenant: String, mapping: String, plan: sync::PullPlan) {
    let count = plan.len();
    let result = plan.install(false).map(|outcomes| {
        if count == 0 {
            return format!("{mapping} has no inline scripts");
        }
        let backups = outcomes
            .iter()
            .filter(|outcome| matches!(outcome.status, sync::PullStatus::LocalBackedUp(_)))
            .count();
        if backups == 0 {
            format!("pulled {count} scripts for {mapping}")
        } else {
            format!("pulled {count} scripts for {mapping} ({backups} local source(s) backed up)")
        }
    });
    apply_pull_result(
        app,
        tenant,
        mapping,
        result.map_err(|error| error.to_string()),
    );
}

fn handle_pull_confirm_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let Some(pending) = app.mappings.pending_pull.take() else {
                app.input_mode = InputMode::Normal;
                return;
            };
            app.input_mode = InputMode::Normal;
            apply_pull_plan(app, pending.tenant, pending.mapping, pending.plan);
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Enter | KeyCode::Esc => {
            if let Some(pending) = app.mappings.pending_pull.take() {
                app.mappings
                    .in_flight_pull
                    .remove(&(pending.tenant, pending.mapping));
            }
            app.input_mode = InputMode::Normal;
            app.push_toast(ToastKind::Info, "Pull cancelled; local changes kept");
        }
        _ => {}
    }
}

pub fn pending_pull_refs(app: &App) -> Vec<String> {
    app.mappings
        .pending_pull
        .as_ref()
        .map(|pending| pending.plan.protected_refs())
        .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::View;
    use crate::config::tenant::{Provenance, Tenant, TenantTheme};
    use crossterm::event::KeyModifiers;

    fn app() -> App {
        App::for_test(
            vec![Tenant {
                name: "sandbox".into(),
                base_url: "https://tenant.example.com".into(),
                theme: TenantTheme::Sandbox,
                sa_id: None,
                scopes: Vec::new(),
                provenance: Provenance::default(),
            }],
            View::Mappings,
        )
    }

    #[test]
    fn protected_mapping_pull_opens_one_default_cancel_batch_modal() {
        let mut app = app();
        app.mappings
            .in_flight_pull
            .insert(("sandbox".into(), "map".into()));

        apply_pull_prepared(
            &mut app,
            "sandbox".into(),
            "map".into(),
            Ok(sync::PullPlan::protected_for_test(&[
                "map.onCreate",
                "map.transform.name",
            ])),
        );
        crate::app::promote_pending_pulls(&mut app);

        assert_eq!(app.input_mode, InputMode::Mappings(Mode::PullConfirm));
        assert_eq!(
            pending_pull_refs(&app),
            ["sync/map.onCreate", "sync/map.transform.name"]
        );

        handle_pull_confirm_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.mappings.pending_pull.is_none());
        assert!(app.mappings.in_flight_pull.is_empty());
    }

    #[test]
    fn protected_mapping_pull_does_not_replace_another_input_mode() {
        let mut app = app();
        app.input_mode = InputMode::Mappings(Mode::Search);
        app.mappings
            .in_flight_pull
            .insert(("sandbox".into(), "map".into()));

        apply_pull_prepared(
            &mut app,
            "sandbox".into(),
            "map".into(),
            Ok(sync::PullPlan::protected_for_test(&["map.onCreate"])),
        );

        assert_eq!(app.input_mode, InputMode::Mappings(Mode::Search));
        assert_eq!(pending_pull_refs(&app), ["sync/map.onCreate"]);
        assert!(
            app.toasts
                .iter()
                .any(|toast| toast.message.contains("waiting for confirmation"))
        );

        crate::app::promote_pending_pulls(&mut app);
        assert_eq!(app.input_mode, InputMode::Mappings(Mode::Search));
        assert!(app.mappings.pending_pull.is_some());

        app.input_mode = InputMode::Normal;
        crate::app::promote_pending_pulls(&mut app);
        assert_eq!(app.input_mode, InputMode::Mappings(Mode::PullConfirm));
        assert_eq!(pending_pull_refs(&app), ["sync/map.onCreate"]);
    }
}
