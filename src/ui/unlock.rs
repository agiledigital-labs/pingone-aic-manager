use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::App;
use crate::ui::modal::centered_rect;

pub fn draw(f: &mut Frame, app: &App) {
    let area = centered_rect(60, 35, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " aic-edit ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Length(2), // intro
        Constraint::Length(3), // password field
        Constraint::Length(2), // error / status
        Constraint::Min(0),    // spacer
        Constraint::Length(1), // hint
    ])
    .split(inner);

    let intro = if app.is_first_run() {
        "Set a master password. It encrypts your tenant service-account keys on disk."
    } else {
        "Enter your master password to unlock aic-edit."
    };
    f.render_widget(
        Paragraph::new(intro)
            .style(Style::default().fg(Color::Gray))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );

    // Single bordered "Master password" field. While the unlock is in flight,
    // we replace the masked value with "Unlocking…" so the user has visible
    // feedback that Enter was accepted — argon2 + AES decrypt take ~hundreds
    // of ms which is long enough to feel unresponsive otherwise.
    let title = Span::styled(
        " Master password ",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    let inner_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(title);
    let body = if app.unlock_busy {
        Line::from(Span::styled(
            " Unlocking… ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
    } else {
        let masked: String = "•".repeat(app.unlock_input.chars().count());
        Line::from(vec![
            Span::styled(
                masked,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::UNDERLINED),
            ),
            Span::styled("▏", Style::default().fg(Color::Yellow)),
        ])
    };
    f.render_widget(Paragraph::new(body).block(inner_block), chunks[1]);

    if let Some(err) = &app.unlock_error {
        f.render_widget(
            Paragraph::new(Span::styled(
                err.as_str(),
                Style::default().fg(Color::Red),
            ))
            .wrap(Wrap { trim: false }),
            chunks[2],
        );
    }

    f.render_widget(
        Paragraph::new("Enter submit · Esc quit").style(Style::default().fg(Color::DarkGray)),
        chunks[4],
    );
}
