use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
};

use crate::app::{App, InputMode, View};
use crate::tui::modal_chrome::Modal;

pub struct State {
    pub query: String,
    pub filtered: Vec<View>,
    pub highlighted: usize,
}

impl State {
    pub fn new(active_view: View) -> Self {
        Self {
            query: String::new(),
            filtered: View::all().to_vec(),
            highlighted: View::all()
                .iter()
                .position(|view| *view == active_view)
                .unwrap_or(0),
        }
    }
}

pub fn open(app: &mut App) {
    app.selector.query.clear();
    app.selector.filtered = rank_views("");
    app.selector.highlighted = app
        .selector
        .filtered
        .iter()
        .position(|view| *view == app.active_view)
        .unwrap_or(0);
    app.input_mode = InputMode::Selector;
}

pub fn draw(f: &mut Frame, app: &App) {
    let body = Modal {
        title: "Go to function",
        status: None,
        hints: &[("↑/↓", "navigate"), ("Enter", "open"), ("Esc", "cancel")],
        body_height: app.selector.filtered.len().max(1) as u16 + 2,
    }
    .draw(f, f.area());

    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(body);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Search  ", Style::default().fg(Color::DarkGray)),
            Span::styled(&app.selector.query, Style::default().fg(Color::White)),
            Span::styled("█", Style::default().fg(Color::Cyan)),
        ])),
        chunks[0],
    );

    let items: Vec<ListItem> = if app.selector.filtered.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No matching functions",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        app.selector
            .filtered
            .iter()
            .map(|view| ListItem::new(Line::from(view.label())))
            .collect()
    };
    let list = List::new(items).highlight_style(
        Style::default()
            .fg(Color::White)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );
    let mut state = ListState::default();
    if !app.selector.filtered.is_empty() {
        state.select(Some(app.selector.highlighted));
    }
    f.render_stateful_widget(list, chunks[2], &mut state);
}

pub fn handle_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.input_mode = InputMode::Normal,
        KeyCode::Up if app.selector.highlighted > 0 => {
            app.selector.highlighted -= 1;
        }
        KeyCode::Down if app.selector.highlighted + 1 < app.selector.filtered.len() => {
            app.selector.highlighted += 1;
        }
        KeyCode::Enter => {
            let Some(view) = app.selector.filtered.get(app.selector.highlighted).copied() else {
                return;
            };
            app.active_view = view;
            app.input_mode = InputMode::Normal;
            crate::app::refresh_view(app, view, false);
        }
        KeyCode::Backspace if app.selector.query.pop().is_some() => {
            rerank(app);
        }
        KeyCode::Char(c)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            app.selector.query.push(c);
            rerank(app);
        }
        _ => {}
    }
}

fn rerank(app: &mut App) {
    app.selector.filtered = rank_views(&app.selector.query);
    app.selector.highlighted = 0;
}

fn rank_views(query: &str) -> Vec<View> {
    if query.is_empty() {
        return View::all().to_vec();
    }

    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let mut buf = Vec::new();
    let mut positions = Vec::new();
    let mut ranked = Vec::new();

    for view in View::all() {
        positions.clear();
        let label = view.label();
        let haystack = Utf32Str::new(label, &mut buf);
        if let Some(score) = pattern.indices(haystack, &mut matcher, &mut positions) {
            ranked.push((*view, score));
        }
    }

    ranked.sort_by(|(a_view, a_score), (b_view, b_score)| {
        b_score
            .cmp(a_score)
            .then_with(|| a_view.label().cmp(b_view.label()))
    });
    ranked.into_iter().map(|(view, _)| view).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_preserves_view_order() {
        assert_eq!(rank_views(""), View::all());
    }

    #[test]
    fn fuzzy_query_filters_and_ranks_labels() {
        assert_eq!(rank_views("scripts"), vec![View::Scripts]);
        assert_eq!(rank_views("ma"), vec![View::Managed, View::Mappings]);
    }
}
