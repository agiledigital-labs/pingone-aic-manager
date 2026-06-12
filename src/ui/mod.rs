pub mod env_picker;
pub mod header;
pub mod keybind_help;
pub mod modal;
pub mod modal_chrome;
pub mod popup_confirm;
pub mod toast;
pub mod undo_history;
pub mod widgets;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::{App, InputMode};
use crate::esv::screen::Mode as EsvMode;
use crate::secrets::{screen::Mode as SecretsMode, view as secret};

pub fn draw(f: &mut Frame, app: &App) {
    // Every modal owns the whole screen. The dashboard (Normal + ESV search)
    // is the only thing that gets the header / body / global-hints layout.
    match app.input_mode {
        InputMode::Vault(mode) => {
            crate::vault::draw(f, app, mode);
            return;
        }
        InputMode::Onboard(mode) => {
            crate::onboard::view::draw(f, app, mode);
            draw_keybind_help(f, app);
            toast::draw(f, app);
            return;
        }
        InputMode::EnvPicker => {
            env_picker::draw(f, app);
            draw_keybind_help(f, app);
            toast::draw(f, app);
            return;
        }
        InputMode::ProdConfirm => {
            modal::draw_prod_confirm(f, app);
            draw_keybind_help(f, app);
            toast::draw(f, app);
            return;
        }
        InputMode::UndoHistory => {
            undo_history::draw(f, app);
            draw_keybind_help(f, app);
            toast::draw(f, app);
            return;
        }
        InputMode::Normal | InputMode::Esv(_) | InputMode::Secrets(_) | InputMode::Scripts(_) => {}
    }

    let area = f.area();
    let show_hints = !app.keybind_help_open
        && !matches!(
            app.input_mode,
            InputMode::Esv(EsvMode::RestartConfirm | EsvMode::DeleteConfirm)
                | InputMode::Secrets(
                    SecretsMode::AddVersion
                        | SecretsMode::DeleteConfirm
                        | SecretsMode::VersionDestroyConfirm
                )
        );
    let chunks = Layout::vertical([
        Constraint::Length(1),                              // top: tabs + chips
        Constraint::Length(1),                              // breathing room under the tab row
        Constraint::Min(0),                                 // body
        Constraint::Length(if show_hints { 1 } else { 0 }), // bottom: keybind hints
    ])
    .split(area);

    header::draw(f, app, chunks[0]);
    draw_body(f, app, chunks[2]);
    if show_hints {
        header::draw_hints(f, app, chunks[3]);
    }

    // Overlay popup confirm for restart, drawn on top of the dashboard
    // (not full-screen — short y/n questions get the small popup style).
    if app.input_mode == InputMode::Esv(EsvMode::RestartConfirm) {
        let n = app
            .active_tenant()
            .map(|t| crate::esv::state::pending_count(app, &t.name))
            .unwrap_or(0);
        let noun = if n == 1 { "change" } else { "changes" };
        let message = format!(
            "{n} {noun} pending.\n\nApply by restarting the tenant runtime?\nTakes a few minutes; users already signed in stay signed in."
        );
        popup_confirm::draw(f, "Apply pending changes?", &message);
    }
    if app.input_mode == InputMode::Esv(EsvMode::DeleteConfirm) {
        let id = app
            .esv_matches()
            .get(app.esv.list.selected)
            .map(|m| m.id.clone())
            .unwrap_or_else(|| "selected variable".to_string());
        let message = format!("Delete {id}?\n\nThis can be undone from the undo log.");
        popup_confirm::draw(f, "Delete ESV variable?", &message);
    }

    // Secret overlays. The version panel now lives in the detail pane (drawn
    // by `secret::draw_body`), so these are just the create / add-version
    // forms and the two y/n confirmations — each drawn over that panel.
    match app.input_mode {
        InputMode::Secrets(SecretsMode::AddVersion) => secret::draw_add_version(f, app),
        InputMode::Secrets(SecretsMode::VersionDestroyConfirm) => {
            let version = app
                .secret
                .pending_version_destroy
                .as_ref()
                .map(|(_, _, v)| v.clone())
                .unwrap_or_default();
            let message = format!(
                "Destroy version {version}?\n\nThis is irreversible — the version's value is gone for good."
            );
            popup_confirm::draw(f, "Destroy secret version?", &message);
        }
        InputMode::Secrets(SecretsMode::DeleteConfirm) => {
            let id = app
                .secret
                .pending_delete
                .as_ref()
                .map(|p| p.id.clone())
                .unwrap_or_else(|| "selected secret".to_string());
            let message = format!(
                "Delete secret {id} and ALL its versions?\n\nThis is irreversible — secret values cannot be recovered."
            );
            popup_confirm::draw(f, "Delete secret?", &message);
        }
        _ => {}
    }

    draw_keybind_help(f, app);
    toast::draw(f, app);
}

fn draw_keybind_help(f: &mut Frame, app: &App) {
    if app.keybind_help_open {
        keybind_help::draw(f, app);
    }
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
    } else if app.current_tab == crate::app::Tab::Scripts {
        crate::scripts::view::draw_body(f, app, area);
    } else {
        crate::esv::view::draw(f, app, area);
    }
}

/// Shared `/query` + right-aligned count header for the tenant list views
/// (variables and secrets), so both halves of the ESVs tab render the search
/// row identically. `area` must be a 1-row rect.
pub(crate) fn draw_search_row(
    f: &mut Frame,
    area: Rect,
    query: &crate::ui::widgets::LineEditor,
    searching: bool,
    count_text: &str,
) {
    // Split horizontally so the count hugs the right edge regardless of the
    // query length.
    let count_width = count_text.chars().count() as u16;
    let cols =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(count_width)]).split(area);

    let query_style = Style::default().fg(if searching {
        Color::Yellow
    } else {
        Color::DarkGray
    });
    // Standard block cursor: reverse-video the char under the cursor (or a
    // single space at end-of-line). Inserting a separate cursor glyph like
    // "▏" displaces following columns in fonts that render box-drawing
    // characters double-wide.
    let cursor_style = query_style.add_modifier(Modifier::REVERSED);
    let mut spans: Vec<Span> = vec![Span::styled(" /", query_style)];
    let cursor_idx = query.cursor();
    let chars: Vec<char> = query.value().chars().collect();
    if searching {
        for (i, c) in chars.iter().enumerate() {
            let style = if i == cursor_idx {
                cursor_style
            } else {
                query_style
            };
            spans.push(Span::styled(c.to_string(), style));
        }
        if cursor_idx >= chars.len() {
            spans.push(Span::styled(" ", cursor_style));
        }
    } else {
        spans.push(Span::styled(query.value().to_string(), query_style));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), cols[0]);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            count_text.to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Right),
        cols[1],
    );
}

/// Pick the new top-of-window so `selected` stays visible. We can't compute
/// this purely from app state because the height comes from the rendered
/// rect; do it here, leave the list's `scroll` as a hint only.
pub(crate) fn clamp_scroll(prev: usize, selected: usize, height: usize, n: usize) -> usize {
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
