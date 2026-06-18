//! Scripts tab rendering: a 40/60 list + preview split, mirroring the ESV
//! tab. The left list shows every script across all namespaces (AM, endpoints,
//! schedules, managed hooks, sync mappings) with a local sync marker (`!` =
//! local edits, `-` = not pulled); the right pane previews the selected
//! script's source plus a one-line status.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::{App, InputMode};
use crate::scripts::screen::{LoadState, Match, Mode};
use crate::scripts::sync::LocalState;

pub fn draw_body(f: &mut Frame, app: &App, area: Rect) {
    let tenant_name = match app.active_tenant() {
        Some(t) => t.name.clone(),
        None => return,
    };

    match app.scripts.data.get(&tenant_name) {
        None | Some(LoadState::Loading) => {
            status_line(f, area, "  Loading scripts…", Color::DarkGray);
            return;
        }
        Some(LoadState::Failed(e)) => {
            status_line(f, area, &format!("  Script list failed: {e}"), Color::Red);
            return;
        }
        Some(LoadState::Loaded(items)) if items.is_empty() => {
            status_line(f, area, "  No scripts found.", Color::DarkGray);
            return;
        }
        Some(LoadState::Loaded(_)) => {}
    }

    let matches = app.scripts.matches(Some(&tenant_name));
    let columns =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);
    draw_list(f, app, &tenant_name, &matches, columns[0]);
    draw_preview(f, app, &tenant_name, &matches, columns[1]);
}

fn status_line(f: &mut Frame, area: Rect, text: &str, color: Color) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            text.to_string(),
            Style::default().fg(color),
        ))),
        area,
    );
}

fn draw_list(f: &mut Frame, app: &App, tenant: &str, matches: &[Match], area: Rect) {
    let searching = app.input_mode == InputMode::Scripts(Mode::Search);
    let total = match app.scripts.data.get(tenant) {
        Some(LoadState::Loaded(items)) => items.len(),
        _ => 0,
    };
    let count_text = if app.scripts.query.is_empty() {
        format!("{total} scripts ")
    } else {
        format!("{}/{} scripts ", matches.len(), total)
    };

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    crate::tui::list_chrome::draw_search_row(
        f,
        rows[0],
        &app.scripts.query,
        searching,
        &count_text,
    );

    let h = rows[1].height as usize;
    let n = matches.len();
    let selected = app.scripts.selected.min(n.saturating_sub(1));
    let scroll = crate::tui::list_chrome::clamp_scroll(app.scripts.scroll, selected, h, n);

    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(h)
        .map(|(i, m)| render_row(m, i == selected))
        .collect();
    f.render_widget(Paragraph::new(lines), rows[1]);
}

/// One list row. Gutter marker mirrors the CLI picker: `!` (yellow) for local
/// edits, `-` (dim) for not-yet-pulled, blank for in-sync. Default
/// (product-shipped) scripts render dimmer.
fn render_row(m: &Match, is_selected: bool) -> Line<'static> {
    let modified = m.local == LocalState::Modified;
    let missing = m.local == LocalState::Missing;

    let base_fg = Color::Gray;
    let row_style = if is_selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(base_fg)
    };
    let match_style = if is_selected {
        row_style.add_modifier(Modifier::UNDERLINED)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    };

    let (marker, marker_style) = match (is_selected, modified, missing) {
        (true, true, _) => ("▶!", row_style),
        (true, _, _) => ("▶ ", row_style),
        (false, true, _) => (
            "! ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        (false, _, true) => ("- ", Style::default().fg(Color::DarkGray)),
        (false, _, _) => ("  ", row_style),
    };
    let mut spans = vec![Span::styled(marker, marker_style)];

    if m.positions.is_empty() {
        spans.push(Span::styled(m.full.clone(), row_style));
    } else {
        let mut pos_iter = m.positions.iter().copied().peekable();
        for (i, c) in m.full.chars().enumerate() {
            let is_match = pos_iter.peek().copied().is_some_and(|p| p as usize == i);
            if is_match {
                pos_iter.next();
                spans.push(Span::styled(c.to_string(), match_style));
            } else {
                spans.push(Span::styled(c.to_string(), row_style));
            }
        }
    }
    Line::from(spans)
}

fn draw_preview(f: &mut Frame, app: &App, tenant: &str, matches: &[Match], area: Rect) {
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let inner = Rect {
        x: inner.x + 2,
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height,
    };

    let selected = app.scripts.selected.min(matches.len().saturating_sub(1));
    let Some(m) = matches.get(selected) else {
        status_line(f, inner, "no match", Color::DarkGray);
        return;
    };
    let Some(LoadState::Loaded(items)) = app.scripts.data.get(tenant) else {
        return;
    };
    let Some(candidate) = items.get(m.idx) else {
        return;
    };

    // Header: full-name (cyan) + status line.
    let (status_text, status_color) = match candidate.local {
        LocalState::Clean => ("in sync".to_string(), Color::Green),
        LocalState::Modified => (
            "modified locally (!) — press P to push".to_string(),
            Color::Yellow,
        ),
        LocalState::Missing => (
            "not pulled (-) — press p to pull".to_string(),
            Color::DarkGray,
        ),
    };
    let default_note = if candidate.is_default {
        "  ·  default (product-shipped)"
    } else {
        ""
    };

    let rows = Layout::vertical([
        Constraint::Length(1), // full-name
        Constraint::Length(1), // status
        Constraint::Length(1), // gap
        Constraint::Min(0),    // source
    ])
    .split(inner);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            m.full.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))),
        rows[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(status_text, Style::default().fg(status_color)),
            Span::styled(default_note, Style::default().fg(Color::DarkGray)),
        ])),
        rows[1],
    );

    let source = crate::scripts::sync::preview_source(tenant, candidate);
    let body = match source {
        Some(src) => {
            let max = rows[3].height as usize;
            let lines: Vec<Line> = src
                .lines()
                .take(max)
                .map(|l| {
                    Line::from(Span::styled(
                        l.to_string(),
                        Style::default().fg(Color::Gray),
                    ))
                })
                .collect();
            Paragraph::new(lines)
        }
        None => Paragraph::new(Line::from(Span::styled(
            "(not pulled — press p to pull it into the workspace)",
            Style::default().fg(Color::DarkGray),
        ))),
    };
    f.render_widget(body, rows[3]);
}
