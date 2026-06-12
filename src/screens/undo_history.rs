//! Undo history modal.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, InputMode};
use crate::config::tenant::TenantTheme;
use crate::event::ToastKind;
use crate::screens::prod_confirm::PendingProdAction;
use crate::undo::{EntryStatus, UndoSummary};

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
        KeyCode::Char('j') | KeyCode::Down => {
            if app.undo_history_idx + 1 < n {
                app.undo_history_idx += 1;
            }
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
            if is_prod {
                app.prod_confirm.pending = Some(PendingProdAction::EsvUndo(summary.id));
                app.input_mode = InputMode::ProdConfirm;
            } else {
                crate::esv::ops::execute_undo(app, summary.id, false);
            }
        }
        _ => {}
    }
}
