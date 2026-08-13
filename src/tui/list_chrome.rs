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

/// Scroll state for a detail pane whose rendered height is known only during
/// drawing. The last rendered limit is retained so key actions cannot build up
/// an unreachable offset between frames.
#[derive(Debug, Default)]
pub struct DetailScroll {
    offset: usize,
    limit: std::cell::Cell<usize>,
}

impl DetailScroll {
    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn scroll(&mut self, delta: isize) {
        let current = self.offset.min(self.limit.get());
        let requested = if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs())
        } else {
            current.saturating_add(delta as usize)
        };
        self.offset = requested.min(self.limit.get());
    }

    /// Clamp for a pane that has already materialised its rows — one `Line` per
    /// rendered row, drawn **without** `Paragraph::wrap`. Passing `lines.len()`
    /// for a pane that does wrap under-counts, and a too-small limit is
    /// invisible: the pane just stops early and looks like it reached the end.
    /// Use [`Self::clamp_wrapping`] there instead.
    pub fn clamp(&self, rendered_height: usize, viewport_height: usize) -> usize {
        let limit = rendered_height.saturating_sub(viewport_height);
        self.limit.set(limit);
        self.offset.min(limit)
    }

    /// Clamp for a pane that lets the widget wrap, because it styles spans
    /// within a line and so cannot pre-wrap to `String`s. Takes the lines
    /// rather than a height so the caller cannot supply the wrong measurement.
    ///
    /// The height is an estimate: it wraps through [`wrap_lines`], which
    /// re-applies a line's leading indentation to continuation rows where
    /// ratatui's own wrapper does not. For an indented line it therefore
    /// over-counts, which lets the scroll run a little past the content into
    /// blank rows rather than stopping short of it. Ratatui 0.30 can answer this
    /// exactly via `Paragraph::line_count`, but only behind its
    /// `unstable-rendered-line-info` feature.
    pub fn clamp_wrapping(&self, lines: &[Line<'_>], width: u16, viewport_height: usize) -> usize {
        self.clamp(wrapped_height(lines, width), viewport_height)
    }

    pub fn reset(&mut self) {
        self.offset = 0;
        self.limit.set(0);
    }
}

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

/// Wrap text to a character width, preferring whitespace boundaries while
/// still breaking tokens that exceed the available width. Continuation rows
/// retain the source line's leading indentation.
pub fn wrap_lines(text: &str, width: u16) -> Vec<String> {
    let width = usize::from(width);
    if width == 0 {
        return Vec::new();
    }

    let indent = text
        .chars()
        .take_while(|character| character.is_whitespace())
        .collect::<String>();
    let continuation_indent = if indent.chars().count() < width {
        indent
    } else {
        String::new()
    };
    let mut wrapped = Vec::new();
    let mut remaining = text.to_string();

    while remaining.chars().count() > width {
        let hard_end = remaining
            .char_indices()
            .nth(width)
            .map_or(remaining.len(), |(index, _)| index);
        let chunk = &remaining[..hard_end];
        let boundary_is_whitespace = remaining[hard_end..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace);
        let word_break = (!boundary_is_whitespace)
            .then(|| {
                chunk
                    .char_indices()
                    .rev()
                    .find(|(index, character)| {
                        character.is_whitespace()
                            && chunk[..*index]
                                .chars()
                                .any(|character| !character.is_whitespace())
                    })
                    .map(|(index, character)| (index, character.len_utf8()))
            })
            .flatten();

        let (line, rest) = if boundary_is_whitespace {
            (chunk, &remaining[hard_end..])
        } else if let Some((index, whitespace_len)) = word_break {
            (&remaining[..index], &remaining[index + whitespace_len..])
        } else {
            (chunk, &remaining[hard_end..])
        };
        wrapped.push(line.to_string());
        remaining = format!(
            "{}{}",
            continuation_indent,
            rest.trim_start_matches(char::is_whitespace)
        );
    }
    wrapped.push(remaining);
    wrapped
}

/// Estimated rows a wrapping paragraph will occupy. Private because
/// [`DetailScroll::clamp_wrapping`] is the only sound way to consume it — the
/// number exists to be handed straight to the clamp, and a caller holding it
/// separately is a caller who can pair it with the wrong viewport. See that
/// method for where the estimate diverges from ratatui.
fn wrapped_height(lines: &[Line<'_>], width: u16) -> usize {
    lines
        .iter()
        .map(|line| {
            let text: String = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect();
            wrap_lines(&text, width).len().max(1)
        })
        .sum()
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
    fn wrapped_height_counts_rows_not_lines() {
        let lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::raw("Alias      "),
                Span::raw("a-long-alias-value that will not fit"),
            ]),
        ];

        // Returning `lines.len()` — the bug this exists to prevent — fails here.
        assert!(wrapped_height(&lines, 20) > lines.len());
        // Wide enough for everything: one row per line, the blank one included.
        assert_eq!(wrapped_height(&lines, 120), lines.len());
        // A zero-width rect mid-resize must not report zero rows, or the clamp
        // takes a limit of 0 and silently discards the operator's offset.
        assert_eq!(wrapped_height(&lines, 0), lines.len());

        // And the public path: wrapped content taller than the viewport leaves
        // rows to reach.
        let mut scroll = DetailScroll::default();
        scroll.clamp_wrapping(&lines, 20, 2);
        scroll.scroll(10);
        assert_eq!(scroll.offset(), wrapped_height(&lines, 20) - 2);

        // Measuring the unwrapped count against the same viewport pins the pane
        // at the top — the defect, expressed as the contrast.
        let mut naive = DetailScroll::default();
        naive.clamp(lines.len(), 2);
        naive.scroll(10);
        assert_eq!(naive.offset(), 0);
    }

    #[test]
    fn detail_scroll_reclamps_before_delta_and_reset_clears_both_halves() {
        let mut scroll = DetailScroll::default();
        assert_eq!(scroll.clamp(15, 10), 0);
        for _ in 0..5 {
            scroll.scroll(10);
        }
        assert_eq!(scroll.offset(), 5);

        // A shorter redraw leaves the stored offset stale until the next key
        // action, which must re-clamp before applying its delta.
        assert_eq!(scroll.clamp(8, 10), 0);
        scroll.scroll(-10);
        assert_eq!(scroll.offset(), 0);

        scroll.clamp(15, 10);
        scroll.scroll(10);
        scroll.reset();
        scroll.scroll(10);
        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn wrapping_prefers_whitespace_breaks_and_preserves_indent() {
        assert_eq!(wrap_lines("alpha beta gamma", 10), ["alpha beta", "gamma"]);
        assert_eq!(
            wrap_lines("  alpha beta gamma", 10),
            ["  alpha", "  beta", "  gamma"]
        );
        assert_eq!(
            wrap_lines("  abcdefghijkl", 7),
            ["  abcde", "  fghij", "  kl"]
        );
    }
}
