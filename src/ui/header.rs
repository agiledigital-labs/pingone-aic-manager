use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Tabs},
};

use crate::app::{App, InputMode, Realm, Tab};
use crate::screens::esv::EditField;
use crate::theme::style_for;

/// Top strip: tabs + realm/tenant chips. Caller must pass a 1-row area.
pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    draw_tab_row(f, app, area);
}

/// Bottom strip: global keybind hints. Caller must pass a 1-row area.
pub fn draw_hints(f: &mut Frame, app: &App, area: Rect) {
    draw_hint_row(f, app, area);
}

fn draw_tab_row(f: &mut Frame, app: &App, area: Rect) {
    // Split: tabs on left, chips on right
    let [left, right] =
        Layout::horizontal([Constraint::Min(20), Constraint::Length(30)]).areas(area);

    // Tabs
    let tab_titles: Vec<Line> = Tab::all().iter().map(|t| Line::from(t.label())).collect();
    let tabs = Tabs::new(tab_titles)
        .select(
            Tab::all()
                .iter()
                .position(|t| *t == app.current_tab)
                .unwrap_or(0),
        )
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .divider("|");
    f.render_widget(tabs, left);

    // Realm chip + env chip
    let realm_label = match app.current_realm {
        Realm::Alpha => " alpha ",
        Realm::Bravo => " bravo ",
    };
    let realm_chip = Span::styled(
        realm_label,
        Style::default().fg(Color::Black).bg(Color::DarkGray),
    );

    let env_spans: Vec<Span> = if let Some(tenant) = app.active_tenant() {
        let s = style_for(tenant.theme);
        vec![
            Span::raw(" "),
            Span::styled(
                format!(" {} {} ", s.glyph, tenant.name),
                Style::default().fg(s.fg).bg(s.bg),
            ),
        ]
    } else {
        vec![Span::styled(
            " no tenant ",
            Style::default().fg(Color::DarkGray),
        )]
    };

    let mut spans = vec![realm_chip];
    spans.extend(env_spans);
    let chips = Paragraph::new(Line::from(spans)).alignment(Alignment::Right);
    f.render_widget(chips, right);
}

fn draw_hint_row(f: &mut Frame, app: &App, area: Rect) {
    let mut spans: Vec<Span> = Vec::new();
    match app.input_mode {
        InputMode::EsvSearch => {
            for (k, d) in [("Enter", "keep filter"), ("Esc", "clear + exit")] {
                spans.extend(hint(k, d));
            }
        }
        InputMode::EsvEdit => {
            let focused = app.esv.editing.as_ref().map(|edit| edit.focused);
            let mut hints = vec![("Tab", "navigate")];
            if let Some(enter_hint) = focused.and_then(esv_edit_enter_hint) {
                hints.push(enter_hint);
            }
            if focused == Some(EditField::Type) {
                hints.push(("←/→", "change type"));
            }
            hints.push(("Esc", "cancel"));
            for (k, d) in hints {
                spans.extend(hint(k, d));
            }
        }
        _ => {
            let mut hints: Vec<(&str, &str)> = Vec::new();
            if app.current_tab == Tab::Esvs && !app.tenants.is_empty() {
                if app
                    .active_tenant()
                    .map(|t| crate::screens::esv::can_request_restart(app, &t.name))
                    .unwrap_or(false)
                {
                    hints.push(("^S", "apply changes"));
                }
                hints.push(("/", "search"));
                let matches = app.esv_matches();
                let mut selected_deleted = false;
                if let Some(selected) =
                    matches.get(app.esv.selected.min(matches.len().saturating_sub(1)))
                {
                    if selected.deleted {
                        selected_deleted = true;
                        hints.push(("^Z", "restore"));
                    } else {
                        hints.extend([("Enter", "edit"), ("d", "delete")]);
                    }
                }
                hints.push(("^N", "new variable"));
                if !selected_deleted {
                    hints.push(("^Z", "undo"));
                }
                hints.push(("^Y", "undo history"));
            } else if app.tenants.is_empty() {
                hints.extend([("^T", "add tenant"), ("^A", "auth settings")]);
            }
            hints.push(("?", "keys"));
            for (k, d) in hints {
                spans.extend(hint(k, d));
            }
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn esv_edit_enter_hint(focused: EditField) -> Option<(&'static str, &'static str)> {
    match focused {
        EditField::Id | EditField::Description | EditField::Type => Some(("Enter", "next")),
        EditField::Value => None,
        EditField::Save => Some(("Enter", "save")),
    }
}

fn hint(key: &'static str, desc: &'static str) -> Vec<Span<'static>> {
    vec![
        Span::raw("  "),
        Span::styled(
            key,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {desc}"), Style::default().fg(Color::DarkGray)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esv_edit_enter_hint_tracks_focused_control() {
        assert_eq!(esv_edit_enter_hint(EditField::Id), Some(("Enter", "next")));
        assert_eq!(
            esv_edit_enter_hint(EditField::Description),
            Some(("Enter", "next"))
        );
        assert_eq!(
            esv_edit_enter_hint(EditField::Type),
            Some(("Enter", "next"))
        );
        assert_eq!(esv_edit_enter_hint(EditField::Value), None);
        assert_eq!(
            esv_edit_enter_hint(EditField::Save),
            Some(("Enter", "save"))
        );
    }
}
