pub mod auth_settings;
pub mod auth_setup;
pub mod env_picker;
pub mod header;
pub mod modal;
pub mod onboard;
pub mod toast;
pub mod unlock;
pub mod widgets;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::{esv_id, App, EsvLoadState, EsvMatch, InputMode};

pub fn draw(f: &mut Frame, app: &App) {
    // Full-screen takeovers come first.
    match app.input_mode {
        InputMode::Unlock => {
            unlock::draw(f, app);
            return;
        }
        InputMode::SetupAuth => {
            auth_setup::draw(f, app);
            return;
        }
        _ => {}
    }

    let area = f.area();
    let chunks = Layout::vertical([
        Constraint::Length(1), // top: tabs + chips
        Constraint::Length(1), // breathing room under the tab row
        Constraint::Min(0),    // body
        Constraint::Length(1), // bottom: global keybind hints
    ])
    .split(area);

    header::draw(f, app, chunks[0]);
    draw_body(f, app, chunks[2]);
    header::draw_hints(f, app, chunks[3]);

    // Overlay modals
    match app.input_mode {
        InputMode::OnboardMenu
        | InputMode::OnboardCookie
        | InputMode::OnboardUserPass
        | InputMode::OnboardPaste => {
            onboard::draw(f, app);
        }
        InputMode::OverwriteConfirm => {
            modal::draw_overwrite_confirm(f, app);
        }
        InputMode::EnvPicker => {
            env_picker::draw(f, app);
        }
        InputMode::ProdConfirm => {
            modal::draw_prod_confirm(f, app);
        }
        InputMode::AuthSettings => {
            auth_settings::draw(f, app);
        }
        InputMode::AuthSettingsConfirm => {
            auth_settings::draw(f, app);
            auth_settings::draw_confirm(f, app);
        }
        InputMode::AuthSettingsRename => {
            auth_settings::draw(f, app);
            auth_settings::draw_rename(f, app);
        }
        _ => {}
    }

    toast::draw(f, app);
}

fn draw_body(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if app.tenants.is_empty() {
        let chunks = Layout::vertical([
            Constraint::Percentage(40),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

        let mut lines = vec![
            Line::from(Span::styled(
                "Welcome to aic-edit",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "No tenants configured. Press Ctrl-N to add your first tenant.",
                Style::default().fg(Color::Gray),
            )),
        ];
        lines.push(Line::from(Span::styled(
            "Press Ctrl-A to manage authentication factors.",
            Style::default().fg(Color::Gray),
        )));
        f.render_widget(
            Paragraph::new(lines).alignment(Alignment::Center),
            chunks[1],
        );
    } else {
        draw_esvs(f, app, area);
    }
}

fn draw_esvs(f: &mut Frame, app: &App, area: Rect) {
    let tenant_name = match app.active_tenant() {
        Some(t) => t.name.as_str(),
        None => return,
    };

    // Loading / failed / empty: full-width status; no split, no preview pane.
    match app.esvs.get(tenant_name) {
        None | Some(EsvLoadState::Loading) => {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "  Loading ESVs…",
                    Style::default().fg(Color::DarkGray),
                ))),
                area,
            );
            return;
        }
        Some(EsvLoadState::Failed(e)) => {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("  ESV list failed: {e}"),
                    Style::default().fg(Color::Red),
                ))),
                area,
            );
            return;
        }
        Some(EsvLoadState::Loaded(vs)) if vs.is_empty() => {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "  No ESV variables.",
                    Style::default().fg(Color::DarkGray),
                ))),
                area,
            );
            return;
        }
        Some(EsvLoadState::Loaded(_)) => {}
    }

    let matches = app.esv_matches();
    let columns = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);
    draw_esv_list(f, app, &matches, columns[0]);
    draw_esv_preview(f, app, &matches, columns[1]);
}

fn draw_esv_list(f: &mut Frame, app: &App, matches: &[EsvMatch], area: Rect) {
    let searching = app.input_mode == InputMode::EsvSearch;
    let total = match app.active_tenant().and_then(|t| app.esvs.get(&t.name)) {
        Some(EsvLoadState::Loaded(vs)) => vs.len(),
        _ => 0,
    };
    let count_text = if app.esv_query.is_empty() {
        format!("{} ESVs ", total)
    } else {
        format!("{}/{} ESVs ", matches.len(), total)
    };

    let rows = Layout::vertical([
        Constraint::Length(1), // /query (left) + count (right)
        Constraint::Min(0),    // list
    ])
    .split(area);

    // Top row: split horizontally so the count hugs the right edge regardless
    // of the query length.
    let count_width = count_text.chars().count() as u16;
    let cols = Layout::horizontal([Constraint::Min(0), Constraint::Length(count_width)])
        .split(rows[0]);

    let query_style = Style::default().fg(if searching {
        Color::Yellow
    } else {
        Color::DarkGray
    });
    let cursor = if searching { "▏" } else { "" };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" /", query_style),
            Span::styled(app.esv_query.clone(), query_style),
            Span::styled(cursor, query_style.add_modifier(Modifier::SLOW_BLINK)),
        ])),
        cols[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            count_text,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Right),
        cols[1],
    );

    // Visible window: keep the selection inside [scroll, scroll + h).
    let h = rows[1].height as usize;
    let n = matches.len();
    let selected = app.esv_selected.min(n.saturating_sub(1));
    let scroll = clamp_scroll(app.esv_scroll, selected, h, n);

    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(h)
        .map(|(i, m)| render_esv_row(m, i == selected))
        .collect();
    f.render_widget(Paragraph::new(lines), rows[1]);
}

/// Pick the new top-of-window so `selected` stays visible. We can't compute
/// this purely from app state because the height comes from the rendered
/// rect; do it here, leave `app.esv_scroll` as a hint only.
fn clamp_scroll(prev: usize, selected: usize, height: usize, n: usize) -> usize {
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

fn render_esv_row(m: &EsvMatch, is_selected: bool) -> Line<'static> {
    let row_style = if is_selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let match_style = if is_selected {
        row_style.add_modifier(Modifier::UNDERLINED)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    };

    let mut spans = vec![Span::styled(
        if is_selected { "▶ " } else { "  " },
        row_style,
    )];

    if m.positions.is_empty() {
        spans.push(Span::styled(m.id.clone(), row_style));
    } else {
        // Highlight matched chars; positions are sorted utf32 indices.
        let mut pos_iter = m.positions.iter().copied().peekable();
        for (i, c) in m.id.chars().enumerate() {
            let is_match = pos_iter
                .peek()
                .copied()
                .is_some_and(|p| p as usize == i);
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

fn draw_esv_preview(f: &mut Frame, app: &App, matches: &[EsvMatch], area: Rect) {
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(tenant) = app.active_tenant() else { return };
    let Some(EsvLoadState::Loaded(items)) = app.esvs.get(&tenant.name) else { return };
    let selected = app.esv_selected.min(matches.len().saturating_sub(1));
    let Some(m) = matches.get(selected) else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  no match",
                Style::default().fg(Color::DarkGray),
            ))),
            inner,
        );
        return;
    };
    let Some(v) = items.get(m.idx) else { return };

    let rows = Layout::vertical([
        Constraint::Length(1), // id title
        Constraint::Length(1), // blank
        Constraint::Min(0),    // pretty JSON
    ])
    .split(inner);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                esv_id(v).to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ])),
        rows[0],
    );

    let pretty = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
    let body: Vec<Line> = pretty
        .lines()
        .map(|l| Line::from(Span::styled(
            format!("  {l}"),
            Style::default().fg(Color::Gray),
        )))
        .collect();
    f.render_widget(
        Paragraph::new(body).wrap(Wrap { trim: false }),
        rows[2],
    );
}
