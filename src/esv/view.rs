//! Ratatui rendering for the ESV variables tab.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::{App, InputMode};
use crate::esv::screen::Mode as EsvMode;
use crate::esv::state::{
    EditField as EsvEditField, ExpressionType as EsvExpressionType, LoadState as EsvLoadState,
    Match as EsvMatch, id_of as esv_id,
};

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let tenant_name = match app.active_tenant() {
        Some(t) => t.name.as_str(),
        None => return,
    };

    // Sub-view toggle header: `Variables | Secrets` with the active half lit.
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    draw_view_toggle(f, app, rows[0]);
    let area = rows[1];

    let mappings_allowed = app
        .active_tenant()
        .is_some_and(|tenant| tenant.allows_secret_mappings());
    match app.esv.view.clamp(mappings_allowed) {
        crate::esv::state::EsvView::Secrets => {
            crate::secrets::view::draw_body(f, app, area);
            return;
        }
        crate::esv::state::EsvView::Mappings => {
            crate::secretmap::view::draw(f, app, area);
            return;
        }
        crate::esv::state::EsvView::Variables => {}
    }

    // Loading / failed / empty: full-width status; no split, no preview pane.
    match app.esv.list.data.get(tenant_name) {
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
    let columns =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);
    draw_esv_list(f, app, &matches, columns[0]);
    draw_esv_preview(f, app, &matches, columns[1]);
}

/// One-line sub-view toggle header. The active view is bold-white, inactive
/// views are dim; Mappings only appears for lower-environment tenants.
fn draw_view_toggle(f: &mut Frame, app: &App, area: Rect) {
    use crate::esv::state::EsvView;
    let mappings_allowed = app
        .active_tenant()
        .is_some_and(|tenant| tenant.allows_secret_mappings());
    let active = app.esv.view.clamp(mappings_allowed);
    let tab = |label: &'static str, is_active: bool| {
        let style = if is_active {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        Span::styled(label, style)
    };
    let mut spans = vec![
        Span::raw(" "),
        tab("Variables", active == EsvView::Variables),
        Span::styled("  |  ", Style::default().fg(Color::DarkGray)),
        tab("Secrets", active == EsvView::Secrets),
    ];
    if mappings_allowed {
        spans.push(Span::styled("  |  ", Style::default().fg(Color::DarkGray)));
        spans.push(tab("Mappings", active == EsvView::Mappings));
    }
    spans.push(Span::styled(
        "   ([ ] to switch)",
        Style::default().fg(Color::DarkGray),
    ));
    let line = Line::from(spans);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_esv_list(f: &mut Frame, app: &App, matches: &[EsvMatch], area: Rect) {
    let searching = app.input_mode == InputMode::Esv(EsvMode::Search);
    let total = match app
        .active_tenant()
        .and_then(|t| app.esv.list.data.get(&t.name))
    {
        Some(EsvLoadState::Loaded(vs)) => vs.len(),
        _ => 0,
    };
    let count_text = if app.esv.list.query.is_empty() {
        format!("{} ESVs ", total)
    } else {
        format!("{}/{} ESVs ", matches.len(), total)
    };

    let rows = Layout::vertical([
        Constraint::Length(1), // /query (left) + count (right)
        Constraint::Min(0),    // list
    ])
    .split(area);

    crate::tui::list_chrome::draw_search_row(
        f,
        rows[0],
        &app.esv.list.query,
        searching,
        &count_text,
    );

    // Visible window: keep the selection inside [scroll, scroll + h).
    let h = rows[1].height as usize;
    let n = matches.len();
    let selected = app.esv.list.selected.min(n.saturating_sub(1));
    let scroll = crate::tui::list_chrome::clamp_scroll(app.esv.list.scroll, selected, h, n);

    let tenant_name = app.active_tenant().map(|t| t.name.clone());
    let loaded_items: Option<&Vec<serde_json::Value>> = tenant_name
        .as_ref()
        .and_then(|t| app.esv.list.data.get(t))
        .and_then(|s| match s {
            EsvLoadState::Loaded(v) => Some(v),
            _ => None,
        });
    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(h)
        .map(|(i, m)| {
            let failed = tenant_name
                .as_ref()
                .map(|t| app.esv.failed_writes.contains(&(t.clone(), m.id.clone())))
                .unwrap_or(false);
            let pending = loaded_items
                .and_then(|items| m.idx.and_then(|idx| items.get(idx)))
                .map(|v| {
                    crate::esv::state::is_pending(v)
                        || tenant_name
                            .as_ref()
                            .and_then(|t| app.esv.list.pending_ids.get(t))
                            .is_some_and(|ids| ids.contains(&m.id))
                })
                .unwrap_or(false);
            render_esv_row(m, i == selected, failed, pending)
        })
        .collect();
    f.render_widget(Paragraph::new(lines), rows[1]);
}

fn render_esv_row(m: &EsvMatch, is_selected: bool, failed: bool, pending: bool) -> Line<'static> {
    // Styling axes:
    //   selected: cyan-on-black bar with ▶
    //   failed:   red — background save errored, user should retry
    //   deleted:  red — local tombstone kept for undo
    //   pending:  body text stays gray; only the gutter glyph turns green
    //             (handled below where we render the gutter span)
    //   default:  gray
    // failed/deleted take precedence over pending. selected always overlays
    // the ▶ glyph while preserving a second-column alert marker.
    let alert = failed || m.deleted;
    let row_fg = if alert { Color::Red } else { Color::Gray };
    let row_style = if is_selected {
        Style::default()
            .fg(if alert { Color::Red } else { Color::Black })
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(row_fg)
    };
    let match_style = if is_selected {
        row_style.add_modifier(Modifier::UNDERLINED)
    } else if alert {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    };

    // Gutter glyph + colour. Search-friendly: every pending row contains a
    // literal `!` so the user can `/!` to filter to just-the-pending-rows.
    let (leader, leader_style) = match (is_selected, alert, pending) {
        (true, true, _) | (true, false, true) => ("▶!", row_style),
        (true, false, false) => ("▶ ", row_style),
        (false, true, _) => (
            "! ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        (false, false, true) => (
            "! ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        (false, false, false) => ("  ", row_style),
    };
    let mut spans = vec![Span::styled(leader, leader_style)];

    if m.positions.is_empty() {
        spans.push(Span::styled(m.id.clone(), row_style));
    } else {
        // Highlight matched chars; positions are sorted utf32 indices.
        let mut pos_iter = m.positions.iter().copied().peekable();
        for (i, c) in m.id.chars().enumerate() {
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

fn draw_esv_preview(f: &mut Frame, app: &App, matches: &[EsvMatch], area: Rect) {
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split off a 3-row banner at the top whenever there's something to
    // tell the user (pending changes, queued saves, or an in-flight
    // restart). Sits above the form, full-width edge-to-edge.
    let Some(tenant) = app.active_tenant() else {
        return;
    };
    let banner = crate::esv::state::banner_state(app, &tenant.name);
    let (banner_area, content_area) = if !matches!(banner, crate::esv::state::BannerState::None) {
        let rows = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1), // gap below banner
            Constraint::Min(0),
        ])
        .split(inner);
        (Some(rows[0]), rows[2])
    } else {
        (None, inner)
    };
    if let Some(banner_rect) = banner_area {
        draw_pending_banner(f, banner_rect, banner);
    }

    // Add a 2-col left gutter so the form text doesn't hug the border.
    let inner = Rect {
        x: content_area.x + 2,
        y: content_area.y,
        width: content_area.width.saturating_sub(2),
        height: content_area.height,
    };

    // Create flow: there's no on-server snapshot yet — the form is
    // entirely driven by EditState.
    if app
        .esv
        .editing
        .as_ref()
        .is_some_and(|e| e.creating && app.input_mode == InputMode::Esv(EsvMode::Edit))
    {
        draw_esv_form(f, app, None, inner);
        return;
    }

    let editing_id = app
        .esv
        .editing
        .as_ref()
        .filter(|_| app.input_mode == InputMode::Esv(EsvMode::Edit))
        .map(|e| e.id.as_str());

    // Prefer the in-progress edit's snapshot as the rendered variable
    // when we're in edit mode — that way the read-only rows still show
    // the original fields even if the user has scrolled the list away.
    let v_owned: Option<serde_json::Value> = if let (Some(id), Some(EsvLoadState::Loaded(items))) =
        (editing_id, app.esv.list.data.get(&tenant.name))
    {
        items.iter().find(|v| esv_id(v) == id).cloned()
    } else {
        let selected = app.esv.list.selected.min(matches.len().saturating_sub(1));
        match (matches.get(selected), app.esv.list.data.get(&tenant.name)) {
            (Some(m), _) if m.deleted => app
                .esv
                .recent_deletes
                .get(&(tenant.name.clone(), m.id.clone()))
                .map(|t| t.body.clone()),
            (Some(m), Some(EsvLoadState::Loaded(items))) => {
                m.idx.and_then(|idx| items.get(idx)).cloned()
            }
            _ => None,
        }
    };
    let Some(v) = v_owned else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "no match",
                Style::default().fg(Color::DarkGray),
            ))),
            inner,
        );
        return;
    };

    draw_esv_form(f, app, Some(&v), inner);
}

/// Render the ESV form. Same skeleton in preview, edit, and create modes —
/// editable rows are drawn unfocused in preview, focusable in edit/create.
/// `snapshot` is `None` when creating (no server-side variable yet); the
/// `_id` title becomes an editable field and the metadata rows are hidden.
fn draw_esv_form(f: &mut Frame, app: &App, snapshot: Option<&serde_json::Value>, area: Rect) {
    let editing = app
        .esv
        .editing
        .as_ref()
        .filter(|_| app.input_mode == InputMode::Esv(EsvMode::Edit));

    let creating = editing.is_some_and(|e| e.creating);

    let id_owned: String;
    let last_changed_date: &str;
    let last_changed_by: &str;
    let loaded;
    if let Some(v) = snapshot {
        id_owned = esv_id(v).to_string();
        last_changed_date = v
            .get("lastChangeDate")
            .and_then(|x| x.as_str())
            .unwrap_or("—");
        last_changed_by = v
            .get("lastChangedBy")
            .and_then(|x| x.as_str())
            .unwrap_or("—");
        loaded = v.get("loaded").and_then(|x| x.as_bool()).unwrap_or(false);
    } else {
        id_owned = String::new();
        last_changed_date = "—";
        last_changed_by = "—";
        loaded = false;
    };

    let error_h =
        if editing.is_some_and(|edit| edit.value.error().is_some() || edit.error.is_some()) {
            2
        } else {
            0
        };
    let save_h = if editing.is_some() { 2 } else { 0 };
    // The `_id` row is a 1-line cyan-bold title in preview/edit but a
    // 2-line TextField in create. Metadata rows hide entirely in create.
    let id_h = if creating { 2 } else { 1 };
    let meta_h = if creating { 0 } else { 1 };

    let rows = Layout::vertical([
        Constraint::Length(id_h),   // _id (title or editable field)
        Constraint::Length(1),      // gap
        Constraint::Length(meta_h), // last changed
        Constraint::Length(meta_h), // loaded
        Constraint::Length(meta_h), // gap
        Constraint::Length(2),      // description
        Constraint::Length(1),
        Constraint::Length(2), // type
        Constraint::Length(1),
        Constraint::Min(3), // value
        Constraint::Length(error_h),
        Constraint::Length(save_h),
    ])
    .split(area);

    // _id row — title in edit/preview, editable in create.
    if creating {
        if let Some(e) = editing {
            e.id_input.draw(f, rows[0], e.focused == EsvEditField::Id);
        }
    } else {
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                id_owned.clone(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )])),
            rows[0],
        );
    }

    if !creating {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Last changed  ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{last_changed_date} by {last_changed_by}"),
                    Style::default().fg(Color::Gray),
                ),
            ])),
            rows[2],
        );
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Loaded        ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    if loaded {
                        "✓ yes"
                    } else {
                        "✗ no (restart pending)"
                    },
                    Style::default().fg(if loaded { Color::Green } else { Color::Yellow }),
                ),
            ])),
            rows[3],
        );
    }

    // Editable rows — always drawn from `editing` when present so the
    // user sees their in-progress changes; otherwise drawn from the
    // snapshot. `snapshot` is only `None` when creating, in which case
    // `editing` is always Some and we never hit the else branches.
    let null = serde_json::Value::Null;
    let v: &serde_json::Value = snapshot.unwrap_or(&null);
    let description_focused = editing.is_some_and(|e| e.focused == EsvEditField::Description);
    let type_focused = editing.is_some_and(|e| e.focused == EsvEditField::Type);
    let value_focused = editing.is_some_and(|e| e.focused == EsvEditField::Value);
    let save_focused = editing.is_some_and(|e| e.focused == EsvEditField::Save);

    if let Some(e) = editing {
        e.description.draw(f, rows[5], description_focused);
    } else {
        let desc = v.get("description").and_then(|x| x.as_str()).unwrap_or("");
        let field = crate::tui::widgets::TextField::single_line("Description").with_initial(desc);
        field.draw(f, rows[5], false);
    }

    draw_type_row(
        f,
        rows[7],
        match editing {
            Some(e) => e.expr_type,
            None => EsvExpressionType::parse(
                v.get("expressionType")
                    .and_then(|x| x.as_str())
                    .unwrap_or(""),
            ),
        },
        type_focused,
    );

    if let Some(e) = editing {
        e.value.draw(f, rows[9], value_focused);
    } else {
        // Show the decoded value if it's UTF-8; fall back to the base64
        // string itself when it isn't (matches the edit-form fallback).
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD as B64;
        let v_b64 = v.get("valueBase64").and_then(|x| x.as_str()).unwrap_or("");
        let text = match B64.decode(v_b64) {
            Ok(bytes) => String::from_utf8(bytes).unwrap_or_else(|_| v_b64.to_string()),
            Err(_) => v_b64.to_string(),
        };
        let field = crate::tui::widgets::TextField::textarea("Value").with_initial(text);
        field.draw(f, rows[9], false);
    }

    if let Some(e) = editing {
        if let Some(err) = e.value.error().or(e.error.as_deref()) {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    err.to_string(),
                    Style::default().fg(Color::Yellow),
                )))
                .wrap(Wrap { trim: false }),
                rows[10],
            );
        }
        draw_esv_save_button(f, rows[11], save_focused);
    }
}

/// "Type:" label + the current expression-type rendered as a chip on a
/// dark-fill row. Matches the input-field styling so preview and edit
/// look the same.
fn draw_type_row(f: &mut Frame, area: Rect, expr_type: EsvExpressionType, focused: bool) {
    if area.height == 0 {
        return;
    }
    let label_area = Rect { height: 1, ..area };
    let label_style = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    f.render_widget(
        Paragraph::new(Span::styled("Type (←/→ to cycle)", label_style)),
        label_area,
    );
    if area.height < 2 {
        return;
    }
    let value_area = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: 1,
    };
    let bg = if focused {
        Color::Indexed(236)
    } else {
        Color::Indexed(234)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default().bg(bg)),
            Span::styled(
                format!(" {} ", expr_type.as_str()),
                Style::default().fg(Color::Cyan).bg(bg),
            ),
            Span::styled("  ", Style::default().bg(bg)),
        ]))
        .style(Style::default().bg(bg)),
        value_area,
    );
}

/// Pastel-blue banner above the form when one or more variables are
/// saved but not yet loaded by the runtime. 3 rows: blank top, message,
/// blank bottom. 2-col side margins inside the text row.
fn draw_pending_banner(f: &mut Frame, area: Rect, state: crate::esv::state::BannerState) {
    use crate::esv::state::BannerState;
    let (bg, count, msg): (Color, usize, String) = match state {
        BannerState::None => return,
        BannerState::ToApply(n) => (
            Color::Indexed(153), // pastel blue  #afd7ff
            n,
            format!(
                "ⓘ  You have {n} {noun} to apply. Press ^S to apply.",
                noun = if n == 1 { "change" } else { "changes" }
            ),
        ),
        BannerState::Queued(n) => (
            Color::Indexed(183), // pastel purple #d7afff
            n,
            format!(
                "↻  You have {n} {noun} queued — waiting for save to complete…",
                noun = if n == 1 { "change" } else { "changes" }
            ),
        ),
        BannerState::Applying(n) => {
            let msg = if n == 0 {
                "ⓘ  Runtime restart in progress…".to_string()
            } else {
                format!(
                    "ⓘ  You have {n} {noun} applying — runtime restart in progress…",
                    noun = if n == 1 { "change" } else { "changes" }
                )
            };
            (Color::Indexed(223), n, msg)
        }
    };
    let _ = count; // silence unused-on-no-arm
    let fg = Color::Indexed(232); // near-black so the pastel reads as the strip colour
    f.render_widget(Block::default().style(Style::default().bg(bg)), area);
    if area.height < 2 {
        return;
    }
    let text_row = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: area.width.saturating_sub(4),
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Span::styled(
            msg,
            Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
        )),
        text_row,
    );
}

fn draw_esv_save_button(f: &mut Frame, area: Rect, focused: bool) {
    if area.height == 0 {
        return;
    }
    let style = if focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green).bg(Color::Indexed(234))
    };
    let row = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: 1,
    };
    f.render_widget(Paragraph::new(Span::styled(" Save ", style)), row);
}
