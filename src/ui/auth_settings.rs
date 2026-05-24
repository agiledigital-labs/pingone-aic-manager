//! Auth Settings — list of currently enrolled factors plus footer actions.
//! Reached from Normal mode via Ctrl-A.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap as TextWrap},
};

use crate::app::App;
use crate::config::wraps::Wrap;
use crate::ui::modal::fixed_rect;

pub fn draw(f: &mut Frame, app: &App) {
    let area = fixed_rect(90, 18, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Auth Settings ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Length(1), // status line
        Constraint::Length(1), // gap
        Constraint::Min(3),    // factor list
        Constraint::Length(1), // gap
        Constraint::Length(2), // hint
    ])
    .split(inner);

    draw_status(f, app, chunks[0]);
    draw_factor_list(f, app, chunks[2]);
    draw_hints(f, app, chunks[4]);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let n = app.wraps.wraps.len();
    let encrypted = app
        .settings
        .as_ref()
        .map(|s| s.encrypt_keys)
        .unwrap_or(false);
    let (label, color) = if encrypted {
        (
            format!("Status: encrypted · {n} factor(s)"),
            Color::Green,
        )
    } else {
        (
            "Status: NOT encrypted · credentials at keys.plain".to_string(),
            Color::Yellow,
        )
    };
    f.render_widget(
        Paragraph::new(Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        area,
    );
}

fn draw_factor_list(f: &mut Frame, app: &App, area: Rect) {
    if app.wraps.wraps.is_empty() {
        let msg = if app
            .settings
            .as_ref()
            .map(|s| s.encrypt_keys)
            .unwrap_or(false)
        {
            "  (encryption is on but no factors are enrolled — inconsistent state!)"
        } else {
            "  No factors enrolled. Press [p] for a master password or [s] for a security key."
        };
        f.render_widget(
            Paragraph::new(Span::styled(msg, Style::default().fg(Color::Gray)))
                .wrap(TextWrap { trim: true }),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .wraps
        .wraps
        .iter()
        .map(|w| {
            let (icon, label) = match w {
                Wrap::Password { .. } => ("🔒", "Master password".to_string()),
                Wrap::SecurityKey { label, .. } => (
                    "🔑",
                    label.clone().unwrap_or_else(|| "Security key".into()),
                ),
            };
            ListItem::new(Line::from(vec![
                Span::raw(format!(" {icon}  ")),
                Span::styled(label, Style::default().fg(Color::White)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    let mut state = ListState::default();
    state.select(Some(app.auth_settings_idx));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_hints(f: &mut Frame, app: &App, area: Rect) {
    let n = app.wraps.wraps.len();
    let has_password = app
        .wraps
        .wraps
        .iter()
        .any(|w| matches!(w, Wrap::Password { .. }));
    let mut hints: Vec<Span> = Vec::new();
    hints.extend(hint(
        "p",
        if has_password {
            "change password"
        } else {
            "set password"
        },
    ));
    hints.extend(hint("s", "add security key"));
    if n > 0 {
        // [d] on the last remaining factor already falls through to the
        // disable-encryption confirm, so a separate [x] is redundant.
        hints.extend(hint("d", "remove"));
        // Rename only makes sense for security-key wraps (the password row's
        // label is always "Master password").
        if let Some(Wrap::SecurityKey { .. }) = app.wraps.wraps.get(app.auth_settings_idx) {
            hints.extend(hint("r", "rename"));
        }
    }
    hints.extend(hint("Esc", "close"));
    f.render_widget(Paragraph::new(Line::from(hints)), area);
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

pub fn draw_rename(f: &mut Frame, app: &App) {
    let area = fixed_rect(60, 8, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Rename security key ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Length(1), // spacer
        Constraint::Length(1), // label
        Constraint::Length(1), // value
        Constraint::Length(1), // spacer
        Constraint::Length(1), // hint
    ])
    .split(inner);

    f.render_widget(
        Paragraph::new(Span::styled(
            "Label",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        chunks[1],
    );

    // Value row: dark-backed text input.
    let bg = Color::Indexed(236);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default().bg(bg)),
            Span::styled(
                app.rename_input.clone(),
                Style::default().fg(Color::White).bg(bg),
            ),
            Span::styled("▏", Style::default().fg(Color::Yellow).bg(bg)),
        ]))
        .style(Style::default().bg(bg)),
        chunks[2],
    );

    let mut hints: Vec<Span> = Vec::new();
    hints.extend(hint("Enter", "save"));
    hints.extend(hint("Esc", "cancel"));
    f.render_widget(Paragraph::new(Line::from(hints)), chunks[4]);
}

pub fn draw_confirm(f: &mut Frame, app: &App) {
    let area = fixed_rect(60, 8, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            " ⚠ Confirm ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Layout mirrors the Auth Settings parent (single-row hint with a 1-row
    // blank below before the bottom border). 6 inner rows total:
    //   row 0  blank
    //   row 1  prompt
    //   rows 2-3  blank (2 lines)
    //   row 4  hint
    //   row 5  blank
    let chunks = Layout::vertical([
        Constraint::Length(1), // top spacer
        Constraint::Length(1), // prompt
        Constraint::Length(2), // 2 blank lines
        Constraint::Length(2), // hint + trailing blank
    ])
    .split(inner);

    let prompt = app
        .pending_auth_action_label()
        .unwrap_or_else(|| "Confirm?".into());
    f.render_widget(
        Paragraph::new(Span::styled(
            format!("  {prompt}"),
            Style::default().fg(Color::White),
        ))
        .wrap(TextWrap { trim: false }),
        chunks[1],
    );

    let mut hints: Vec<Span> = Vec::new();
    hints.extend(hint("y", "yes"));
    hints.extend(hint("n/Esc", "cancel"));
    f.render_widget(Paragraph::new(Line::from(hints)), chunks[3]);
}
