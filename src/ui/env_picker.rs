use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
};

use crate::app::App;
use crate::theme::Theme;
use crate::ui::modal::centered_rect;

pub fn draw(f: &mut Frame, app: &App) {
    let area = centered_rect(50, 60, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(" Switch Tenant  (j/k select  Enter confirm  Esc cancel) ", Style::default().fg(Color::Cyan)));

    let items: Vec<ListItem> = app
        .tenants
        .iter()
        .map(|t| {
            let style = Theme::from_tenant(t.theme).style();
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", style.glyph),
                    Style::default().fg(style.fg).bg(style.bg),
                ),
                Span::raw(format!(" {}", t.name)),
            ]))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.env_picker_idx));

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, &mut state);
}
