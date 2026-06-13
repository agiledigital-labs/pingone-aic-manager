//! Shared 3-row labelled-secret field. Used for the master password, the PIN
//! on the security-key enrol form, and the unlock screen's PIN+password
//! inputs.
//!
//! Row 0  label  (caller-supplied)
//! Row 1  masked value on a dark-backed strip
//! Row 2  optional status row (e.g. "🔑 Tap your security key to unlock…",
//!        or "Unlocking…")
//!
//! Centralising this means any visual tweak — colour, glyph, layout — happens
//! in one place instead of every form that needs a masked input.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

/// Recommended row count for layouts that allocate space for this widget.
/// Callers can also pass a 2-row area to suppress the status row entirely.
pub const HEIGHT: u16 = 3;

const BG_UNFOCUSED: Color = Color::Indexed(234);
const BG_FOCUSED: Color = Color::Indexed(236);

/// Draw a labelled, masked secret field. `area` must be at least 2 rows; the
/// status row is rendered only when `status.is_some()` *and* `area.height >= 3`.
pub fn draw(
    f: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    focused: bool,
    status: Option<&str>,
) {
    if area.height == 0 {
        return;
    }

    let label_area = Rect { height: 1, ..area };
    f.render_widget(
        Paragraph::new(Span::styled(
            label.to_string(),
            if focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            },
        )),
        label_area,
    );

    if area.height < 2 {
        return;
    }

    let value_area = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: 1,
    };
    let bg = if focused { BG_FOCUSED } else { BG_UNFOCUSED };
    let masked: String = "•".repeat(value.chars().count());
    let cursor = if focused { "▏" } else { " " };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default().bg(bg)),
            Span::styled(masked, Style::default().fg(Color::Yellow).bg(bg)),
            Span::styled(cursor, Style::default().fg(Color::Yellow).bg(bg)),
        ]))
        .style(Style::default().bg(bg)),
        value_area,
    );

    if area.height < 3 {
        return;
    }

    let status_area = Rect {
        x: area.x,
        y: area.y + 2,
        width: area.width,
        height: 1,
    };
    if let Some(msg) = status {
        f.render_widget(
            Paragraph::new(Span::styled(
                msg,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            status_area,
        );
    }
}
