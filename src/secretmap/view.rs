//! Secret-mapping browser: searchable mapping list, helper text detail pane,
//! and the ESV-secret alias picker modal.

use std::collections::HashSet;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{App, InputMode};
use crate::secretmap::labels;
use crate::secretmap::screen::Mode;
use crate::secretmap::state::{AliasMatch, LabelMatch, LoadState, MappingMatch};

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };

    match app.secretmap.data.get(&tenant) {
        None | Some(LoadState::Loading) => {
            crate::tui::list_chrome::draw_status_line(
                f,
                area,
                "  Loading secret mappings...",
                Color::DarkGray,
            );
            draw_active_modal(f, app, area);
            return;
        }
        Some(LoadState::Failed(error)) => {
            crate::tui::list_chrome::draw_status_line(
                f,
                area,
                &format!("  Secret mappings failed: {error}"),
                Color::Red,
            );
            draw_active_modal(f, app, area);
            return;
        }
        Some(LoadState::Loaded(mappings)) if mappings.is_empty() => {
            crate::tui::list_chrome::draw_status_line(
                f,
                area,
                "  No secret mappings found.",
                Color::DarkGray,
            );
            draw_active_modal(f, app, area);
            return;
        }
        Some(LoadState::Loaded(_)) => {}
    }

    let matches = app.secretmap.matches(Some(&tenant));
    let columns =
        Layout::horizontal([Constraint::Percentage(46), Constraint::Percentage(54)]).split(area);
    draw_list(f, app, &tenant, &matches, columns[0]);
    draw_detail(f, app, &matches, columns[1]);

    draw_active_modal(f, app, area);
}

fn draw_active_modal(f: &mut Frame, app: &App, area: Rect) {
    match app.input_mode {
        InputMode::Secretmap(Mode::PickLabel) => draw_label_picker(f, app, area),
        InputMode::Secretmap(Mode::PickAlias) => draw_alias_picker(f, app, area),
        _ => {}
    }
}

fn draw_list(f: &mut Frame, app: &App, tenant: &str, matches: &[MappingMatch], area: Rect) {
    let searching = app.input_mode == InputMode::Secretmap(Mode::Search);
    let total = match app.secretmap.data.get(tenant) {
        Some(LoadState::Loaded(mappings)) => mappings.len(),
        _ => 0,
    };
    let count_text = if app.secretmap.query.is_empty() {
        format!("{total} mappings ")
    } else {
        format!("{}/{} mappings ", matches.len(), total)
    };
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    crate::tui::list_chrome::draw_search_row(
        f,
        rows[0],
        &app.secretmap.query,
        searching,
        &count_text,
    );

    let height = rows[1].height as usize;
    let selected = app.secretmap.selected.min(matches.len().saturating_sub(1));
    let scroll = crate::tui::list_chrome::clamp_scroll(
        app.secretmap.scroll,
        selected,
        height,
        matches.len(),
    );
    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(height)
        .map(|(idx, item)| {
            let failed = app
                .secretmap
                .failed_writes
                .contains(&(tenant.to_string(), item.secret_id.clone()));
            let saving = app
                .secretmap
                .in_flight_writes
                .contains(&(tenant.to_string(), item.secret_id.clone()));
            render_mapping_row(item, idx == selected, failed, saving)
        })
        .collect();
    f.render_widget(Paragraph::new(lines), rows[1]);
}

fn render_mapping_row(
    item: &MappingMatch,
    selected: bool,
    failed: bool,
    saving: bool,
) -> Line<'static> {
    let row_style = if selected {
        Style::default()
            .fg(if failed { Color::Red } else { Color::White })
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else if failed {
        Style::default().fg(Color::Red)
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
    let secret_len = item.secret_id.chars().count();
    let positions: HashSet<usize> = item
        .positions
        .iter()
        .filter_map(|position| {
            let pos = *position as usize;
            (pos < secret_len).then_some(pos)
        })
        .collect();
    for (idx, ch) in item.secret_id.chars().enumerate() {
        let style = if positions.contains(&idx) {
            match_style
        } else {
            row_style
        };
        spans.push(Span::styled(ch.to_string(), style));
    }
    spans.push(Span::styled("  ->  ", suffix_style));
    spans.push(Span::styled(
        item.alias.clone().unwrap_or_else(|| "(unset)".to_string()),
        if selected {
            row_style
        } else if item.alias.is_some() {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        },
    ));
    if saving {
        spans.push(Span::styled("  saving", Style::default().fg(Color::Yellow)));
    } else if failed {
        spans.push(Span::styled("  failed", Style::default().fg(Color::Red)));
    }
    Line::from(spans)
}

fn draw_detail(f: &mut Frame, app: &App, matches: &[MappingMatch], area: Rect) {
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

    let selected = app.secretmap.selected.min(matches.len().saturating_sub(1));
    let Some(item) = matches.get(selected) else {
        crate::tui::list_chrome::draw_status_line(f, inner, "no match", Color::DarkGray);
        return;
    };

    let lines = vec![
        Line::from(Span::styled(
            item.secret_id.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("Category  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                labels::category(&item.secret_id),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            labels::describe(&item.secret_id),
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Alias      ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                item.alias.clone().unwrap_or_else(|| "(unset)".to_string()),
                if item.alias.is_some() {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
        ]),
        Line::from(vec![
            Span::styled("secretId   ", Style::default().fg(Color::DarkGray)),
            Span::styled(item.secret_id.clone(), Style::default().fg(Color::Gray)),
        ]),
    ];

    let height = inner.height as usize;
    // The pane wraps rather than pre-wrapping, because the alias and secretId
    // rows style label and value differently within one line. So the rendered
    // height is not `lines.len()`.
    let rendered = crate::tui::list_chrome::wrapped_height(&lines, inner.width);
    let scroll = app.secretmap.detail_scroll.clamp(rendered, height);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll as u16, 0)),
        inner,
    );
}

fn draw_alias_picker(f: &mut Frame, app: &App, area: Rect) {
    let Some(edit) = app.secretmap.editing.as_ref() else {
        return;
    };
    let width = area.width.min(72);
    let height = area.height.clamp(8, 20);
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " ESV alias ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(if edit.error.is_some() { 1 } else { 0 }),
        Constraint::Length(1),
    ])
    .split(inner);

    let tenant = app.active_tenant().map(|tenant| tenant.name.as_str());
    let matches = app.secretmap.alias_matches(tenant);
    let count_text = if app
        .active_tenant()
        .is_some_and(|tenant| app.secretmap.esv_secret_loading.contains(&tenant.name))
    {
        "loading ".to_string()
    } else {
        format!("{} secrets ", matches.len())
    };
    crate::tui::list_chrome::draw_search_row(f, rows[0], &edit.query, true, &count_text);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Mapping  ", Style::default().fg(Color::DarkGray)),
            Span::styled(edit.secret_id.clone(), Style::default().fg(Color::Gray)),
        ])),
        rows[1],
    );

    if app
        .active_tenant()
        .is_some_and(|tenant| app.secretmap.esv_secret_loading.contains(&tenant.name))
    {
        crate::tui::list_chrome::draw_status_line(
            f,
            rows[2],
            "  Loading ESV secrets...",
            Color::DarkGray,
        );
    } else if matches.is_empty() {
        crate::tui::list_chrome::draw_status_line(
            f,
            rows[2],
            "  No matching ESV secrets.",
            Color::DarkGray,
        );
    } else {
        draw_alias_matches(f, edit.selected, &matches, rows[2]);
    }

    if let Some(error) = &edit.error {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                error.clone(),
                Style::default().fg(Color::Yellow),
            ))),
            rows[3],
        );
    }
    f.render_widget(
        Paragraph::new(crate::tui::modal_chrome::hint_line(&[
            ("Enter", "choose"),
            ("↑/↓", "navigate"),
            ("Esc", "cancel"),
        ])),
        rows[4],
    );
}

fn draw_label_picker(f: &mut Frame, app: &App, area: Rect) {
    let Some(pick) = app.secretmap.picking_label.as_ref() else {
        return;
    };
    let width = area.width.min(92);
    let height = area.height.clamp(10, 24);
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Secret label ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(if pick.error.is_some() { 1 } else { 0 }),
        Constraint::Length(1),
    ])
    .split(inner);

    let tenant = app.active_tenant().map(|tenant| tenant.name.as_str());
    let matches = app.secretmap.label_matches(tenant);
    let count_text = if app
        .active_tenant()
        .is_some_and(|tenant| app.secretmap.valid_secret_loading.contains(&tenant.name))
    {
        "loading ".to_string()
    } else {
        format!("{} unmapped ", matches.len())
    };
    crate::tui::list_chrome::draw_search_row(f, rows[0], &pick.query, true, &count_text);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Realm  ", Style::default().fg(Color::DarkGray)),
            Span::styled(pick.realm.clone(), Style::default().fg(Color::Gray)),
        ])),
        rows[1],
    );

    let loading_labels = app
        .active_tenant()
        .is_some_and(|tenant| app.secretmap.valid_secret_loading.contains(&tenant.name));
    let loading_mappings = app.active_tenant().is_some_and(|tenant| {
        matches!(
            app.secretmap.data.get(&tenant.name),
            None | Some(LoadState::Loading)
        )
    });
    if loading_labels || loading_mappings {
        crate::tui::list_chrome::draw_status_line(
            f,
            rows[2],
            "  Loading unmapped secret labels...",
            Color::DarkGray,
        );
    } else if matches.is_empty() {
        crate::tui::list_chrome::draw_status_line(
            f,
            rows[2],
            "  No matching unmapped secret labels.",
            Color::DarkGray,
        );
    } else {
        draw_label_matches(f, pick.selected, &matches, rows[2]);
    }

    if let Some(error) = &pick.error {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                error.clone(),
                Style::default().fg(Color::Yellow),
            ))),
            rows[3],
        );
    }
    f.render_widget(
        Paragraph::new(crate::tui::modal_chrome::hint_line(&[
            ("Enter", "choose"),
            ("↑/↓", "navigate"),
            ("Esc", "cancel"),
        ])),
        rows[4],
    );
}

fn draw_alias_matches(f: &mut Frame, selected: usize, matches: &[AliasMatch], area: Rect) {
    let selected = selected.min(matches.len().saturating_sub(1));
    let height = area.height as usize;
    let scroll = crate::tui::list_chrome::clamp_scroll(0, selected, height, matches.len());
    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(height)
        .map(|(idx, item)| render_alias_row(item, idx == selected))
        .collect();
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_label_matches(f: &mut Frame, selected: usize, matches: &[LabelMatch], area: Rect) {
    let selected = selected.min(matches.len().saturating_sub(1));
    let height = area.height as usize;
    let scroll = crate::tui::list_chrome::clamp_scroll(0, selected, height, matches.len());
    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(height)
        .map(|(idx, item)| render_label_row(item, idx == selected))
        .collect();
    f.render_widget(Paragraph::new(lines), area);
}

fn render_label_row(item: &LabelMatch, selected: bool) -> Line<'static> {
    let row_style = if selected {
        Style::default()
            .fg(Color::White)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let helper_style = if selected {
        row_style
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let match_style = if selected {
        row_style.add_modifier(Modifier::UNDERLINED)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    };
    let id_len = item.id.chars().count();
    let positions: HashSet<usize> = item
        .positions
        .iter()
        .filter_map(|position| {
            let pos = *position as usize;
            (pos < id_len).then_some(pos)
        })
        .collect();
    let mut spans = vec![Span::styled("  ", row_style)];
    for (idx, ch) in item.id.chars().enumerate() {
        let style = if positions.contains(&idx) {
            match_style
        } else {
            row_style
        };
        spans.push(Span::styled(ch.to_string(), style));
    }
    spans.push(Span::styled("  ", helper_style));
    spans.push(Span::styled(item.description.clone(), helper_style));
    Line::from(spans)
}

fn render_alias_row(item: &AliasMatch, selected: bool) -> Line<'static> {
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
    let positions: HashSet<usize> = item
        .positions
        .iter()
        .map(|position| *position as usize)
        .collect();
    let mut spans = vec![Span::styled("  ", row_style)];
    for (idx, ch) in item.id.chars().enumerate() {
        let style = if positions.contains(&idx) {
            match_style
        } else {
            row_style
        };
        spans.push(Span::styled(ch.to_string(), style));
    }
    Line::from(spans)
}
