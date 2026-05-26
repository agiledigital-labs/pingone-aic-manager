//! Shared chrome for every full-screen modal in the TUI.
//!
//! The four screens that used to roll their own layout (Auth Settings,
//! Auth Setup, onboarding menu, onboarding forms) now go through this so
//! the title row, status row, and bottom hint row all line up — both
//! visually and in code.
//!
//! ```text
//!  Title (cyan bold)
//!
//!  Status (white, optional)
//!
//!  <body returned to the caller>
//!
//!  k1 desc1  k2 desc2  …       (cyan key + grey desc)
//! ```
//!
//! The takeover covers the entire frame; content sits in an 80-column
//! gutter centered horizontally. Body height adjusts to whatever is left
//! after the title / status / hint rows.
//!
//! Callers do the body themselves — this module owns title / status /
//! hints and the geometry around them.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

/// Fixed content width, in columns. Keeps every modal the same width on
/// wide terminals so they look like siblings instead of unrelated screens.
pub const CONTENT_WIDTH: u16 = 80;

pub struct Modal<'a> {
    pub title: &'a str,
    pub status: Option<&'a str>,
    pub hints: &'a [(&'a str, &'a str)],
    /// How many rows the body wants. The chrome adds title / gap / status /
    /// gap / hints around it and then centers the whole composition
    /// vertically on the screen. Callers should pass the exact height
    /// their body needs — the modal grows / shrinks accordingly.
    pub body_height: u16,
}

impl<'a> Modal<'a> {
    /// Render the title / status / hints around the modal area and return
    /// the body Rect the caller should fill. The total composition is
    /// centered both horizontally (80-col column) and vertically.
    pub fn draw(&self, f: &mut Frame, area: Rect) -> Rect {
        let column_width = CONTENT_WIDTH.min(area.width);
        let has_status = self.status.is_some();
        let chrome_height: u16 = 1                                                     // title
            + 1                                                                          // gap
            + (if has_status { 2 } else { 0 })                                           // status + gap
            + 2                                                                          // gap above hints
            + 1; // hints
        let desired_total = chrome_height.saturating_add(self.body_height.max(1));
        let total = desired_total.min(area.height);

        let column = Rect {
            x: area.x + (area.width.saturating_sub(column_width)) / 2,
            y: area.y + (area.height.saturating_sub(total)) / 2,
            width: column_width,
            height: total,
        };

        let chunks = Layout::vertical([
            Constraint::Length(1),                              // title
            Constraint::Length(1),                              // gap below title
            Constraint::Length(if has_status { 1 } else { 0 }), // status
            Constraint::Length(if has_status { 1 } else { 0 }), // gap below status
            Constraint::Min(1),                                 // body
            Constraint::Length(2),                              // gap above hints
            Constraint::Length(1),                              // hints
        ])
        .split(column);

        f.render_widget(
            Paragraph::new(Span::styled(
                self.title,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            chunks[0],
        );

        if let Some(s) = self.status {
            f.render_widget(
                Paragraph::new(Span::styled(s, Style::default().fg(Color::White))),
                chunks[2],
            );
        }

        f.render_widget(Paragraph::new(hint_line(self.hints)), chunks[6]);

        chunks[4]
    }
}

/// Same hint-rendering used by the global header strip, just consolidated
/// here so modals stay consistent.
pub fn hint_line(hints: &[(&str, &str)]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, (k, d)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            (*k).to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {d}"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    Line::from(spans)
}
