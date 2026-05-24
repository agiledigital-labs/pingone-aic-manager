use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Tabs},
};

use crate::app::{App, Realm, Tab};
use crate::theme::style_for;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);
    draw_tab_row(f, app, chunks[0]);
    draw_hint_row(f, app, chunks[1]);
}

fn draw_tab_row(f: &mut Frame, app: &App, area: Rect) {
    // Split: tabs on left, chips on right
    let [left, right] = Layout::horizontal([Constraint::Min(20), Constraint::Length(30)])
        .areas(area);

    // Tabs
    let tab_titles: Vec<Line> = Tab::all()
        .iter()
        .map(|t| Line::from(t.label()))
        .collect();
    let tabs = Tabs::new(tab_titles)
        .select(Tab::all().iter().position(|t| *t == app.current_tab).unwrap_or(0))
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
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
        vec![Span::styled(" no tenant ", Style::default().fg(Color::DarkGray))]
    };

    let mut spans = vec![realm_chip];
    spans.extend(env_spans);
    let chips = Paragraph::new(Line::from(spans)).alignment(Alignment::Right);
    f.render_widget(chips, right);
}

fn draw_hint_row(f: &mut Frame, app: &App, area: Rect) {
    let _ = app;
    let mut spans: Vec<Span> = Vec::new();
    for (k, d) in [
        ("R", "realm"),
        ("T", "env"),
        ("^N", "add tenant"),
        ("^A", "auth settings"),
        ("q", "quit"),
    ] {
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
