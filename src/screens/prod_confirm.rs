//! Shared production-write confirmation. Any screen that wants to write to a
//! production tenant stores a pending action here, switches to
//! `InputMode::ProdConfirm`, and lets this handler dispatch the confirmed
//! action.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, InputMode};
use crate::config::tenant::Tenant;
use crate::event::ToastKind;

#[derive(Debug)]
pub enum PendingProdAction {
    SaveTenant {
        tenant: Tenant,
        jwk: serde_json::Value,
    },
    EsvSave(crate::screens::esv::SavePlan),
    EsvRestart {
        tenant_name: String,
    },
}

#[derive(Debug, Default)]
pub struct State {
    pub pending: Option<PendingProdAction>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }
}

pub async fn handle_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let action = app.prod_confirm.pending.take();
            app.input_mode = InputMode::Normal;
            if let Some(action) = action {
                match action {
                    PendingProdAction::SaveTenant { tenant, jwk } => {
                        match crate::screens::onboard::persist_new_tenant(app, tenant, jwk) {
                            Ok(()) => app.push_toast(ToastKind::Success, "Tenant added!"),
                            Err(e) => {
                                app.push_toast(ToastKind::Error, format!("Save failed: {e}"))
                            }
                        }
                    }
                    PendingProdAction::EsvSave(plan) => {
                        crate::screens::esv::execute_save_plan(app, plan, true);
                    }
                    PendingProdAction::EsvRestart { tenant_name } => {
                        crate::screens::esv::trigger_restart_confirmed(app, tenant_name, true);
                    }
                }
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            let action = app.prod_confirm.pending.take();
            app.input_mode = match action {
                Some(PendingProdAction::EsvSave(_)) if app.esv.editing.is_some() => {
                    InputMode::EsvEdit
                }
                _ => InputMode::Normal,
            };
            app.push_toast(ToastKind::Info, "Prod write cancelled");
        }
        _ => {}
    }
    Ok(())
}
