//! Read-only OAuth2 client browser: searchable client-id list and scrollable
//! config detail with inherited-value wrappers unwrapped for display.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use serde_json::Value;

use crate::app::{App, InputMode};
use crate::oauth::screen::Mode;
use crate::oauth::state::{ClientMatch, LoadState, State};

const CONFIG_SECTIONS: [&str; 6] = [
    "coreOAuth2ClientConfig",
    "advancedOAuth2ClientConfig",
    "coreOpenIDClientConfig",
    "signEncOAuth2ClientConfig",
    "coreUmaClientConfig",
    "overrideOAuth2ClientConfig",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeafDisplay {
    pub text: String,
    pub inherited: bool,
}

pub fn draw_body(f: &mut Frame, app: &App, area: Rect) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };

    match app.oauth.data.get(&tenant) {
        None | Some(LoadState::Loading) => {
            crate::tui::list_chrome::draw_status_line(
                f,
                area,
                "  Loading OAuth clients...",
                Color::DarkGray,
            );
            return;
        }
        Some(LoadState::Failed(error)) => {
            crate::tui::list_chrome::draw_status_line(
                f,
                area,
                &format!("  OAuth client list failed: {error}"),
                Color::Red,
            );
            return;
        }
        Some(LoadState::Loaded(clients)) if clients.is_empty() => {
            crate::tui::list_chrome::draw_status_line(
                f,
                area,
                "  No OAuth clients found.",
                Color::DarkGray,
            );
            return;
        }
        Some(LoadState::Loaded(_)) => {}
    }

    let matches = app.oauth.matches(Some(&tenant));
    let columns =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);
    draw_list(f, app, &tenant, &matches, columns[0]);
    draw_detail(f, app, &tenant, &matches, columns[1]);
}

fn draw_list(f: &mut Frame, app: &App, tenant: &str, matches: &[ClientMatch], area: Rect) {
    let searching = app.input_mode == InputMode::Oauth(Mode::Search);
    let total = match app.oauth.data.get(tenant) {
        Some(LoadState::Loaded(clients)) => clients.len(),
        _ => 0,
    };
    let count_text = if app.oauth.query.is_empty() {
        format!("{total} clients ")
    } else {
        format!("{}/{} clients ", matches.len(), total)
    };

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    crate::tui::list_chrome::draw_search_row(f, rows[0], &app.oauth.query, searching, &count_text);

    let height = rows[1].height as usize;
    let selected = app.oauth.selected.min(matches.len().saturating_sub(1));
    let scroll =
        crate::tui::list_chrome::clamp_scroll(app.oauth.scroll, selected, height, matches.len());
    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(height)
        .map(|(idx, item)| render_row(item, idx == selected))
        .collect();
    f.render_widget(Paragraph::new(lines), rows[1]);
}

fn render_row(item: &ClientMatch, selected: bool) -> Line<'static> {
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
    let mut spans = vec![Span::styled("  ", row_style)];

    if item.positions.is_empty() {
        spans.push(Span::styled(item.id.clone(), row_style));
    } else {
        let mut positions = item.positions.iter().copied().peekable();
        for (idx, ch) in item.id.chars().enumerate() {
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
    Line::from(spans)
}

fn draw_detail(f: &mut Frame, app: &App, tenant: &str, matches: &[ClientMatch], area: Rect) {
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

    let selected = app.oauth.selected.min(matches.len().saturating_sub(1));
    let Some(item) = matches.get(selected) else {
        crate::tui::list_chrome::draw_status_line(f, inner, "no match", Color::DarkGray);
        return;
    };
    let detail_key = State::detail_key(tenant, &item.id);
    if app.oauth.detail_loading.contains(&detail_key) {
        draw_detail_status(
            f,
            inner,
            &item.id,
            "Loading client config...",
            Color::DarkGray,
        );
        return;
    }
    if let Some(error) = app.oauth.detail_failed.get(&detail_key) {
        draw_detail_status(
            f,
            inner,
            &item.id,
            &format!("Client config failed: {error}"),
            Color::Red,
        );
        return;
    }
    let Some(client) = app.oauth.detail_cache.get(&detail_key) else {
        draw_detail_status(
            f,
            inner,
            &item.id,
            "Press Enter to load client config.",
            Color::DarkGray,
        );
        return;
    };

    let lines = render_client_lines(client, &item.id);
    let height = inner.height as usize;
    let max_scroll = lines.len().saturating_sub(height);
    let scroll = app.oauth.detail_scroll.min(max_scroll);
    f.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), inner);
}

fn draw_detail_status(f: &mut Frame, area: Rect, id: &str, message: &str, color: Color) {
    let lines = vec![
        Line::from(Span::styled(
            id.to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            message.to_string(),
            Style::default().fg(color),
        )),
    ];
    f.render_widget(Paragraph::new(lines), area);
}

fn render_client_lines(client: &Value, fallback_id: &str) -> Vec<Line<'static>> {
    let id = client
        .get("_id")
        .and_then(Value::as_str)
        .unwrap_or(fallback_id);
    let mut lines = vec![Line::from(Span::styled(
        id.to_string(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))];
    if let Some(rev) = client.get("_rev").and_then(Value::as_str) {
        lines.push(Line::from(vec![
            Span::styled("_rev  ", Style::default().fg(Color::DarkGray)),
            Span::styled(rev.to_string(), Style::default().fg(Color::DarkGray)),
        ]));
    }

    let mut rendered_any_section = false;
    for section in CONFIG_SECTIONS {
        if let Some(value) = client.get(section) {
            rendered_any_section = true;
            lines.push(Line::from(""));
            lines.push(section_header(section));
            push_value_lines(&mut lines, 1, section, value);
        }
    }

    if !rendered_any_section {
        lines.push(Line::from(""));
        lines.push(section_header("raw JSON"));
        for line in serde_json::to_string_pretty(client)
            .unwrap_or_else(|_| client.to_string())
            .lines()
        {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(Color::Gray),
            )));
        }
    }

    lines
}

fn section_header(label: &str) -> Line<'static> {
    Line::from(Span::styled(
        label.to_string(),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ))
}

fn push_value_lines(lines: &mut Vec<Line<'static>>, indent: usize, label: &str, value: &Value) {
    if is_inherited_wrapper(value) || !value.is_object() {
        lines.push(leaf_line(indent, label, render_leaf_value(value)));
        return;
    }

    let Some(map) = value.as_object() else {
        lines.push(leaf_line(indent, label, render_leaf_value(value)));
        return;
    };
    if map.is_empty() {
        lines.push(leaf_line(
            indent,
            label,
            LeafDisplay {
                text: "{}".into(),
                inherited: false,
            },
        ));
        return;
    }

    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    for key in keys {
        let child = &map[key];
        if is_inherited_wrapper(child) || !child.is_object() {
            lines.push(leaf_line(indent, key, render_leaf_value(child)));
        } else {
            lines.push(Line::from(vec![
                Span::raw("  ".repeat(indent)),
                Span::styled(key.clone(), Style::default().fg(Color::Gray)),
            ]));
            push_value_lines(lines, indent + 1, key, child);
        }
    }
}

fn leaf_line(indent: usize, label: &str, display: LeafDisplay) -> Line<'static> {
    let mut spans = vec![
        Span::raw("  ".repeat(indent)),
        Span::styled(label.to_string(), Style::default().fg(Color::Gray)),
        Span::styled(": ", Style::default().fg(Color::DarkGray)),
        Span::styled(display.text, Style::default().fg(Color::White)),
    ];
    if display.inherited {
        spans.push(Span::styled(
            " (inherited)",
            Style::default().fg(Color::DarkGray),
        ));
    }
    Line::from(spans)
}

fn is_inherited_wrapper(value: &Value) -> bool {
    value.get("inherited").and_then(Value::as_bool).is_some() && value.get("value").is_some()
}

pub(crate) fn render_leaf_value(value: &Value) -> LeafDisplay {
    let inherited = value
        .get("inherited")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let effective = if is_inherited_wrapper(value) {
        value.get("value").unwrap_or(&Value::Null)
    } else {
        value
    };
    LeafDisplay {
        text: display_value(effective),
        inherited,
    }
}

fn display_value(value: &Value) -> String {
    match value {
        Value::String(value) if value.is_empty() => "\"\"".into(),
        Value::String(value) => value.clone(),
        Value::Null => "null".into(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn render_leaf_value_unwraps_inherited_string() {
        assert_eq!(
            render_leaf_value(&json!({"inherited": true, "value": "https://app/cb"})),
            LeafDisplay {
                text: "https://app/cb".into(),
                inherited: true,
            }
        );
    }

    #[test]
    fn render_leaf_value_unwraps_non_inherited_array() {
        assert_eq!(
            render_leaf_value(&json!({"inherited": false, "value": ["openid", "profile"]})),
            LeafDisplay {
                text: r#"["openid","profile"]"#.into(),
                inherited: false,
            }
        );
    }

    #[test]
    fn render_leaf_value_handles_bare_values() {
        assert_eq!(
            render_leaf_value(&json!(true)),
            LeafDisplay {
                text: "true".into(),
                inherited: false,
            }
        );
        assert_eq!(
            render_leaf_value(&json!({"nested": 1})),
            LeafDisplay {
                text: r#"{"nested":1}"#.into(),
                inherited: false,
            }
        );
    }
}
