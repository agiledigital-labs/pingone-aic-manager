//! Small y/n overlay confirm. Unlike `modal_chrome` (which takes over the
//! whole screen), this draws a narrow centered box with a cyan border on
//! top of whatever's behind it. Used for short questions where a full
//! takeover would be more disruptive than the question warrants — e.g.
//! "trigger a tenant restart?".

use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap},
};

use crate::tui::modal_chrome::{centered, hint_line};

const WIDTH: u16 = 72;
const HEIGHT: u16 = 11;

pub fn draw(f: &mut Frame, title: &str, message: &str) {
    let area = centered(f.area(), WIDTH, HEIGHT);
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .padding(Padding::new(2, 2, 1, 1))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Layout: message grows, then a blank row separating it from the
    // hint row at the bottom. Mirrors the bottom-of-modal gap on the
    // full-screen chrome.
    let chunks = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);

    f.render_widget(
        Paragraph::new(message.to_string())
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new(hint_line(&[("y", "yes"), ("n/Esc", "cancel")])),
        chunks[2],
    );
}
