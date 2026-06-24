use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::{App, Realm};
use crate::tui::theme::style_for;

/// Top strip: active view + realm/tenant chips. Caller must pass a 1-row area.
pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    draw_header_row(f, app, area);
}

/// Bottom strip: global keybind hints. Caller must pass a 1-row area.
pub fn draw_hints(f: &mut Frame, app: &App, area: Rect) {
    draw_hint_row(f, app, area);
}

fn draw_header_row(f: &mut Frame, app: &App, area: Rect) {
    let [left, right] =
        Layout::horizontal([Constraint::Min(20), Constraint::Length(30)]).areas(area);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                app.active_view.label(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Ctrl-P functions", Style::default().fg(Color::DarkGray)),
        ])),
        left,
    );

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
    // Every mode's footer comes from one place — see `keymap::footer_hints`.
    let mut spans: Vec<Span> = Vec::new();
    for (k, d) in crate::app::keymap::footer_hints(app) {
        spans.extend(hint(k, d));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
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
