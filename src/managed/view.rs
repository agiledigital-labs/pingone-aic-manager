//! Read-only managed-object browser: searchable object list and schema detail.

use std::collections::HashSet;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use serde_json::Value;

use crate::app::{App, InputMode};
use crate::managed::api::ObjectSummary;
use crate::managed::screen::Mode;
use crate::managed::state::{LoadState, ManagedMatch};

pub fn draw_body(f: &mut Frame, app: &App, area: Rect) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };

    let doc = match app.managed.data.get(&tenant) {
        None | Some(LoadState::Loading) => {
            status_line(f, area, "  Loading managed objects…", Color::DarkGray);
            return;
        }
        Some(LoadState::Failed(error)) => {
            status_line(
                f,
                area,
                &format!("  Managed objects failed: {error}"),
                Color::Red,
            );
            return;
        }
        Some(LoadState::Loaded(doc)) => doc,
    };
    let summaries = match crate::managed::api::summarize(doc) {
        Ok(summaries) if summaries.is_empty() => {
            status_line(f, area, "  No managed objects found.", Color::DarkGray);
            return;
        }
        Ok(summaries) => summaries,
        Err(error) => {
            status_line(
                f,
                area,
                &format!("  Managed schema failed: {error}"),
                Color::Red,
            );
            return;
        }
    };

    let matches = app.managed.matches(Some(&tenant));
    let columns =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);
    draw_list(f, app, summaries.len(), &matches, columns[0]);
    draw_detail(f, app, doc, &summaries, &matches, columns[1]);
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

fn draw_list(f: &mut Frame, app: &App, total: usize, matches: &[ManagedMatch], area: Rect) {
    let searching = app.input_mode == InputMode::Managed(Mode::Search);
    let count_text = if app.managed.query.is_empty() {
        format!("{total} objects ")
    } else {
        format!("{}/{} objects ", matches.len(), total)
    };
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    crate::tui::list_chrome::draw_search_row(
        f,
        rows[0],
        &app.managed.query,
        searching,
        &count_text,
    );

    let height = rows[1].height as usize;
    let selected = app.managed.selected.min(matches.len().saturating_sub(1));
    let scroll =
        crate::tui::list_chrome::clamp_scroll(app.managed.scroll, selected, height, matches.len());
    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(height)
        .map(|(idx, item)| render_row(item, idx == selected))
        .collect();
    f.render_widget(Paragraph::new(lines), rows[1]);
}

fn render_row(item: &ManagedMatch, selected: bool) -> Line<'static> {
    let row_style = if selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
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
    let mut spans = vec![Span::styled(if selected { "▶ " } else { "  " }, row_style)];

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

    spans.push(Span::styled(
        format!("  {} props", item.properties),
        suffix_style,
    ));
    if item.hooks_inline > 0 {
        spans.push(Span::styled(
            format!(" · {} hooks", item.hooks_inline),
            suffix_style,
        ));
    }
    Line::from(spans)
}

fn draw_detail(
    f: &mut Frame,
    app: &App,
    doc: &Value,
    summaries: &[ObjectSummary],
    matches: &[ManagedMatch],
    area: Rect,
) {
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

    let selected = app.managed.selected.min(matches.len().saturating_sub(1));
    let Some(item) = matches.get(selected) else {
        status_line(f, inner, "no match", Color::DarkGray);
        return;
    };
    let Some(summary) = summaries.get(item.idx) else {
        return;
    };
    let Ok(object) = crate::managed::api::object_named(doc, &item.name) else {
        return;
    };

    let mut lines = vec![
        Line::from(Span::styled(
            item.name.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(
                "{} properties · {} inline hooks · {} file hooks",
                item.properties, item.hooks_inline, item.hooks_file
            ),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
    ];

    let properties = object
        .pointer("/schema/properties")
        .and_then(Value::as_object);
    let required: HashSet<&str> = object
        .pointer("/schema/required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut property_names: Vec<&str> = properties
        .into_iter()
        .flat_map(|properties| properties.keys().map(String::as_str))
        .collect();
    property_names.sort_unstable();

    let hook_lines = hook_lines(summary);
    let remaining = (inner.height as usize).saturating_sub(lines.len());
    let property_slots = remaining.saturating_sub(hook_lines.len().saturating_add(1));
    let shown_properties = if property_names.len() > property_slots {
        property_slots.saturating_sub(1)
    } else {
        property_names.len()
    };
    if let Some(properties) = properties {
        for name in property_names.iter().take(shown_properties) {
            lines.push(property_line(
                name,
                &properties[*name],
                required.contains(name),
            ));
        }
    }
    if property_names.len() > shown_properties && property_slots > 0 {
        lines.push(Line::from(Span::styled(
            format!("… (+{} more)", property_names.len() - shown_properties),
            Style::default().fg(Color::DarkGray),
        )));
    }

    if lines.len() < inner.height as usize {
        lines.push(Line::from(""));
    }
    lines.extend(hook_lines);
    lines.truncate(inner.height as usize);
    f.render_widget(Paragraph::new(lines), inner);
}

fn property_line(name: &str, property: &Value, required: bool) -> Line<'static> {
    let mut spans = vec![Span::styled(
        name.to_string(),
        Style::default().fg(Color::Gray),
    )];
    if required {
        spans.push(Span::styled(
            "*",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::styled(": ", Style::default().fg(Color::DarkGray)));
    spans.push(Span::styled(
        property_type(property),
        Style::default().fg(Color::White),
    ));
    Line::from(spans)
}

fn property_type(property: &Value) -> String {
    match property.get("type") {
        Some(Value::String(kind)) => base_type(kind, property),
        Some(Value::Array(kinds)) if kinds.iter().any(|kind| kind.as_str() == Some("null")) => {
            let base = kinds
                .iter()
                .filter_map(Value::as_str)
                .find(|kind| *kind != "null")
                .map(|kind| base_type(kind, property))
                .unwrap_or_else(|| "any".to_string());
            format!("{base}?")
        }
        _ => "any".to_string(),
    }
}

fn base_type(kind: &str, property: &Value) -> String {
    match kind {
        "string" | "boolean" | "number" | "object" | "relationship" => kind.to_string(),
        "array" => {
            let item = property
                .pointer("/items/type")
                .and_then(Value::as_str)
                .map(|kind| base_type(kind, &Value::Null))
                .unwrap_or_else(|| "any".to_string());
            format!("{item}[]")
        }
        _ => "any".to_string(),
    }
}

fn hook_lines(summary: &ObjectSummary) -> Vec<Line<'static>> {
    if summary.hooks_inline.is_empty() && summary.hooks_file.is_empty() {
        return vec![Line::from(Span::styled(
            "(no inline hooks)",
            Style::default().fg(Color::DarkGray),
        ))];
    }

    let mut lines = Vec::new();
    for name in &summary.hooks_inline {
        lines.push(Line::from(vec![
            Span::styled("hook  ", Style::default().fg(Color::DarkGray)),
            Span::styled(name.clone(), Style::default().fg(Color::Green)),
            Span::styled(
                format!("  (sync: aic script pull managed/{}.{name})", summary.name),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    for name in &summary.hooks_file {
        lines.push(Line::from(vec![
            Span::styled("hook  ", Style::default().fg(Color::DarkGray)),
            Span::styled(name.clone(), Style::default().fg(Color::DarkGray)),
            Span::styled("  (file, read-only)", Style::default().fg(Color::DarkGray)),
        ]));
    }
    lines
}
