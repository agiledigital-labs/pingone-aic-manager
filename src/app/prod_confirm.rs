//! Shared production-write confirmation. Any screen that wants to write to a
//! production tenant stores a pending action here, switches to
//! `InputMode::ProdConfirm`, and lets this handler dispatch the confirmed
//! action.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::event::ToastKind;
use crate::app::{App, InputMode};
#[derive(Debug)]
pub enum PendingProdAction {
    Esv(crate::esv::screen::ProdAction),
    Secrets(crate::secrets::screen::ProdAction),
    Managed(crate::managed::ops::ProdAction),
    Secretmap(crate::secretmap::ops::ProdAction),
    Scripts(crate::scripts::screen::ProdAction),
    Mappings(crate::mappings::ops::ProdAction),
    Access(crate::access::ops::ProdAction),
    Onboard(crate::onboard::screen::ProdAction),
    Offboard(crate::offboard::screen::ProdAction),
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
                    PendingProdAction::Onboard(action) => {
                        crate::onboard::screen::execute_prod_action(app, action)
                    }
                    PendingProdAction::Esv(action) => {
                        crate::esv::screen::execute_prod_action(app, action)
                    }
                    PendingProdAction::Secrets(action) => {
                        crate::secrets::screen::execute_prod_action(app, action)
                    }
                    PendingProdAction::Managed(action) => {
                        crate::managed::ops::execute_prod_action(app, action)
                    }
                    PendingProdAction::Secretmap(action) => {
                        crate::secretmap::ops::execute_prod_action(app, action)
                    }
                    PendingProdAction::Scripts(action) => {
                        crate::scripts::screen::execute_prod_action(app, action)
                    }
                    PendingProdAction::Mappings(action) => {
                        crate::mappings::ops::execute_prod_action(app, action)
                    }
                    PendingProdAction::Access(action) => {
                        crate::access::ops::execute_prod_action(app, action)
                    }
                    PendingProdAction::Offboard(action) => {
                        crate::offboard::screen::execute_prod_action(app, action)
                    }
                }
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            let action = app.prod_confirm.pending.take();
            app.input_mode = match action {
                Some(PendingProdAction::Esv(action)) => {
                    crate::esv::screen::resume_mode(app, &action)
                }
                Some(PendingProdAction::Managed(action)) => {
                    crate::managed::ops::resume_mode(app, &action)
                }
                Some(PendingProdAction::Secrets(action)) => {
                    crate::secrets::screen::resume_mode(app, &action)
                }
                Some(PendingProdAction::Secretmap(action)) => {
                    crate::secretmap::ops::resume_mode(app, &action)
                }
                Some(PendingProdAction::Scripts(action)) => {
                    crate::scripts::screen::resume_mode(app, &action)
                }
                Some(PendingProdAction::Mappings(action)) => {
                    crate::mappings::ops::resume_mode(app, &action)
                }
                Some(PendingProdAction::Access(action)) => {
                    crate::access::ops::resume_mode(app, &action)
                }
                Some(PendingProdAction::Onboard(action)) => {
                    crate::onboard::screen::resume_mode(app, &action)
                }
                Some(PendingProdAction::Offboard(action)) => {
                    crate::offboard::screen::resume_mode(app, &action)
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
        .and_then(|action| pending_description(app, action));
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

fn pending_description(app: &App, action: &PendingProdAction) -> Option<String> {
    match action {
        PendingProdAction::Scripts(action) => crate::scripts::screen::describe_prod_action(action),
        PendingProdAction::Mappings(action) => crate::mappings::ops::describe_prod_action(action),
        PendingProdAction::Access(action) => crate::access::ops::describe_prod_action(action),
        PendingProdAction::Offboard(action) => {
            crate::offboard::screen::describe_prod_action(app, action)
        }
        _ => None,
    }
}
