//! Undo history modal.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::event::ToastKind;
use crate::app::prod_confirm::PendingProdAction;
use crate::app::{App, InputMode};
use crate::config::tenant::TenantTheme;
use crate::undo::{EntryStatus, UndoExecutor, UndoSummary};

pub fn request_latest(app: &mut App, executor: UndoExecutor) {
    match executor {
        UndoExecutor::Esv => crate::esv::ops::request_latest_undo(app),
        UndoExecutor::Managed => crate::managed::ops::request_latest_undo(app),
        UndoExecutor::SecretMapping => crate::secretmap::ops::request_latest_undo(app),
        UndoExecutor::Access => crate::access::ops::request_latest_undo(app),
    }
}

fn execute(app: &mut App, executor: UndoExecutor, undo_id: crate::undo::UndoId) {
    match executor {
        UndoExecutor::Esv => crate::esv::ops::execute_undo(app, undo_id, false),
        UndoExecutor::Managed => crate::managed::ops::execute_undo(app, undo_id, false),
        UndoExecutor::SecretMapping => crate::secretmap::ops::execute_undo(app, undo_id, false),
        UndoExecutor::Access => crate::access::ops::execute_undo(app, undo_id, false),
    }
}

fn pending_prod_action(executor: UndoExecutor, undo_id: crate::undo::UndoId) -> PendingProdAction {
    match executor {
        UndoExecutor::Esv => PendingProdAction::Esv(crate::esv::screen::ProdAction::Undo(undo_id)),
        UndoExecutor::Managed => {
            PendingProdAction::Managed(crate::managed::ops::ProdAction::Undo(undo_id))
        }
        UndoExecutor::SecretMapping => {
            PendingProdAction::Secretmap(crate::secretmap::ops::ProdAction::Undo(undo_id))
        }
        UndoExecutor::Access => {
            PendingProdAction::Access(crate::access::ops::ProdAction::Undo(undo_id))
        }
    }
}

pub fn summaries(app: &App) -> Vec<UndoSummary> {
    let Some(tenant) = app.active_tenant() else {
        return Vec::new();
    };
    app.undo
        .list(100)
        .into_iter()
        .filter(|summary| summary.tenant == tenant.name)
        .collect()
}

pub fn handle_key(app: &mut App, key: KeyEvent) {
    let n = summaries(app).len();
    match key.code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char('j') | KeyCode::Down if app.undo_history_idx + 1 < n => {
            app.undo_history_idx += 1;
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.undo_history_idx = app.undo_history_idx.saturating_sub(1);
        }
        KeyCode::Enter => {
            let summaries = summaries(app);
            let Some(summary) = summaries.get(app.undo_history_idx) else {
                return;
            };
            if summary.status != EntryStatus::Pending {
                app.push_toast(ToastKind::Info, "Undo entry is no longer pending");
                return;
            }
            let is_prod = app
                .active_tenant()
                .is_some_and(|tenant| tenant.theme == TenantTheme::Production);
            let executor = match app.undo.load(summary.id) {
                Ok(entry) => match entry.op {
                    Some(op) => op.executor(),
                    None => {
                        app.push_toast(ToastKind::Warning, "This change cannot be undone");
                        return;
                    }
                },
                Err(error) => {
                    app.push_toast(ToastKind::Error, format!("Undo failed: {error}"));
                    return;
                }
            };
            if executor == UndoExecutor::SecretMapping
                && !app
                    .active_tenant()
                    .is_some_and(|tenant| tenant.allows_secret_mappings())
            {
                app.push_toast(
                    ToastKind::Warning,
                    "Secret-mapping undo is only available on sandbox/development tenants",
                );
                return;
            }
            if is_prod {
                app.prod_confirm.pending = Some(pending_prod_action(executor, summary.id));
                app.input_mode = InputMode::ProdConfirm;
            } else {
                execute(app, executor, summary.id);
            }
        }
        _ => {}
    }
}
