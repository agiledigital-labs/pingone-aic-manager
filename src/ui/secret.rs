//! Rendering for the secrets sub-view of the ESVs tab. State + behaviour live
//! in `crate::screens::secret`; this is draw-only.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::app::App;
use crate::screens::esv::{LoadState, id_of};
use crate::screens::secret::{self, CreateField, Encoding};
use crate::ui::modal_chrome::Modal;

/// The secrets list (left) + selected-secret detail (right). Mirrors the
/// variables view: borderless list, a left-border divider on the detail pane.
/// Called from `draw_esvs` when the tab is in the Secrets view; `area` excludes
/// the sub-view header row.
pub fn draw_body(f: &mut Frame, app: &App, area: Rect) {
    let tenant = match app.active_tenant() {
        Some(t) => t.name.as_str(),
        None => return,
    };
    // While the create form is open we always show the split (form in the
    // detail pane), even on a Loading/empty tenant — the empty-state message
    // tells the user to press ^N, so the form has to appear from there too.
    let creating =
        app.input_mode == crate::app::InputMode::SecretCreate && app.secret.create.is_some();
    if !creating {
        match app.secret.list.data.get(tenant) {
            None | Some(LoadState::Loading) => {
                status(f, area, "  Loading secrets…", Color::DarkGray);
                return;
            }
            Some(LoadState::Failed(e)) => {
                status(f, area, &format!("  Secret list failed: {e}"), Color::Red);
                return;
            }
            Some(LoadState::Loaded(vs)) if vs.is_empty() => {
                status(f, area, "  No secrets. ^N to create one.", Color::DarkGray);
                return;
            }
            Some(LoadState::Loaded(_)) => {}
        }
    }

    let columns =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);
    draw_list(f, app, columns[0]);
    draw_detail(f, app, columns[1]);
}

fn status(f: &mut Frame, area: Rect, msg: &str, color: Color) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            msg.to_string(),
            Style::default().fg(color),
        ))),
        area,
    );
}

fn draw_list(f: &mut Frame, app: &App, area: Rect) {
    let rows = secret::rows(app, app.active_tenant().map(|t| t.name.as_str()));
    let searching = app.input_mode == crate::app::InputMode::EsvSearch;
    let total = match app
        .active_tenant()
        .and_then(|t| app.secret.list.data.get(&t.name))
    {
        Some(LoadState::Loaded(vs)) => vs.len(),
        _ => 0,
    };
    let count_text = if app.secret.list.query.is_empty() {
        format!("{total} secrets ")
    } else {
        format!("{}/{} secrets ", rows.len(), total)
    };

    let layout = Layout::vertical([
        Constraint::Length(1), // /query (left) + count (right)
        Constraint::Min(0),    // list
    ])
    .split(area);

    crate::ui::draw_search_row(f, layout[0], &app.secret.list.query, searching, &count_text);

    // Windowed render, same scroll math as the variables list.
    let h = layout[1].height as usize;
    let n = rows.len();
    let selected = app.secret.list.selected.min(n.saturating_sub(1));
    let scroll = crate::ui::clamp_scroll(app.secret.list.scroll, selected, h, n);
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .skip(scroll)
        .take(h)
        .map(|(i, r)| render_secret_row(r, i == selected))
        .collect();
    f.render_widget(Paragraph::new(lines), layout[1]);
}

/// One secret row, styled to match the variables list: cyan selection bar,
/// a green `!` gutter for pending rows, dim metadata tags.
fn render_secret_row(r: &secret::SecretRow, is_selected: bool) -> Line<'static> {
    let row_style = if is_selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let meta_style = if is_selected {
        row_style
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let (leader, leader_style) = match (is_selected, r.pending) {
        (true, true) => ("▶!", row_style),
        (true, false) => ("▶ ", row_style),
        (false, true) => (
            "! ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        (false, false) => ("  ", row_style),
    };
    let mut spans = vec![
        Span::styled(leader, leader_style),
        Span::styled(r.id.clone(), row_style),
        Span::styled(format!("  [{}]", r.encoding), meta_style),
    ];
    if !r.use_in_placeholders {
        spans.push(Span::styled("  no-ph", meta_style));
    }
    Line::from(spans)
}

fn draw_detail(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // 2-col left gutter so text doesn't hug the divider (matches variables).
    let inner = Rect {
        x: inner.x + 2,
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height,
    };

    // Create flow: the New-Secret form lives in this pane (mirrors the
    // variables create form), not a modal.
    if app.input_mode == crate::app::InputMode::SecretCreate && app.secret.create.is_some() {
        draw_create_form(f, app, inner);
        return;
    }

    // Enter opens the interactive panel (metadata + editable description +
    // versions) in this pane, the same way Enter opens the edit form in the
    // variables pane — no modal.
    if secret::versions_panel_open(app) {
        draw_secret_panel(f, app, inner);
        return;
    }

    let Some(secret) = secret::selected_secret(app) else {
        f.render_widget(
            Paragraph::new(Span::styled(
                "no match",
                Style::default().fg(Color::DarkGray),
            )),
            inner,
        );
        return;
    };

    // Metadata block, then the description rendered as an (unfocused) input
    // field so the preview matches the variables preview, then the write-only
    // note.
    let lines = meta_lines(&secret);
    let meta_h = lines.len() as u16;
    let rows = Layout::vertical([
        Constraint::Length(meta_h), // metadata
        Constraint::Length(1),      // gap
        Constraint::Length(2),      // description (input-styled, read-only)
        Constraint::Length(1),      // gap
        Constraint::Min(1),         // write-only note
    ])
    .split(inner);

    f.render_widget(Paragraph::new(lines), rows[0]);
    crate::ui::widgets::TextField::single_line("Description")
        .with_initial(secret::description_of(&secret))
        .draw(f, rows[2], false);
    f.render_widget(
        Paragraph::new(Span::styled(
            "Value is write-only — never shown. Enter: versions & description.",
            Style::default().fg(Color::DarkGray),
        ))
        .wrap(Wrap { trim: false }),
        rows[4],
    );
}

fn field_str(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Fixed-width label span shared across the secret detail rows.
fn meta_label(k: &str) -> Span<'static> {
    Span::styled(format!("{k:<16}"), Style::default().fg(Color::DarkGray))
}

/// The read-only metadata rows (ID … Loaded, plus a pending note) shown in both
/// the static detail view and the interactive panel.
fn meta_lines(secret: &serde_json::Value) -> Vec<Line<'static>> {
    let id = id_of(secret).to_string();
    let encoding = secret::encoding_of(secret).to_string();
    let use_ph = secret::use_in_placeholders(secret);
    let active = field_str(secret, "activeVersion");
    let loaded_v = field_str(secret, "loadedVersion");
    let loaded = secret
        .get("loaded")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let pending = use_ph && active != loaded_v;
    let mut lines = vec![
        Line::from(vec![meta_label("ID"), Span::raw(id)]),
        Line::from(vec![meta_label("Encoding"), Span::raw(encoding)]),
        Line::from(vec![
            meta_label("In placeholders"),
            Span::raw(
                if use_ph {
                    "yes (gates restart)"
                } else {
                    "no (loads immediately)"
                }
                .to_string(),
            ),
        ]),
        Line::from(vec![
            meta_label("Active version"),
            Span::raw(if active.is_empty() {
                "—".into()
            } else {
                active
            }),
        ]),
        Line::from(vec![
            meta_label("Loaded version"),
            Span::raw(if loaded_v.is_empty() {
                "—".into()
            } else {
                loaded_v
            }),
        ]),
        Line::from(vec![
            meta_label("Loaded"),
            Span::styled(
                if loaded { "yes" } else { "no" }.to_string(),
                Style::default().fg(if loaded { Color::Green } else { Color::Yellow }),
            ),
        ]),
    ];
    if pending {
        lines.push(Line::from(Span::styled(
            "pending — apply with ^S",
            Style::default().fg(Color::Yellow),
        )));
    }
    lines
}

// --- Interactive detail panel (metadata + description + versions) ---------

/// The detail pane shown after Enter: read-only metadata, an editable
/// description, then the version list. `Tab` moves focus between the
/// description editor and the version list (see `handle_versions_key`).
fn draw_secret_panel(f: &mut Frame, app: &App, area: Rect) {
    // Resolve the subject from the panel's stored target, not the live list
    // selection — a background refresh can re-sort the list underneath us.
    let Some((tenant, id)) = app.secret.version_target.clone() else {
        return;
    };
    let meta = secret::secret_in_cache(app, &tenant, &id);
    let lines = meta.as_ref().map(meta_lines).unwrap_or_default();
    let meta_h = lines.len() as u16;
    let desc_focused = app.secret.detail_focus == secret::DetailFocus::Description;

    let rows = Layout::vertical([
        Constraint::Length(meta_h), // metadata block
        Constraint::Length(1),      // gap
        Constraint::Length(2),      // description editor (label + input)
        Constraint::Length(1),      // gap
        Constraint::Min(0),         // versions
    ])
    .split(area);

    f.render_widget(Paragraph::new(lines), rows[0]);
    // The TextField renders its own "Description" label and shows a cursor when
    // focused; the footer carries the Tab/Enter hints.
    app.secret.description.draw(f, rows[2], desc_focused);
    draw_version_list(f, app, rows[4]);
}

/// The "Versions" header + version list portion of the detail panel.
fn draw_version_list(f: &mut Frame, app: &App, area: Rect) {
    use secret::VersionsView;
    let vers_focused = app.secret.detail_focus == secret::DetailFocus::Versions;
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    f.render_widget(
        Paragraph::new(Span::styled(
            "Versions",
            if vers_focused {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        )),
        rows[0],
    );
    let body = rows[1];

    let versions = match secret::versions_view(app) {
        Some(VersionsView::Loaded { versions, .. }) => versions,
        Some(VersionsView::Loading) => {
            f.render_widget(
                Paragraph::new(Span::styled(
                    "Loading versions…",
                    Style::default().fg(Color::DarkGray),
                )),
                body,
            );
            return;
        }
        Some(VersionsView::Failed(e)) => {
            f.render_widget(
                Paragraph::new(Span::styled(
                    format!("Failed to load versions: {e}"),
                    Style::default().fg(Color::Red),
                ))
                .wrap(Wrap { trim: false }),
                body,
            );
            return;
        }
        None => return,
    };

    if versions.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "No versions.",
                Style::default().fg(Color::DarkGray),
            )),
            body,
        );
        return;
    }

    let items: Vec<ListItem> = versions
        .iter()
        .map(|v| {
            let version = v
                .get("version")
                .and_then(|x| {
                    x.as_str()
                        .map(|s| s.to_string())
                        .or_else(|| x.as_u64().map(|n| n.to_string()))
                })
                .unwrap_or_default();
            let stat = v.get("status").and_then(|x| x.as_str()).unwrap_or("?");
            let loaded = v.get("loaded").and_then(|x| x.as_bool()).unwrap_or(false);
            let stat_style = match stat {
                "ENABLED" => Style::default().fg(Color::Green),
                "DISABLED" => Style::default().fg(Color::Yellow),
                "DESTROYED" => Style::default().fg(Color::Red),
                _ => Style::default().fg(Color::DarkGray),
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("v{version:<4} "),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{stat:<10} "), stat_style),
                Span::styled(
                    if loaded { "loaded" } else { "" }.to_string(),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(
        app.secret
            .version_selected
            .min(versions.len().saturating_sub(1)),
    ));
    // De-emphasise the selection when the description editor holds focus, so it
    // reads clearly which half `j/k`, `e/d`, `x` act on.
    let (hl_style, hl_symbol) = if vers_focused {
        (
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            "▶ ",
        )
    } else {
        (Style::default().fg(Color::Gray), "  ")
    };
    let list = List::new(items)
        .highlight_style(hl_style)
        .highlight_symbol(hl_symbol);
    f.render_stateful_widget(list, body, &mut state);
}

// --- Create form (rendered in the detail pane) ----------------------------

/// The New-Secret form, rendered into the detail pane (`area`) — mirrors the
/// variables create form. The secrets list stays visible on the left.
fn draw_create_form(f: &mut Frame, app: &App, area: Rect) {
    let Some(form) = app.secret.create.as_ref() else {
        return;
    };
    let json_relevant = form.encoding == Encoding::Generic;

    let rows = Layout::vertical([
        Constraint::Length(1), // title
        Constraint::Length(1), // gap
        Constraint::Length(2), // id
        Constraint::Length(2), // description
        Constraint::Length(1), // gap
        Constraint::Length(1), // encoding
        Constraint::Length(1), // placeholders
        Constraint::Length(1), // json
        Constraint::Length(1), // gap
        Constraint::Length(2), // value
        Constraint::Length(1), // gap
        Constraint::Length(1), // save
        Constraint::Min(1),    // error
    ])
    .split(area);

    f.render_widget(
        Paragraph::new(Span::styled(
            "New secret",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        rows[0],
    );
    form.id.draw(f, rows[2], form.focused == CreateField::Id);
    form.description
        .draw(f, rows[3], form.focused == CreateField::Description);
    draw_chip_row(
        f,
        rows[5],
        "Encoding",
        Encoding::ALL
            .iter()
            .map(|e| (e.label(), *e == form.encoding))
            .collect(),
        form.focused == CreateField::Encoding,
    );
    draw_toggle_row(
        f,
        rows[6],
        "Use in placeholders",
        form.use_in_placeholders,
        form.focused == CreateField::Placeholders,
        "(off ⇒ loads immediately, no restart)",
    );
    draw_toggle_row(
        f,
        rows[7],
        "Validate as JSON",
        form.as_json,
        form.focused == CreateField::Json,
        if json_relevant {
            "(generic only)"
        } else {
            "(ignored for this encoding)"
        },
    );
    form.value
        .draw(f, rows[9], form.focused == CreateField::Value);
    draw_save(f, rows[11], form.focused == CreateField::Save);

    if let Some(err) = &form.error {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!("✗ {err}"),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ))
            .wrap(Wrap { trim: false }),
            rows[12],
        );
    }
}

pub fn draw_add_version(f: &mut Frame, app: &App) {
    let Some(form) = app.secret.add_version.as_ref() else {
        return;
    };
    let body = Modal {
        title: "Add secret version",
        status: Some(&form.id),
        hints: &[("Enter", "add version"), ("Esc", "cancel")],
        body_height: 4,
    }
    .draw(f, f.area());
    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(body);
    form.value.draw(f, rows[0], true);
    f.render_widget(
        Paragraph::new(Span::styled(
            format!("encoding: {}", form.encoding.as_str()),
            Style::default().fg(Color::DarkGray),
        )),
        rows[1],
    );
    if let Some(err) = &form.error {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!("✗ {err}"),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            rows[2],
        );
    }
}

fn draw_chip_row(f: &mut Frame, area: Rect, label: &str, chips: Vec<(&str, bool)>, focused: bool) {
    let mut spans = vec![Span::styled(format!("{label:<20} "), label_style(focused))];
    for (text, selected) in chips {
        let style = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(if focused { Color::Yellow } else { Color::Gray })
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(format!(" {text} "), style));
        spans.push(Span::raw(" "));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_toggle_row(f: &mut Frame, area: Rect, label: &str, on: bool, focused: bool, note: &str) {
    let box_ = if on { "[✓]" } else { "[ ]" };
    let spans = vec![
        Span::styled(format!("{label:<20} "), label_style(focused)),
        Span::styled(
            box_.to_string(),
            if focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            },
        ),
        Span::styled(format!("  {note}"), Style::default().fg(Color::DarkGray)),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_save(f: &mut Frame, area: Rect, focused: bool) {
    let style = if focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    f.render_widget(Paragraph::new(Span::styled(" Create secret ", style)), area);
}

fn label_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}
