use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::App;

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

pub fn fixed_rect(width: u16, height: u16, r: Rect) -> Rect {
    let x = r.x + r.width.saturating_sub(width) / 2;
    let y = r.y + r.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: width.min(r.width),
        height: height.min(r.height),
    }
}

pub fn draw_overwrite_confirm(f: &mut Frame, app: &App) {
    let name = app.pending_overwrite_name().unwrap_or("?");
    let area = centered_rect(60, 25, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            "⚠ Tenant already exists",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  A tenant with the name \"{name}\" already exists."),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  Do you want to overwrite it?",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from("  (y) overwrite   (n/Esc) cancel"),
        Line::from(""),
    ];
    f.render_widget(Paragraph::new(text).block(block), area);
}

pub fn draw_prod_confirm(f: &mut Frame, _app: &App) {
    let area = centered_rect(50, 20, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(Span::styled("⚠ PRODUCTION WRITE", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  You are about to write to PRODUCTION.",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  Are you sure?  (y) confirm   (n/Esc) cancel"),
        Line::from(""),
    ];

    let para = Paragraph::new(text).block(block);
    f.render_widget(para, area);
}
