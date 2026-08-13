use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState},
};

use crate::app::App;
use crate::tui::modal_chrome::Modal;
use crate::tui::theme::style_for;

/// Footer hints for the picker. [`DELETE_HINT`] is the load-bearing
/// advertisement for `d` / Delete — F1 help and this footer both use it.
pub const DELETE_HINT: (&str, &str) = ("d/Del", "delete");

pub const HINTS: &[(&str, &str)] = &[
    ("↑/↓", "navigate"),
    ("Enter", "confirm"),
    DELETE_HINT,
    ("1-9", "switch"),
    ("Esc", "cancel"),
];

pub fn draw(f: &mut Frame, app: &App) {
    let body = Modal {
        title: "Switch Tenant",
        status: None,
        hints: HINTS,
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

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{DELETE_HINT, HINTS};
    use crate::app::{App, InputMode, View};
    use crate::config::tenant::{Tenant, TenantTheme};

    fn tenant(name: &str) -> Tenant {
        Tenant {
            name: name.into(),
            base_url: "https://test.invalid".into(),
            theme: TenantTheme::Sandbox,
            sa_id: None,
            scopes: Vec::new(),
            provenance: crate::config::Provenance::default(),
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn d_and_delete_open_offboard_and_hints_advertise_it() {
        // Two halves, and they fail to opposite edits. Removing the
        // handle_env_picker_key arm leaves the hint in place; removing
        // the hint leaves a silent key. The handler assertion is what
        // fails if `d` is unbound.
        let mut app = App::for_test(vec![tenant("uat")], View::Esvs);
        app.input_mode = InputMode::EnvPicker;
        app.handle_env_picker_key(key(KeyCode::Char('d')));
        assert!(
            matches!(app.input_mode, InputMode::Offboard(_)),
            "d must open the offboard modal, got {:?}",
            app.input_mode
        );

        let mut app = App::for_test(vec![tenant("uat")], View::Esvs);
        app.input_mode = InputMode::EnvPicker;
        app.handle_env_picker_key(key(KeyCode::Delete));
        assert!(
            matches!(app.input_mode, InputMode::Offboard(_)),
            "Delete must open the offboard modal, got {:?}",
            app.input_mode
        );

        assert!(
            HINTS.contains(&DELETE_HINT),
            "picker footer must advertise delete: {HINTS:?}"
        );
        assert!(
            DELETE_HINT.0.contains('d'),
            "advertised delete key must mention d: {}",
            DELETE_HINT.0
        );
    }

    #[test]
    fn d_on_empty_tenant_list_is_a_noop() {
        let mut app = App::for_test(vec![], View::Esvs);
        app.input_mode = InputMode::EnvPicker;
        app.handle_env_picker_key(key(KeyCode::Char('d')));
        assert_eq!(app.input_mode, InputMode::EnvPicker);
        assert!(app.offboard.form.is_none());

        app.handle_env_picker_key(key(KeyCode::Delete));
        assert_eq!(app.input_mode, InputMode::EnvPicker);
        assert!(app.offboard.form.is_none());
    }
}
