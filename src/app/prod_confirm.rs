//! Shared production-write confirmation. Any screen that wants to write to a
//! production tenant stores a pending action here, switches to
//! `InputMode::ProdConfirm`, and lets this handler dispatch the confirmed
//! action.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::event::ToastKind;
use crate::app::{App, InputMode};
use crate::config::LogKeyPair;
use crate::config::tenant::Tenant;

#[derive(Debug)]
pub enum PendingProdAction {
    SaveTenant {
        tenant: Tenant,
        jwk: Option<serde_json::Value>,
        log_key: Option<LogKeyPair>,
    },
    EsvSave(crate::esv::state::SavePlan),
    EsvDelete(crate::esv::state::DeletePlan),
    EsvUndo(crate::undo::UndoId),
    EsvRestart {
        tenant_name: String,
    },
    SecretsCreate(crate::secrets::state::CreatePlan),
    SecretsAddVersion(crate::secrets::state::VersionAddPlan),
    SecretDelete(crate::secrets::state::DeletePlan),
    SecretSetDescription(crate::secrets::state::SetDescriptionPlan),
    SecretVersionStatus {
        tenant: String,
        id: String,
        version: String,
        status: String,
    },
    SecretVersionDestroy {
        tenant: String,
        id: String,
        version: String,
    },
    ManagedUpdate(crate::managed::ops::ObjectReplacePlan),
    ManagedUndo(crate::undo::UndoId),
    SecretMappingReplace(crate::secretmap::ops::AliasReplacePlan),
    SecretMappingDelete(crate::secretmap::ops::MappingDeletePlan),
    ScriptPush {
        tenant: String,
        kind: crate::scripts::Kind,
        realm: String,
        name: String,
        full: String,
    },
    MappingRecon {
        tenant: String,
        mapping: String,
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
                    PendingProdAction::SaveTenant {
                        tenant,
                        jwk,
                        log_key,
                    } => {
                        match crate::onboard::screen::persist_tenant_overwriting(
                            app, tenant, jwk, log_key,
                        ) {
                            Ok(()) => app.push_toast(ToastKind::Success, "Tenant saved"),
                            Err(e) => app.push_toast(ToastKind::Error, format!("Save failed: {e}")),
                        }
                    }
                    PendingProdAction::EsvSave(plan) => {
                        crate::esv::ops::execute_save_plan(app, plan, true);
                    }
                    PendingProdAction::EsvDelete(plan) => {
                        crate::esv::ops::execute_delete_plan(app, plan, true);
                    }
                    PendingProdAction::EsvUndo(undo_id) => {
                        crate::esv::ops::execute_undo(app, undo_id, true);
                    }
                    PendingProdAction::EsvRestart { tenant_name } => {
                        crate::esv::ops::trigger_restart_confirmed(app, tenant_name, true);
                    }
                    PendingProdAction::SecretsCreate(plan) => {
                        crate::secrets::ops::execute_create(app, plan, true);
                    }
                    PendingProdAction::SecretsAddVersion(plan) => {
                        crate::secrets::ops::execute_add_version(app, plan, true);
                    }
                    PendingProdAction::SecretDelete(plan) => {
                        crate::secrets::ops::execute_delete(app, plan, true);
                    }
                    PendingProdAction::SecretSetDescription(plan) => {
                        crate::secrets::ops::execute_set_description(app, plan, true);
                    }
                    PendingProdAction::SecretVersionStatus {
                        tenant,
                        id,
                        version,
                        status,
                    } => {
                        crate::secrets::ops::execute_version_status(
                            app, tenant, id, version, status, true,
                        );
                    }
                    PendingProdAction::SecretVersionDestroy {
                        tenant,
                        id,
                        version,
                    } => {
                        crate::secrets::ops::execute_version_destroy(
                            app, tenant, id, version, true,
                        );
                    }
                    PendingProdAction::ManagedUpdate(plan) => {
                        crate::managed::ops::execute_update_plan(app, plan, true);
                    }
                    PendingProdAction::ManagedUndo(undo_id) => {
                        crate::managed::ops::execute_undo(app, undo_id, true);
                    }
                    PendingProdAction::SecretMappingReplace(plan) => {
                        crate::secretmap::ops::execute_write_plan(app, plan, true);
                    }
                    PendingProdAction::SecretMappingDelete(plan) => {
                        crate::secretmap::ops::execute_remove_plan(app, plan, true);
                    }
                    PendingProdAction::ScriptPush {
                        tenant,
                        kind,
                        realm,
                        name,
                        full,
                    } => {
                        crate::scripts::screen::execute_push(
                            app, tenant, kind, realm, name, full, true,
                        );
                    }
                    PendingProdAction::MappingRecon { tenant, mapping } => {
                        crate::mappings::ops::execute_recon(app, tenant, mapping, true);
                    }
                }
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            let action = app.prod_confirm.pending.take();
            app.input_mode = match action {
                Some(PendingProdAction::EsvSave(_)) if app.esv.editing.is_some() => {
                    InputMode::Esv(crate::esv::screen::Mode::Edit)
                }
                Some(PendingProdAction::ManagedUpdate(_)) => {
                    crate::managed::screen::resume_mode_after_prod_cancel(app)
                        .map(InputMode::Managed)
                        .unwrap_or(InputMode::Normal)
                }
                _ => InputMode::Normal,
            };
            app.push_toast(ToastKind::Info, "Prod write cancelled");
        }
        _ => {}
    }
    Ok(())
}

/// Render the production-write confirm modal (absorbed from the old
/// `ui::modal` when `screens/` + `ui/` dissolved into feature verticals).
pub fn draw(f: &mut ratatui::Frame, app: &App) {
    use ratatui::{
        style::{Color, Style},
        text::{Line, Span},
        widgets::Paragraph,
    };

    let description = app
        .prod_confirm
        .pending
        .as_ref()
        .and_then(pending_description);
    let body_height = if description.is_some() { 5 } else { 3 };
    let body = crate::tui::modal_chrome::Modal {
        title: "\u{26a0} PRODUCTION WRITE",
        status: None,
        hints: &[("y", "confirm"), ("n/Esc", "cancel")],
        body_height,
    }
    .draw(f, f.area());

    let mut text = vec![
        Line::from(Span::styled(
            "You are about to write to PRODUCTION.",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
    ];
    if let Some(description) = description {
        text.push(Line::from(Span::styled(
            description,
            Style::default().fg(Color::Yellow),
        )));
        text.push(Line::from(""));
    }
    text.push(Line::from(Span::styled(
        "Are you sure?",
        Style::default().fg(Color::White),
    )));
    f.render_widget(Paragraph::new(text), body);
}

fn pending_description(action: &PendingProdAction) -> Option<String> {
    match action {
        PendingProdAction::ScriptPush { full, .. } => Some(format!("push script {full}")),
        PendingProdAction::MappingRecon { mapping, .. } => Some(format!(
            "run reconciliation on {mapping} - creates/updates/deletes target objects"
        )),
        _ => None,
    }
}
