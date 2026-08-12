//! Shared chrome and layout math for searchable tenant-list views, including
//! ESVs, scripts, managed objects, Access, and OAuth. Besides the `/query` +
//! count row, this owns list/detail scroll clamps and metadata truncation.
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

/// Shared one-line status text for list-style feature bodies while they are
/// loading, empty, or failed.
pub fn draw_status_line(f: &mut Frame, area: Rect, text: &str, color: Color) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            text.to_string(),
            Style::default().fg(color),
        ))),
        area,
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

/// Clamp a detail-pane offset to the last fully reachable rendered row.
pub fn clamp_detail_scroll(scroll: usize, rendered_height: usize, viewport_height: usize) -> usize {
    scroll.min(rendered_height.saturating_sub(viewport_height))
}

/// Truncate text to a character budget and make the loss visible.
pub fn truncate_metadata(value: &str, max_width: usize) -> String {
    if value.chars().count() <= max_width {
        return value.to_string();
    }
    if max_width <= 1 {
        return "…".to_string();
    }
    format!("{}…", value.chars().take(max_width - 1).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_uses_an_ellipsis_at_every_clipped_width() {
        // Replacing the ellipsis with a silent character cut makes each
        // clipped case fail.
        assert_eq!(truncate_metadata("abcdef", 4), "abc…");
        assert_eq!(truncate_metadata("abcdef", 1), "…");
        assert_eq!(truncate_metadata("abcdef", 0), "…");
        assert_eq!(truncate_metadata("abc", 3), "abc");
    }

    #[test]
    fn detail_scroll_never_passes_the_rendered_bottom() {
        // Returning the requested offset directly makes both over-scroll
        // cases fail, including content shorter than its viewport.
        assert_eq!(clamp_detail_scroll(50, 30, 10), 20);
        assert_eq!(clamp_detail_scroll(50, 8, 10), 0);
    }
}
