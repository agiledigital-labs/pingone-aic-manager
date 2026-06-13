//! Auth Settings — full-screen modal listing the enrolled factors plus the
//! action keybinds at the bottom. Renamed factors and the destructive y/n
//! confirm both render through the same modal chrome.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
};

use crate::app::App;
use crate::config::wraps::Wrap;
use crate::tui::modal_chrome::Modal;

pub fn draw(f: &mut Frame, app: &App) {
    let n = app.wraps.wraps.len();
    let encrypted = app
        .settings
        .as_ref()
        .map(|s| s.encrypt_keys)
        .unwrap_or(false);
    let status = if encrypted {
        format!("Status: encrypted · {n} factor(s)")
    } else {
        "Status: NOT encrypted · credentials at keys.plain".to_string()
    };

    let has_password = app
        .wraps
        .wraps
        .iter()
        .any(|w| matches!(w, Wrap::Password { .. }));
    let selected_is_security_key = matches!(
        app.wraps.wraps.get(app.auth_settings.idx),
        Some(Wrap::SecurityKey { .. })
    );

    let mut hints: Vec<(&str, &str)> = Vec::new();
    hints.push((
        "p",
        if has_password {
            "change password"
        } else {
            "set password"
        },
    ));
    hints.push(("s", "add security key"));
    if n > 0 {
        // `d` on the last remaining factor already falls through to the
        // disable-encryption confirm, so a separate [x] is redundant.
        hints.push(("d", "remove"));
        hints.push((
            "Enter",
            if selected_is_security_key {
                "rename"
            } else {
                "edit"
            },
        ));
    }
    hints.push(("Esc", "close"));

    // Body is one row per factor (or one row for the empty-state message).
    let body_height = app.wraps.wraps.len().max(1) as u16;
    let body = Modal {
        title: "Auth Settings",
        status: Some(&status),
        hints: &hints,
        body_height,
    }
    .draw(f, f.area());

    draw_factor_list(f, app, body);
}

fn draw_factor_list(f: &mut Frame, app: &App, area: Rect) {
    if app.wraps.wraps.is_empty() {
        let msg = if app
            .settings
            .as_ref()
            .map(|s| s.encrypt_keys)
            .unwrap_or(false)
        {
            "(encryption is on but no factors are enrolled — inconsistent state!)"
        } else {
            "No factors enrolled."
        };
        f.render_widget(
            Paragraph::new(Span::styled(msg, Style::default().fg(Color::Gray))),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .wraps
        .wraps
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let (icon, label) = match w {
                Wrap::Password { .. } => ("🔒", "Master password".to_string()),
                Wrap::SecurityKey { label, .. } => {
                    ("🔑", label.clone().unwrap_or_else(|| "Security key".into()))
                }
            };
            // Same shape as the Add Tenant menu: " N  glyph  label". 1-9
            // get a number that doubles as a hotkey; rows past 9 keep the
            // gutter so columns line up but lose the digit.
            let num = if i < 9 {
                format!("{} ", i + 1)
            } else {
                "  ".to_string()
            };
            ListItem::new(Line::from(vec![
                Span::raw(num),
                Span::raw(format!(" {icon}  ")),
                Span::raw(label),
            ]))
        })
        .collect();

    // Match the Add Tenant menu: ▶ glyph + yellow-bold text, no bg bar.
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    let mut state = ListState::default();
    state.select(Some(app.auth_settings.idx));
    f.render_stateful_widget(list, area, &mut state);
}

pub fn draw_rename(f: &mut Frame, app: &App) {
    let body = Modal {
        title: "Rename security key",
        status: None,
        hints: &[("Enter", "save"), ("Esc", "cancel")],
        body_height: 2, // label row + value row
    }
    .draw(f, f.area());

    let chunks = Layout::vertical([
        Constraint::Length(1), // label
        Constraint::Length(1), // value
        Constraint::Min(0),
    ])
    .split(body);

    f.render_widget(
        Paragraph::new(Span::styled(
            "Label",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        chunks[0],
    );

    let bg = Color::Indexed(236);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default().bg(bg)),
            Span::styled(
                app.auth_settings.rename_input.clone(),
                Style::default().fg(Color::White).bg(bg),
            ),
            Span::styled("▏", Style::default().fg(Color::Yellow).bg(bg)),
        ]))
        .style(Style::default().bg(bg)),
        chunks[1],
    );
}

pub fn draw_confirm(f: &mut Frame, app: &App) {
    let prompt = app
        .auth_settings
        .pending_action_label(&app.wraps)
        .unwrap_or_else(|| "Confirm?".into());
    let body = Modal {
        title: "⚠ Confirm",
        status: None,
        hints: &[("y", "yes"), ("n/Esc", "cancel")],
        body_height: 1,
    }
    .draw(f, f.area());
    f.render_widget(
        Paragraph::new(Span::styled(prompt, Style::default().fg(Color::White))),
        body,
    );
}
