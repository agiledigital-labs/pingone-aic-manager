//! Small y/n confirmation modals — overwrite-existing-tenant and the
//! prod-write guard. Both share the same chrome as every other modal.

use ratatui::{
    Frame,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::App;
use crate::ui::modal_chrome::Modal;

pub fn draw_overwrite_confirm(f: &mut Frame, app: &App) {
    let name = app.pending_overwrite_name().unwrap_or("?");
    let body = Modal {
        title: "⚠ Tenant already exists",
        status: None,
        hints: &[("y", "overwrite"), ("n/Esc", "cancel")],
        body_height: 2,
    }
    .draw(f, f.area());

    let text = vec![
        Line::from(Span::styled(
            format!("A tenant with the name \"{name}\" already exists."),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "Do you want to overwrite it?",
            Style::default().fg(Color::White),
        )),
    ];
    f.render_widget(Paragraph::new(text), body);
}

pub fn draw_prod_confirm(f: &mut Frame, _app: &App) {
    let body = Modal {
        title: "⚠ PRODUCTION WRITE",
        status: None,
        hints: &[("y", "confirm"), ("n/Esc", "cancel")],
        body_height: 3,
    }
    .draw(f, f.area());

    let text = vec![
        Line::from(Span::styled(
            "You are about to write to PRODUCTION.",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Are you sure?",
            Style::default().fg(Color::White),
        )),
    ];
    f.render_widget(Paragraph::new(text), body);
}
