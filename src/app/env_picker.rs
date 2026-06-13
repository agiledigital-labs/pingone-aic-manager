use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState},
};

use crate::app::App;
use crate::tui::modal_chrome::Modal;
use crate::tui::theme::style_for;

pub fn draw(f: &mut Frame, app: &App) {
    let body = Modal {
        title: "Switch Tenant",
        status: None,
        hints: &[
            ("j/k", "navigate"),
            ("Enter", "confirm"),
            ("1-9", "switch"),
            ("Esc", "cancel"),
        ],
        body_height: app.tenants.len().max(1) as u16,
    }
    .draw(f, f.area());

    let items: Vec<ListItem> = app
        .tenants
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let style = style_for(t.theme);
            let num = if i < 9 {
                format!("{} ", i + 1)
            } else {
                "  ".to_string()
            };
            ListItem::new(Line::from(vec![
                Span::raw(num),
                Span::styled(
                    format!(" {} ", style.glyph),
                    Style::default().fg(style.fg).bg(style.bg),
                ),
                Span::raw(format!(" {}", t.name)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    let mut state = ListState::default();
    state.select(Some(app.env_picker_idx));
    f.render_stateful_widget(list, body, &mut state);
}
