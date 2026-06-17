//! Undo history modal.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::event::ToastKind;
use crate::app::prod_confirm::PendingProdAction;
use crate::app::{App, InputMode};
use crate::config::tenant::TenantTheme;
use crate::undo::{EntryStatus, UndoOp, UndoSummary};

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
            let op = app.undo.load(summary.id).ok().and_then(|entry| entry.op);
            let managed = op
                .as_ref()
                .is_some_and(|op| matches!(op, UndoOp::ManagedObjectReplace { .. }));
            let secretmap = op
                .as_ref()
                .is_some_and(|op| matches!(op, UndoOp::SecretMappingReplace { .. }));
            if secretmap
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
                app.prod_confirm.pending = Some(if managed {
                    PendingProdAction::ManagedUndo(summary.id)
                } else {
                    PendingProdAction::EsvUndo(summary.id)
                });
                app.input_mode = InputMode::ProdConfirm;
            } else if managed {
                crate::managed::ops::execute_undo(app, summary.id, false);
            } else if secretmap {
                crate::secretmap::ops::execute_undo(app, summary.id, false);
            } else {
                crate::esv::ops::execute_undo(app, summary.id, false);
            }
        }
        _ => {}
    }
}
