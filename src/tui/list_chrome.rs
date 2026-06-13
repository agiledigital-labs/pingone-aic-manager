//! Shared chrome for the searchable tenant-list views (ESV variables,
//! secrets, scripts): the `/query` + count header row and the scroll clamp.
//! Pure rendering/math — no feature knowledge.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

/// Shared `/query` + right-aligned count header for the tenant list views
/// (variables and secrets), so both halves of the ESVs tab render the search
/// row identically. `area` must be a 1-row rect.
pub fn draw_search_row(
    f: &mut Frame,
    area: Rect,
    query: &crate::tui::widgets::LineEditor,
    searching: bool,
    count_text: &str,
) {
    // Split horizontally so the count hugs the right edge regardless of the
    // query length.
    let count_width = count_text.chars().count() as u16;
    let cols =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(count_width)]).split(area);

    let query_style = Style::default().fg(if searching {
        Color::Yellow
    } else {
        Color::DarkGray
    });
    // Standard block cursor: reverse-video the char under the cursor (or a
    // single space at end-of-line). Inserting a separate cursor glyph like
    // "▏" displaces following columns in fonts that render box-drawing
    // characters double-wide.
    let cursor_style = query_style.add_modifier(Modifier::REVERSED);
    let mut spans: Vec<Span> = vec![Span::styled(" /", query_style)];
    let cursor_idx = query.cursor();
    let chars: Vec<char> = query.value().chars().collect();
    if searching {
        for (i, c) in chars.iter().enumerate() {
            let style = if i == cursor_idx {
                cursor_style
            } else {
                query_style
            };
            spans.push(Span::styled(c.to_string(), style));
        }
        if cursor_idx >= chars.len() {
            spans.push(Span::styled(" ", cursor_style));
        }
    } else {
        spans.push(Span::styled(query.value().to_string(), query_style));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), cols[0]);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            count_text.to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Right),
        cols[1],
    );
}

/// Pick the new top-of-window so `selected` stays visible. We can't compute
/// this purely from app state because the height comes from the rendered
/// rect; do it here, leave the list's `scroll` as a hint only.
pub fn clamp_scroll(prev: usize, selected: usize, height: usize, n: usize) -> usize {
    if n == 0 || height == 0 {
        return 0;
    }
    let mut scroll = prev.min(n.saturating_sub(1));
    if selected < scroll {
        scroll = selected;
    } else if selected >= scroll + height {
        scroll = selected + 1 - height;
    }
    scroll
}
