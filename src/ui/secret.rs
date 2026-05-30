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

/// The secrets list (left) + selected-secret detail (right). Called from
/// `draw_esvs` when the tab is in the Secrets view; `area` excludes the
/// sub-view header row.
pub fn draw_body(f: &mut Frame, app: &App, area: Rect) {
    let tenant = match app.active_tenant() {
        Some(t) => t.name.as_str(),
        None => return,
    };
    match app.secret.data.get(tenant) {
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

    let columns =
        Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).split(area);
    draw_list(f, app, columns[0]);
    draw_detail(f, app, columns[1]);
}

fn status(f: &mut Frame, area: Rect, msg: &str, color: Color) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(msg.to_string(), Style::default().fg(color)))),
        area,
    );
}

fn draw_list(f: &mut Frame, app: &App, area: Rect) {
    let rows = secret::rows(app, app.active_tenant().map(|t| t.name.as_str()));
    let searching = app.input_mode == crate::app::InputMode::EsvSearch;
    let title = if searching || !app.secret.query.value().is_empty() {
        format!(" secrets  /{} ", app.secret.query.value())
    } else {
        format!(" secrets ({}) ", rows.len())
    };
    let block = Block::default().borders(Borders::ALL).title(title);

    let selected = app.secret.selected.min(rows.len().saturating_sub(1));
    let items: Vec<ListItem> = rows
        .iter()
        .map(|r| {
            let mut spans = vec![Span::raw(r.id.clone())];
            // encoding tag + placeholder / pending markers.
            spans.push(Span::styled(
                format!("  [{}]", r.encoding),
                Style::default().fg(Color::DarkGray),
            ));
            if !r.use_in_placeholders {
                spans.push(Span::styled(
                    "  no-ph",
                    Style::default().fg(Color::DarkGray),
                ));
            }
            if r.pending {
                spans.push(Span::styled(
                    "  !",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut state = ListState::default();
    if !rows.is_empty() {
        state.select(Some(selected));
    }
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_detail(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(" detail ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(secret) = secret::selected_secret(app) else {
        f.render_widget(
            Paragraph::new(Span::styled("no match", Style::default().fg(Color::DarkGray))),
            inner,
        );
        return;
    };

    let id = id_of(&secret).to_string();
    let encoding = secret::encoding_of(&secret).to_string();
    let use_ph = secret::use_in_placeholders(&secret);
    let active = field_str(&secret, "activeVersion");
    let loaded_v = field_str(&secret, "loadedVersion");
    let loaded = secret.get("loaded").and_then(|v| v.as_bool()).unwrap_or(false);
    let description = field_str(&secret, "description");

    let pending = use_ph && active != loaded_v;
    let label = |k: &str| Span::styled(format!("{k:<16}"), Style::default().fg(Color::DarkGray));
    let mut lines = vec![
        Line::from(vec![label("ID"), Span::raw(id)]),
        Line::from(vec![label("Encoding"), Span::raw(encoding)]),
        Line::from(vec![
            label("In placeholders"),
            Span::raw(if use_ph { "yes (gates restart)" } else { "no (loads immediately)" }.to_string()),
        ]),
        Line::from(vec![
            label("Active version"),
            Span::raw(if active.is_empty() { "—".into() } else { active }),
        ]),
        Line::from(vec![
            label("Loaded version"),
            Span::raw(if loaded_v.is_empty() { "—".into() } else { loaded_v }),
        ]),
        Line::from(vec![
            label("Loaded"),
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
    lines.push(Line::from(""));
    lines.push(Line::from(vec![label("Description")]));
    lines.push(Line::from(Span::styled(description, Style::default().fg(Color::Gray))));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Value is write-only — never shown. Enter/v: versions.",
        Style::default().fg(Color::DarkGray),
    )));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn field_str(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

// --- Version panel --------------------------------------------------------

fn draw_versions_status(f: &mut Frame, msg: &str, color: Color) {
    let body = Modal {
        title: "Secret versions",
        status: None,
        hints: &[("Esc", "close")],
        body_height: 1,
    }
    .draw(f, f.area());
    f.render_widget(
        Paragraph::new(Span::styled(msg.to_string(), Style::default().fg(color))),
        body,
    );
}

pub fn draw_versions(f: &mut Frame, app: &App) {
    use secret::VersionsView;
    let (id, versions) = match secret::versions_view(app) {
        Some(VersionsView::Loaded { id, versions, .. }) => (id, versions),
        Some(VersionsView::Loading) => {
            draw_versions_status(f, "Loading versions…", Color::DarkGray);
            return;
        }
        Some(VersionsView::Failed(e)) => {
            draw_versions_status(f, &format!("Failed to load versions: {e}"), Color::Red);
            return;
        }
        None => return,
    };
    let body = Modal {
        title: "Secret versions",
        status: Some(&id),
        hints: &[
            ("j/k", "navigate"),
            ("e/d", "enable/disable"),
            ("x", "destroy"),
            ("^N", "add version"),
            ("Esc", "close"),
        ],
        body_height: versions.len().max(1) as u16,
    }
    .draw(f, f.area());

    if versions.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled("No versions.", Style::default().fg(Color::DarkGray))),
            body,
        );
        return;
    }

    let items: Vec<ListItem> = versions
        .iter()
        .map(|v| {
            let version = v
                .get("version")
                .and_then(|x| x.as_str().map(|s| s.to_string()).or_else(|| x.as_u64().map(|n| n.to_string())))
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
                Span::styled(format!("v{version:<4} "), Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(format!("{stat:<10} "), stat_style),
                Span::styled(
                    if loaded { "loaded" } else { "" }.to_string(),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.secret.version_selected.min(versions.len().saturating_sub(1))));
    let list = List::new(items)
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, body, &mut state);
}

// --- Create form ----------------------------------------------------------

pub fn draw_create(f: &mut Frame, app: &App) {
    let Some(form) = app.secret.create.as_ref() else {
        return;
    };
    let body = Modal {
        title: "New secret",
        status: app.active_tenant().map(|t| t.name.as_str()),
        hints: &[("Tab", "next field"), ("Enter", "create"), ("Esc", "cancel")],
        body_height: 13,
    }
    .draw(f, f.area());

    let rows = Layout::vertical([
        Constraint::Length(2), // id
        Constraint::Length(2), // description
        Constraint::Length(1), // encoding
        Constraint::Length(1), // placeholders
        Constraint::Length(1), // json
        Constraint::Length(2), // value
        Constraint::Length(1), // save
        Constraint::Length(1), // error
    ])
    .split(body);

    form.id.draw(f, rows[0], form.focused == CreateField::Id);
    form.description.draw(f, rows[1], form.focused == CreateField::Description);

    // Encoding chips.
    draw_chip_row(
        f,
        rows[2],
        "Encoding",
        Encoding::ALL.iter().map(|e| (e.label(), *e == form.encoding)).collect(),
        form.focused == CreateField::Encoding,
    );
    draw_toggle_row(
        f,
        rows[3],
        "Use in placeholders",
        form.use_in_placeholders,
        form.focused == CreateField::Placeholders,
        "(off ⇒ loads immediately, no restart)",
    );
    let json_relevant = form.encoding == Encoding::Generic;
    draw_toggle_row(
        f,
        rows[4],
        "Validate as JSON",
        form.as_json,
        form.focused == CreateField::Json,
        if json_relevant { "(generic only)" } else { "(ignored for this encoding)" },
    );
    form.value.draw(f, rows[5], form.focused == CreateField::Value);
    draw_save(f, rows[6], form.focused == CreateField::Save);

    if let Some(err) = &form.error {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!("✗ {err}"),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            rows[7],
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
    let rows = Layout::vertical([Constraint::Length(2), Constraint::Length(1), Constraint::Length(1)])
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
    let mut spans = vec![Span::styled(
        format!("{label:<20} "),
        label_style(focused),
    )];
    for (text, selected) in chips {
        let style = if selected {
            Style::default().fg(Color::Black).bg(if focused { Color::Yellow } else { Color::Gray })
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
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
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
        Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    f.render_widget(
        Paragraph::new(Span::styled(" Create secret ", style)),
        area,
    );
}

fn label_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}
