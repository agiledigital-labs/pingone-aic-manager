//! Placeholder rendering for the IDM record store tab.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::App;

pub fn draw_body(f: &mut Frame, _app: &App, area: Rect) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  IDM query store is not yet implemented.",
            Style::default().fg(Color::DarkGray),
        ))),
        area,
    );
}
