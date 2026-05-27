//! Add-tenant flows: the picker menu and the three onboarding form
//! variants (session cookie / username+password / paste-JWK). Each one
//! is a full-screen modal that goes through `ui::modal_chrome`.

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::aic::onboard::cookie::{CookieField, CookieForm};
use crate::aic::onboard::paste::{PasteField, PasteForm};
use crate::aic::onboard::userpass::{UpField, UpForm};
use crate::app::{App, InputMode};
use crate::theme::style_for;
use crate::ui::modal_chrome::Modal;

pub fn draw(f: &mut Frame, app: &App) {
    match app.input_mode {
        InputMode::OnboardMenu => draw_menu(f, app),
        InputMode::OnboardCookie => {
            if let Some(form) = &app.onboard.cookie_form {
                draw_cookie_form(f, form);
            }
        }
        InputMode::OnboardUserPass => {
            if let Some(form) = &app.onboard.up_form {
                draw_up_form(f, form);
            }
        }
        InputMode::OnboardPaste => {
            if let Some(form) = &app.onboard.paste_form {
                draw_paste_form(f, form);
            }
        }
        _ => {}
    }
}

fn draw_menu(f: &mut Frame, app: &App) {
    let n_options = if app.has_env_creds { 4 } else { 3 };
    let body = Modal {
        title: "Add Tenant",
        status: None,
        hints: &[("Enter", "choose"), ("Esc", "cancel")],
        body_height: n_options as u16,
    }
    .draw(f, f.area());

    let mut options = vec![
        ListItem::new("  1  Paste browser session cookie  (full SSO/MFA/passkey)"),
        ListItem::new("  2  Username + password           (TOTP supported)"),
        ListItem::new("  3  Paste service-account JWK     (already have one)"),
    ];
    if app.has_env_creds {
        options.push(ListItem::new("  4  Import sandbox from environment"));
    }

    let list = List::new(options)
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    let mut state = ListState::default();
    state.select(Some(app.onboard.menu_idx));
    f.render_stateful_widget(list, body, &mut state);
}

fn draw_cookie_form(f: &mut Frame, form: &CookieForm) {
    // help(2) + 5 fields × (1 gap + 2 rows) + gap + submit(1)
    const BODY: u16 = 2 + (2 + 1) * 5 + 1 + 1;
    let body = Modal {
        title: "Add Tenant — Session Cookie",
        status: form_status_text(form.status.as_deref(), form.error.as_deref()),
        hints: form_hints(form.busy),
        body_height: BODY,
    }
    .draw(f, f.area());

    let chunks = Layout::vertical([
        Constraint::Length(2), // help
        Constraint::Length(1), // gap
        Constraint::Length(2), // name
        Constraint::Length(1),
        Constraint::Length(2), // domain
        Constraint::Length(1),
        Constraint::Length(2), // theme
        Constraint::Length(1),
        Constraint::Length(2), // cookie name
        Constraint::Length(1),
        Constraint::Length(2), // cookie value
        Constraint::Length(1),
        Constraint::Length(1), // submit
        Constraint::Min(0),
    ])
    .split(body);

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

    form.name.draw(f, chunks[2], form.focused == CookieField::Name);
    form.domain.draw(f, chunks[4], form.focused == CookieField::Domain);
    draw_theme_row(f, chunks[6], form.theme, form.focused == CookieField::Theme);
    form.cookie_name.draw(f, chunks[8], form.focused == CookieField::CookieName);
    form.cookie_value.draw(f, chunks[10], form.focused == CookieField::Cookie);
    draw_submit_row(
        f,
        chunks[12],
        if form.busy {
            "Working…"
        } else {
            "Create service account"
        },
        form.focused == CookieField::Submit,
        form.busy,
    );
}

fn draw_up_form(f: &mut Frame, form: &UpForm) {
    // help(2) + 5 fields × (1 gap + 2 rows) + gap + submit(1) + gap + otp(3)
    const BODY: u16 = 2 + (2 + 1) * 5 + 1 + 1 + 1 + 3;
    let body = Modal {
        title: "Add Tenant — Username & Password",
        status: form_status_text(form.status.as_deref(), form.error.as_deref()),
        hints: if form.pending_prompt.is_some() {
            &[
                ("Type", "the code"),
                ("Enter", "submit"),
                ("Esc", "cancel"),
            ]
        } else {
            form_hints(form.busy)
        },
        body_height: BODY,
    }
    .draw(f, f.area());

    let chunks = Layout::vertical([
        Constraint::Length(2), // help
        Constraint::Length(1),
        Constraint::Length(2), // name
        Constraint::Length(1),
        Constraint::Length(2), // domain
        Constraint::Length(1),
        Constraint::Length(2), // theme
        Constraint::Length(1),
        Constraint::Length(2), // username
        Constraint::Length(1),
        Constraint::Length(2), // password
        Constraint::Length(1),
        Constraint::Length(1), // submit
        Constraint::Length(1),
        Constraint::Length(3), // OTP prompt
        Constraint::Min(0),
    ])
    .split(body);

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Sign in as a platform admin via the root realm's default Login journey (TOTP supported).",
                Style::default().fg(Color::Gray),
            )),
            Line::from(Span::styled(
                "For passkey, push, or SSO-only flows use the session-cookie option instead.",
                Style::default().fg(Color::Gray),
            )),
        ]),
        chunks[0],
    );

    form.name.draw(f, chunks[2], form.focused == UpField::Name);
    form.domain.draw(f, chunks[4], form.focused == UpField::Domain);
    draw_theme_row(f, chunks[6], form.theme, form.focused == UpField::Theme);
    form.username.draw(f, chunks[8], form.focused == UpField::Username);
    form.password.draw(f, chunks[10], form.focused == UpField::Password);
    draw_submit_row(
        f,
        chunks[12],
        if form.busy {
            "Working…"
        } else {
            "Authenticate & create SA"
        },
        form.focused == UpField::Submit,
        form.busy,
    );

    if let Some(prompt) = &form.pending_prompt {
        let prompt_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" Additional input required ");
        let inner = prompt_block.inner(chunks[14]);
        f.render_widget(prompt_block, chunks[14]);
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
}

fn draw_paste_form(f: &mut Frame, form: &PasteForm) {
    // 4 fields × (2 + 1 gap) + jwk(8) + gap + submit(1)
    const BODY: u16 = (2 + 1) * 4 + 8 + 1 + 1;
    let body = Modal {
        title: "Add Tenant — Paste Service Account",
        status: form_status_text(None, form.error.as_deref()),
        hints: form_hints(false),
        body_height: BODY,
    }
    .draw(f, f.area());

    let chunks = Layout::vertical([
        Constraint::Length(2), // name
        Constraint::Length(1),
        Constraint::Length(2), // domain
        Constraint::Length(1),
        Constraint::Length(2), // theme
        Constraint::Length(1),
        Constraint::Length(2), // sa id
        Constraint::Length(1),
        Constraint::Min(8),    // jwk textarea
        Constraint::Length(1),
        Constraint::Length(1), // submit
    ])
    .split(body);

    form.name.draw(f, chunks[0], form.focused == PasteField::Name);
    form.domain.draw(f, chunks[2], form.focused == PasteField::Domain);
    draw_theme_row(f, chunks[4], form.theme, form.focused == PasteField::Theme);
    form.sa_id.draw(f, chunks[6], form.focused == PasteField::SaId);
    form.jwk_input.draw(f, chunks[8], form.focused == PasteField::Jwk);
    draw_submit_row(
        f,
        chunks[10],
        "Save",
        form.focused == PasteField::Submit,
        false,
    );
}

/// Status takes precedence — when a status is present we don't show the
/// (probably stale) error. Returned as `Option<&str>` so it can be wired
/// straight into the Modal chrome.
fn form_status_text<'a>(status: Option<&'a str>, error: Option<&'a str>) -> Option<&'a str> {
    status.or(error)
}

fn form_hints(busy: bool) -> &'static [(&'static str, &'static str)] {
    if busy {
        &[("Esc", "cancel")]
    } else {
        &[("Enter", "submit"), ("Esc", "go back")]
    }
}

// ---- Shared widgets ----

fn draw_theme_row(
    f: &mut Frame,
    area: Rect,
    theme: crate::config::tenant::TenantTheme,
    focused: bool,
) {
    if area.height == 0 {
        return;
    }
    let label_area = Rect { height: 1, ..area };
    f.render_widget(
        Paragraph::new(Span::styled("Theme (←/→ to cycle)", label_style(focused))),
        label_area,
    );
    if area.height < 2 {
        return;
    }
    let value_area = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: area.height - 1,
    };
    let style = style_for(theme);
    let bg = if focused {
        Color::Indexed(236)
    } else {
        Color::Indexed(234)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default().bg(bg)),
            Span::styled(
                format!(" {} {} ", style.glyph, style.label),
                Style::default().fg(style.fg).bg(style.bg),
            ),
            Span::styled("  ", Style::default().bg(bg)),
        ]))
        .style(Style::default().bg(bg)),
        value_area,
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
        // Same dark fill as an unselected input — the leading space then
        // reads as button padding instead of shifting the text right of
        // the field labels above.
        (false, false) => Style::default().fg(Color::Green).bg(Color::Indexed(234)),
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
