use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::app::App;

const BG_FOCUSED: Color = Color::Indexed(236);

pub fn draw(f: &mut Frame, app: &App) {
    let yubikey_enrolled = app.wraps.has_yubikey();
    let height = if yubikey_enrolled { 8 } else { 6 };
    let area = centered_form(f.area(), 50, height);

    let chunks = if yubikey_enrolled {
        Layout::vertical([
            Constraint::Length(1), // yubikey hint
            Constraint::Length(1), // gap
            Constraint::Length(1), // label
            Constraint::Length(1), // value
            Constraint::Length(1), // gap
            Constraint::Length(2), // error
            Constraint::Length(1), // hint
        ])
        .split(area)
    } else {
        Layout::vertical([
            Constraint::Length(0), // (no yubikey hint)
            Constraint::Length(0),
            Constraint::Length(1), // label
            Constraint::Length(1), // value
            Constraint::Length(1), // gap
            Constraint::Length(2), // error
            Constraint::Length(1), // hint
        ])
        .split(area)
    };

    if yubikey_enrolled {
        f.render_widget(
            Paragraph::new(Span::styled(
                "🔑  Tap your Yubikey, or type your password below",
                Style::default().fg(Color::Cyan),
            )),
            chunks[0],
        );
    }

    // Label.
    f.render_widget(
        Paragraph::new(Span::styled(
            "Master password",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        chunks[2],
    );

    // Value row: dark bg, "Unlocking…" while busy, otherwise masked input.
    let body = if app.unlock_busy {
        Line::from(vec![
            Span::styled(" ", Style::default().bg(BG_FOCUSED)),
            Span::styled(
                "Unlocking…",
                Style::default()
                    .fg(Color::Cyan)
                    .bg(BG_FOCUSED)
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        let masked: String = "•".repeat(app.unlock_input.chars().count());
        Line::from(vec![
            Span::styled(" ", Style::default().bg(BG_FOCUSED)),
            Span::styled(masked, Style::default().fg(Color::Yellow).bg(BG_FOCUSED)),
            Span::styled("▏", Style::default().fg(Color::Yellow).bg(BG_FOCUSED)),
        ])
    };
    f.render_widget(
        Paragraph::new(body).style(Style::default().bg(BG_FOCUSED)),
        chunks[3],
    );

    if let Some(err) = &app.unlock_error {
        f.render_widget(
            Paragraph::new(Span::styled(err.as_str(), Style::default().fg(Color::Red)))
                .wrap(Wrap { trim: false }),
            chunks[5],
        );
    }

    f.render_widget(
        Paragraph::new("Enter submit · Esc quit").style(Style::default().fg(Color::DarkGray)),
        chunks[6],
    );
}

/// Center a fixed-size form within the full terminal — no outer block, just
/// the form fields floating in the empty screen.
fn centered_form(parent: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(parent.width);
    let h = height.min(parent.height);
    Rect {
        x: parent.x + (parent.width.saturating_sub(w)) / 2,
        y: parent.y + (parent.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}
