use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap},
    Frame,
};

use crate::aic::onboard::cookie::{CookieField, CookieForm};
use crate::aic::onboard::paste::{PasteField, PasteForm};
use crate::aic::onboard::userpass::{UpField, UpForm};
use crate::app::{App, InputMode};
use crate::theme::style_for;
use crate::ui::modal::centered_rect;

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
    if app.has_env_creds {
        options.push(ListItem::new("  4  Import sandbox from environment"));
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
    state.select(Some(app.onboard.menu_idx));
    f.render_stateful_widget(list, area, &mut state);
}

// ---- Pattern 1: cookie ----

fn draw_cookie_form(f: &mut Frame, form: &CookieForm) {
    let area = centered_rect(80, 80, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .padding(form_padding())
        .title(Span::styled(
            " Add Tenant — Session Cookie ",
            Style::default().fg(Color::Cyan),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Field heights: 2 rows each (label + value). A 1-row gap separates
    // every field so the form breathes; the final spacer absorbs slack.
    let chunks = Layout::vertical([
        Constraint::Length(2), // help
        Constraint::Length(1), // gap
        Constraint::Length(2), // name
        Constraint::Length(1), // gap
        Constraint::Length(2), // domain
        Constraint::Length(1), // gap
        Constraint::Length(2), // theme
        Constraint::Length(1), // gap
        Constraint::Length(2), // cookie name
        Constraint::Length(1), // gap
        Constraint::Length(2), // cookie value
        Constraint::Length(1), // gap
        Constraint::Length(1), // submit
        Constraint::Length(1), // gap
        Constraint::Length(2), // status / error
        Constraint::Min(0),
        Constraint::Length(1), // hint
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

    form.name
        .draw(f, chunks[2], form.focused == CookieField::Name);
    form.domain
        .draw(f, chunks[4], form.focused == CookieField::Domain);
    draw_theme_row(f, chunks[6], form.theme, form.focused == CookieField::Theme);
    form.cookie_name
        .draw(f, chunks[8], form.focused == CookieField::CookieName);
    form.cookie_value
        .draw(f, chunks[10], form.focused == CookieField::Cookie);
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

    if let Some(status) = &form.status {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!("  {status}"),
                Style::default().fg(Color::Cyan),
            )),
            chunks[14],
        );
    } else if let Some(err) = &form.error {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!("  {err}"),
                Style::default().fg(Color::Red),
            )),
            chunks[14],
        );
    }

    f.render_widget(
        Paragraph::new(form_hint(form.busy)).style(Style::default().fg(Color::DarkGray)),
        chunks[16],
    );
}

// ---- Pattern 2: u/p ----

fn draw_up_form(f: &mut Frame, form: &UpForm) {
    let area = centered_rect(80, 80, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .padding(form_padding())
        .title(Span::styled(
            " Add Tenant — Username & Password ",
            Style::default().fg(Color::Cyan),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Length(2), // help
        Constraint::Length(1), // gap
        Constraint::Length(2), // name
        Constraint::Length(1), // gap
        Constraint::Length(2), // domain
        Constraint::Length(1), // gap
        Constraint::Length(2), // theme
        Constraint::Length(1), // gap
        Constraint::Length(2), // username
        Constraint::Length(1), // gap
        Constraint::Length(2), // password
        Constraint::Length(1), // gap
        Constraint::Length(1), // submit
        Constraint::Length(1), // gap
        Constraint::Length(2), // status/error
        Constraint::Length(3), // OTP prompt (rendered when needed)
        Constraint::Min(0),
        Constraint::Length(1), // hint
    ])
    .split(inner);

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
    form.domain
        .draw(f, chunks[4], form.focused == UpField::Domain);
    draw_theme_row(f, chunks[6], form.theme, form.focused == UpField::Theme);
    form.username
        .draw(f, chunks[8], form.focused == UpField::Username);
    form.password
        .draw(f, chunks[10], form.focused == UpField::Password);
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

    if let Some(status) = &form.status {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!("  {status}"),
                Style::default().fg(Color::Cyan),
            )),
            chunks[14],
        );
    } else if let Some(err) = &form.error {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!("  {err}"),
                Style::default().fg(Color::Red),
            )),
            chunks[14],
        );
    }

    if let Some(prompt) = &form.pending_prompt {
        let prompt_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" Additional input required ");
        let inner = prompt_block.inner(chunks[15]);
        f.render_widget(prompt_block, chunks[15]);
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
        chunks[17],
    );
}

// ---- Pattern 3: paste ----

fn draw_paste_form(f: &mut Frame, form: &PasteForm) {
    let area = centered_rect(80, 90, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .padding(form_padding())
        .title(Span::styled(
            " Add Tenant — Paste Service Account ",
            Style::default().fg(Color::Cyan),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Length(2), // name
        Constraint::Length(1), // gap
        Constraint::Length(2), // domain
        Constraint::Length(1), // gap
        Constraint::Length(2), // theme
        Constraint::Length(1), // gap
        Constraint::Length(2), // sa id
        Constraint::Length(1), // gap
        Constraint::Min(8),    // jwk textarea (biggest)
        Constraint::Length(1), // gap
        Constraint::Length(1), // submit
        Constraint::Length(1), // gap
        Constraint::Length(2), // error
        Constraint::Length(1), // hint
    ])
    .split(inner);

    form.name
        .draw(f, chunks[0], form.focused == PasteField::Name);
    form.domain
        .draw(f, chunks[2], form.focused == PasteField::Domain);
    draw_theme_row(f, chunks[4], form.theme, form.focused == PasteField::Theme);
    form.sa_id
        .draw(f, chunks[6], form.focused == PasteField::SaId);
    form.jwk_input
        .draw(f, chunks[8], form.focused == PasteField::Jwk);
    draw_submit_row(
        f,
        chunks[10],
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
            chunks[12],
        );
    }
    f.render_widget(
        Paragraph::new(form_hint(false)).style(Style::default().fg(Color::DarkGray)),
        chunks[13],
    );
}

/// Two-space horizontal padding + a one-row top/bottom margin around the
/// inner area of an onboarding form's outer block.
fn form_padding() -> Padding {
    Padding::new(2, 2, 1, 1)
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
    let label_area = ratatui::layout::Rect { height: 1, ..area };
    f.render_widget(
        Paragraph::new(Span::styled("Theme (←/→ to cycle)", label_style(focused))),
        label_area,
    );
    if area.height < 2 {
        return;
    }
    let value_area = ratatui::layout::Rect {
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
