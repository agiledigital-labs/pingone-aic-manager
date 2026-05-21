use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::aic::onboard::cookie::{CookieField, CookieForm};
use crate::aic::onboard::paste::{PasteField, PasteForm};
use crate::aic::onboard::userpass::{UpField, UpForm};
use crate::app::{App, InputMode};
use crate::theme::Theme;
use crate::ui::modal::centered_rect;

pub fn draw(f: &mut Frame, app: &App) {
    match app.input_mode {
        InputMode::OnboardMenu => draw_menu(f, app),
        InputMode::OnboardCookie => {
            if let Some(form) = &app.cookie_form {
                draw_cookie_form(f, form);
            }
        }
        InputMode::OnboardUserPass => {
            if let Some(form) = &app.up_form {
                draw_up_form(f, form);
            }
        }
        InputMode::OnboardPaste => {
            if let Some(form) = &app.paste_form {
                draw_paste_form(f, form);
            }
        }
        _ => {}
    }
}

fn draw_menu(f: &mut Frame, app: &App) {
    let area = centered_rect(60, 50, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Add Tenant ",
            Style::default().fg(Color::Cyan),
        ));

    let mut options = vec![
        ListItem::new("  1  Paste browser session cookie  (full SSO/MFA/passkey)"),
        ListItem::new("  2  Username + password           (TOTP supported)"),
        ListItem::new("  3  Paste service-account JWK     (already have one)"),
    ];
    if app.has_envrc {
        options.push(ListItem::new("  4  Import sandbox from .envrc"));
    }

    let list = List::new(options)
        .block(block)
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    let mut state = ListState::default();
    state.select(Some(app.onboard_menu_idx));
    f.render_stateful_widget(list, area, &mut state);
}

// ---- Pattern 1: cookie ----

fn draw_cookie_form(f: &mut Frame, form: &CookieForm) {
    let area = centered_rect(80, 80, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Add Tenant — Session Cookie ",
            Style::default().fg(Color::Cyan),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Length(2),  // help
        Constraint::Length(3),  // name
        Constraint::Length(3),  // domain
        Constraint::Length(3),  // theme
        Constraint::Length(3),  // cookie name
        Constraint::Length(3),  // cookie value (single-line bordered field)
        Constraint::Length(1),  // submit
        Constraint::Length(2),  // status / error
        Constraint::Min(0),
        Constraint::Length(1),  // hint
    ])
    .split(inner);

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Log into the AIC admin console in your browser; in DevTools → Application → Cookies,",
                Style::default().fg(Color::Gray),
            )),
            Line::from(Span::styled(
                "copy the random-hex cookie name + value (NOT amlbcookie) into the fields below.",
                Style::default().fg(Color::Gray),
            )),
        ])
        .wrap(Wrap { trim: false }),
        chunks[0],
    );

    form.name.draw(f, chunks[1], form.focused == CookieField::Name);
    form.domain.draw(f, chunks[2], form.focused == CookieField::Domain);
    draw_theme_row(f, chunks[3], form.theme, form.focused == CookieField::Theme);
    form.cookie_name
        .draw(f, chunks[4], form.focused == CookieField::CookieName);
    form.cookie_value
        .draw(f, chunks[5], form.focused == CookieField::Cookie);
    draw_submit_row(
        f,
        chunks[6],
        if form.busy { "Working…" } else { "Create service account" },
        form.focused == CookieField::Submit,
        form.busy,
    );

    if let Some(status) = &form.status {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!("  {status}"),
                Style::default().fg(Color::Cyan),
            )),
            chunks[7],
        );
    } else if let Some(err) = &form.error {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!("  {err}"),
                Style::default().fg(Color::Red),
            )),
            chunks[7],
        );
    }

    f.render_widget(
        Paragraph::new(form_hint(form.busy)).style(Style::default().fg(Color::DarkGray)),
        chunks[9],
    );
}

// ---- Pattern 2: u/p ----

fn draw_up_form(f: &mut Frame, form: &UpForm) {
    let area = centered_rect(80, 80, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Add Tenant — Username & Password ",
            Style::default().fg(Color::Cyan),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Length(2), // help
        Constraint::Length(3), // name
        Constraint::Length(3), // domain
        Constraint::Length(3), // theme
        Constraint::Length(3), // username
        Constraint::Length(3), // password
        Constraint::Length(1), // submit
        Constraint::Length(2), // status/error
        Constraint::Length(3), // OTP prompt (rendered when needed)
        Constraint::Min(0),
        Constraint::Length(1), // hint
    ])
    .split(inner);

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Signs in as a platform admin via the root realm's default Login journey (TOTP supported).",
                Style::default().fg(Color::Gray),
            )),
            Line::from(Span::styled(
                "For passkey, push, or SSO-only flows use the session-cookie option instead.",
                Style::default().fg(Color::Gray),
            )),
        ]),
        chunks[0],
    );

    form.name.draw(f, chunks[1], form.focused == UpField::Name);
    form.domain.draw(f, chunks[2], form.focused == UpField::Domain);
    draw_theme_row(f, chunks[3], form.theme, form.focused == UpField::Theme);
    form.username
        .draw(f, chunks[4], form.focused == UpField::Username);
    form.password
        .draw(f, chunks[5], form.focused == UpField::Password);
    draw_submit_row(
        f,
        chunks[6],
        if form.busy {
            "Working…"
        } else {
            "Authenticate & create SA"
        },
        form.focused == UpField::Submit,
        form.busy,
    );

    if let Some(status) = &form.status {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!("  {status}"),
                Style::default().fg(Color::Cyan),
            )),
            chunks[7],
        );
    } else if let Some(err) = &form.error {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!("  {err}"),
                Style::default().fg(Color::Red),
            )),
            chunks[7],
        );
    }

    if let Some(prompt) = &form.pending_prompt {
        let prompt_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" Additional input required ");
        let inner = prompt_block.inner(chunks[8]);
        f.render_widget(prompt_block, chunks[8]);
        let masked: String = "•".repeat(form.prompt_input.chars().count());
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!(" {prompt}: "), Style::default().fg(Color::White)),
                Span::styled(
                    masked,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::UNDERLINED),
                ),
                Span::styled("▏", Style::default().fg(Color::Yellow)),
            ])),
            inner,
        );
    }

    f.render_widget(
        Paragraph::new(if form.pending_prompt.is_some() {
            "Type the code · Enter submit · Esc cancel"
        } else {
            form_hint(form.busy)
        })
        .style(Style::default().fg(Color::DarkGray)),
        chunks[10],
    );
}

// ---- Pattern 3: paste ----

fn draw_paste_form(f: &mut Frame, form: &PasteForm) {
    let area = centered_rect(80, 90, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Add Tenant — Paste Service Account ",
            Style::default().fg(Color::Cyan),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Length(3), // name
        Constraint::Length(3), // domain
        Constraint::Length(3), // theme
        Constraint::Length(3), // sa id
        Constraint::Min(8),    // jwk textarea (biggest)
        Constraint::Length(1), // submit
        Constraint::Length(2), // error
        Constraint::Length(1), // hint
    ])
    .split(inner);

    form.name.draw(f, chunks[0], form.focused == PasteField::Name);
    form.domain.draw(f, chunks[1], form.focused == PasteField::Domain);
    draw_theme_row(f, chunks[2], form.theme, form.focused == PasteField::Theme);
    form.sa_id.draw(f, chunks[3], form.focused == PasteField::SaId);
    form.jwk_input
        .draw(f, chunks[4], form.focused == PasteField::Jwk);
    draw_submit_row(
        f,
        chunks[5],
        "Save",
        form.focused == PasteField::Submit,
        false,
    );
    if let Some(err) = &form.error {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!("  {err}"),
                Style::default().fg(Color::Red),
            )),
            chunks[6],
        );
    }
    f.render_widget(
        Paragraph::new(form_hint(false)).style(Style::default().fg(Color::DarkGray)),
        chunks[7],
    );
}

// ---- Shared widgets ----

fn draw_theme_row(
    f: &mut Frame,
    area: Rect,
    theme: crate::config::tenant::TenantTheme,
    focused: bool,
) {
    let border_color = if focused { Color::Yellow } else { Color::DarkGray };
    let style = Theme::from_tenant(theme).style();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(" Theme (←/→ to cycle) ", label_style(focused)));
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {} {} ", style.glyph, style.label),
            Style::default().fg(style.fg).bg(style.bg),
        )))
        .block(block),
        area,
    );
}

fn draw_submit_row(f: &mut Frame, area: Rect, label: &str, focused: bool, busy: bool) {
    let style = match (focused, busy) {
        (_, true) => Style::default()
            .fg(Color::Black)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
        (true, false) => Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD),
        (false, false) => Style::default().fg(Color::Green),
    };
    f.render_widget(
        Paragraph::new(Span::styled(format!(" {label} "), style)).alignment(Alignment::Left),
        area,
    );
}

fn label_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    }
}

fn form_hint(busy: bool) -> &'static str {
    if busy {
        "Esc to cancel"
    } else {
        "Tab/Shift-Tab navigate · Enter submit on action · Esc go back"
    }
}
