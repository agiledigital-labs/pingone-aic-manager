pub mod auth_settings;
pub mod auth_setup;
pub mod env_picker;
pub mod header;
pub mod modal;
pub mod onboard;
pub mod toast;
pub mod unlock;
pub mod widgets;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::{App, InputMode};

pub fn draw(f: &mut Frame, app: &App) {
    // Full-screen takeovers come first.
    match app.input_mode {
        InputMode::Unlock => {
            unlock::draw(f, app);
            return;
        }
        InputMode::SetupAuth => {
            auth_setup::draw(f, app);
            return;
        }
        _ => {}
    }

    let area = f.area();
    let chunks = Layout::vertical([
        Constraint::Length(1), // top: tabs + chips
        Constraint::Min(0),    // body
        Constraint::Length(1), // bottom: global keybind hints
    ])
    .split(area);

    header::draw(f, app, chunks[0]);
    draw_body(f, app, chunks[1]);
    header::draw_hints(f, app, chunks[2]);

    // Overlay modals
    match app.input_mode {
        InputMode::OnboardMenu
        | InputMode::OnboardCookie
        | InputMode::OnboardUserPass
        | InputMode::OnboardPaste => {
            onboard::draw(f, app);
        }
        InputMode::OverwriteConfirm => {
            modal::draw_overwrite_confirm(f, app);
        }
        InputMode::EnvPicker => {
            env_picker::draw(f, app);
        }
        InputMode::ProdConfirm => {
            modal::draw_prod_confirm(f, app);
        }
        InputMode::AuthSettings => {
            auth_settings::draw(f, app);
        }
        InputMode::AuthSettingsConfirm => {
            auth_settings::draw(f, app);
            auth_settings::draw_confirm(f, app);
        }
        InputMode::AuthSettingsRename => {
            auth_settings::draw(f, app);
            auth_settings::draw_rename(f, app);
        }
        _ => {}
    }

    toast::draw(f, app);
}

fn draw_body(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if app.tenants.is_empty() {
        let chunks = Layout::vertical([
            Constraint::Percentage(40),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

        let mut lines = vec![
            Line::from(Span::styled(
                "Welcome to aic-edit",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "No tenants configured. Press Ctrl-N to add your first tenant.",
                Style::default().fg(Color::Gray),
            )),
        ];
        lines.push(Line::from(Span::styled(
            "Press Ctrl-A to manage authentication factors.",
            Style::default().fg(Color::Gray),
        )));
        f.render_widget(
            Paragraph::new(lines).alignment(Alignment::Center),
            chunks[1],
        );
    } else {
        // ESVs tab placeholder (Step 3)
        f.render_widget(
            Paragraph::new(Span::styled(
                "  ESVs — coming in Step 3",
                Style::default().fg(Color::DarkGray),
            )),
            area,
        );
    }
}
