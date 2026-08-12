//! Browse-only access-rule table and raw selected-rule JSON detail.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, Wrap},
};

use crate::access::screen::Mode;
use crate::access::state::{LoadState, RuleMatch, RuleRow};
use crate::app::{App, InputMode};

pub fn draw_body(f: &mut Frame, app: &App, area: Rect) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };

    match app.access.data.get(&tenant) {
        None | Some(LoadState::Loading) => {
            crate::tui::list_chrome::draw_status_line(
                f,
                area,
                "  Loading access rules...",
                Color::DarkGray,
            );
            return;
        }
        Some(LoadState::Failed(error)) => {
            crate::tui::list_chrome::draw_status_line(
                f,
                area,
                &format!("  Access rules failed: {error}"),
                Color::Red,
            );
            return;
        }
        Some(LoadState::Loaded(_)) => {}
    }

    let document = app
        .access
        .document(&tenant)
        .expect("matched Loaded access document above");
    let matches = app.access.matches(Some(&tenant));
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(area);
    draw_document_digest(f, &document.digest, rows[0]);

    let columns = Layout::horizontal([
        Constraint::Percentage(62),
        Constraint::Length(2),
        Constraint::Percentage(38),
    ])
    .split(rows[2]);
    draw_table(f, app, document.rows.len(), &matches, columns[0]);
    draw_detail(f, app, &matches, columns[2]);
}

fn draw_document_digest(f: &mut Frame, digest: &str, area: Rect) {
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("document digest  ", Style::default().fg(Color::DarkGray)),
            Span::styled(digest.to_string(), Style::default().fg(Color::Cyan)),
        ])),
        area,
    );
}

fn draw_table(f: &mut Frame, app: &App, total: usize, matches: &[RuleMatch], area: Rect) {
    let searching = app.input_mode == InputMode::Access(Mode::Search);
    let count_text = if app.access.query.is_empty() {
        format!("{total} rules ")
    } else {
        format!("{}/{} rules ", matches.len(), total)
    };
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    crate::tui::list_chrome::draw_search_row(f, rows[0], &app.access.query, searching, &count_text);

    let selected = app.access.selected.min(matches.len().saturating_sub(1));
    let visible_height = rows[1].height.saturating_sub(1) as usize;
    let scroll = crate::tui::list_chrome::clamp_scroll(
        app.access.scroll,
        selected,
        visible_height,
        matches.len(),
    );
    let table_rows = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_height)
        .map(|(position, item)| rule_table_row(&item.row, position == selected));
    let header = Row::new(["#", "DIGEST", "PATTERN", "METHODS", "ROLES", "DUP"]).style(
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );
    let widths = [
        Constraint::Length(4),
        Constraint::Length(8),
        Constraint::Percentage(32),
        Constraint::Length(18),
        Constraint::Percentage(38),
        Constraint::Length(3),
    ];
    f.render_widget(
        Table::new(table_rows, widths)
            .header(header)
            .column_spacing(1),
        rows[1],
    );
}

fn rule_table_row(row: &RuleRow, selected: bool) -> Row<'static> {
    let style = if selected {
        Style::default()
            .fg(Color::White)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let duplicate = if row.duplicate { "dup" } else { "" };
    Row::new([
        Cell::from(row.index.to_string()),
        Cell::from(row.digest.clone()),
        Cell::from(row.pattern.clone()),
        Cell::from(row.methods.clone()),
        Cell::from(row.roles.clone()),
        Cell::from(duplicate),
    ])
    .style(style)
}

fn draw_detail(f: &mut Frame, app: &App, matches: &[RuleMatch], area: Rect) {
    let selected = app.access.selected.min(matches.len().saturating_sub(1));
    let Some(rule) = matches.get(selected).map(|item| &item.row) else {
        crate::tui::list_chrome::draw_status_line(f, area, "no matching rule", Color::DarkGray);
        return;
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("rule #{}  ", rule.index),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(rule.digest.clone(), Style::default().fg(Color::Cyan)),
            if rule.duplicate {
                Span::styled("  duplicate", Style::default().fg(Color::Yellow))
            } else {
                Span::raw("")
            },
        ]),
        Line::from(""),
    ];
    match serde_json::to_string_pretty(&rule.raw) {
        Ok(json) => lines.extend(json.lines().map(|line| Line::from(line.to_string()))),
        Err(error) => lines.push(Line::from(Span::styled(
            format!("Could not render rule JSON: {error}"),
            Style::default().fg(Color::Red),
        ))),
    }
    f.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: false }),
        area,
    );
}
