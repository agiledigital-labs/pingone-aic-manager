//! Read-only IDM sync-mapping browser: searchable mapping list and static
//! mapping detail.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::{App, InputMode};
use crate::mappings::api::{self, ReconStatus};
use crate::mappings::screen::Mode;
use crate::mappings::state::{LoadState, MappingMatch};

pub fn draw_body(f: &mut Frame, app: &App, area: Rect) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };

    match app.mappings.data.get(&tenant) {
        None | Some(LoadState::Loading) => {
            status_line(f, area, "  Loading sync mappings...", Color::DarkGray);
            return;
        }
        Some(LoadState::Failed(error)) => {
            status_line(
                f,
                area,
                &format!("  Sync mappings failed: {error}"),
                Color::Red,
            );
            return;
        }
        Some(LoadState::Loaded(mappings)) if mappings.is_empty() => {
            status_line(f, area, "  No sync mappings found.", Color::DarkGray);
            return;
        }
        Some(LoadState::Loaded(_)) => {}
    }

    let matches = app.mappings.matches(Some(&tenant));
    let columns =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);
    draw_list(f, app, &tenant, &matches, columns[0]);
    draw_detail(f, app, &tenant, &matches, columns[1]);
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

fn draw_list(f: &mut Frame, app: &App, tenant: &str, matches: &[MappingMatch], area: Rect) {
    let searching = app.input_mode == InputMode::Mappings(Mode::Search);
    let total = match app.mappings.data.get(tenant) {
        Some(LoadState::Loaded(mappings)) => mappings.len(),
        _ => 0,
    };
    let count_text = if app.mappings.query.is_empty() {
        format!("{total} mappings ")
    } else {
        format!("{}/{} mappings ", matches.len(), total)
    };

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    crate::tui::list_chrome::draw_search_row(
        f,
        rows[0],
        &app.mappings.query,
        searching,
        &count_text,
    );

    let height = rows[1].height as usize;
    let selected = app.mappings.selected.min(matches.len().saturating_sub(1));
    let scroll =
        crate::tui::list_chrome::clamp_scroll(app.mappings.scroll, selected, height, matches.len());
    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(height)
        .map(|(idx, item)| {
            render_row(
                item,
                idx == selected,
                app.mappings.recon_for(tenant, &item.name),
            )
        })
        .collect();
    f.render_widget(Paragraph::new(lines), rows[1]);
}

fn render_row(item: &MappingMatch, selected: bool, recon: Option<&ReconStatus>) -> Line<'static> {
    let row_style = if selected {
        Style::default()
            .fg(Color::White)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let match_style = if selected {
        row_style.add_modifier(Modifier::UNDERLINED)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    };
    let suffix_style = if selected {
        row_style
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let mut spans = vec![Span::styled("  ", row_style)];

    if item.positions.is_empty() {
        spans.push(Span::styled(item.name.clone(), row_style));
    } else {
        let mut positions = item.positions.iter().copied().peekable();
        for (idx, ch) in item.name.chars().enumerate() {
            if positions
                .peek()
                .copied()
                .is_some_and(|position| position as usize == idx)
            {
                positions.next();
                spans.push(Span::styled(ch.to_string(), match_style));
            } else {
                spans.push(Span::styled(ch.to_string(), row_style));
            }
        }
    }

    spans.push(Span::styled(format!("  {}", item.display), suffix_style));
    spans.push(Span::styled(
        format!("  {}", script_count_label(item.inline_script_count)),
        suffix_style,
    ));
    if let Some(status) = recon {
        let (badge, style) = recon_badge(status);
        spans.push(Span::styled(format!("  {badge}"), style));
    }
    Line::from(spans)
}

fn draw_detail(f: &mut Frame, app: &App, tenant: &str, matches: &[MappingMatch], area: Rect) {
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

    let selected = app.mappings.selected.min(matches.len().saturating_sub(1));
    let Some(item) = matches.get(selected) else {
        status_line(f, inner, "no match", Color::DarkGray);
        return;
    };

    let mut lines = vec![
        Line::from(Span::styled(
            item.name.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("source  ", Style::default().fg(Color::DarkGray)),
            Span::styled(item.source.clone(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("target  ", Style::default().fg(Color::DarkGray)),
            Span::styled(item.target.clone(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("scripts ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                script_count_label(item.inline_script_count),
                Style::default().fg(Color::White),
            ),
        ]),
    ];
    if let Some(status) = app.mappings.recon_for(tenant, &item.name) {
        lines.push(Line::from(""));
        lines.extend(recon_detail_lines(status));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("r", Style::default().fg(Color::Cyan)),
        Span::styled(" reconcile   ·   ", Style::default().fg(Color::DarkGray)),
        Span::styled("p", Style::default().fg(Color::Cyan)),
        Span::styled(" pull scripts", Style::default().fg(Color::DarkGray)),
    ]));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn script_count_label(count: usize) -> String {
    let noun = if count == 1 { "script" } else { "scripts" };
    format!("{count} inline {noun}")
}

fn recon_badge(status: &ReconStatus) -> (String, Style) {
    if !api::state_is_terminal(&status.state) {
        return ("⟳ ACTIVE".into(), Style::default().fg(Color::Yellow));
    }
    match status.state.as_str() {
        "SUCCESS" => ("✓ SUCCESS".into(), Style::default().fg(Color::Green)),
        "FAILED" => ("✗ FAILED".into(), Style::default().fg(Color::Red)),
        state => (format!("✗ {state}"), Style::default().fg(Color::Red)),
    }
}

fn recon_detail_lines(status: &ReconStatus) -> Vec<Line<'static>> {
    let (badge, badge_style) = recon_badge(status);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("recon   ", Style::default().fg(Color::DarkGray)),
            Span::styled(badge, badge_style),
        ]),
        Line::from(vec![
            Span::styled("stage   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                status.stage_description.clone(),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("counts  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "created {}  updated {}  deleted {}",
                    status.created, status.updated, status.deleted
                ),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("source  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("processed {}", status.processed),
                Style::default().fg(Color::White),
            ),
        ]),
    ];
    if let Some(duration) = status.duration {
        lines.push(Line::from(vec![
            Span::styled("time    ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{duration} ms"), Style::default().fg(Color::White)),
        ]));
    }
    lines
}
