//! Setup-auth modal. First-run shows the method picker + the conditional
//! field block; Auth-Settings "add factor" reuses the same screen with the
//! picker collapsed. Goes through the shared modal chrome for title + hints.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::app::{App, AuthMethod, AuthSetupField, SetupContext};
use crate::security_key;
use crate::ui::modal_chrome::Modal;
use crate::ui::widgets::secret_field;

const BG_UNFOCUSED: Color = Color::Indexed(234);
const BG_FOCUSED: Color = Color::Indexed(236);

pub fn draw(f: &mut Frame, app: &App) {
    let first_run = app.auth_setup.context == SetupContext::FirstRun;
    let title = if first_run {
        "Set up authentication"
    } else {
        match app.auth_setup.form.method {
            AuthMethod::None => "Add factor",
            AuthMethod::Password => "Change password",
            AuthMethod::SecurityKey => "Add security key",
        }
    };

    let hints: &[(&str, &str)] = if app.auth_setup.form.busy {
        &[]
    } else if first_run {
        &[
            ("Tab", "navigate"),
            ("←/→", "change method"),
            ("Enter", "submit"),
            ("Esc", "quit"),
        ]
    } else {
        &[
            ("Tab", "navigate"),
            ("Enter", "submit"),
            ("Esc", "cancel"),
        ]
    };

    // The method picker only renders on first-run. When the user pressed
    // `p`/`s` in auth_settings the method is already known.
    let show_method = first_run;
    let method_label_h = if show_method { 1 } else { 0 };
    let method_value_h = if show_method { 1 } else { 0 };
    let pre_body_gap_h = if show_method { 1 } else { 0 };
    let body_height = method_label_h + method_value_h + pre_body_gap_h + 7 + 1 + 1 + 2;

    let body = Modal {
        title,
        status: None,
        hints,
        body_height,
    }
    .draw(f, f.area());

    let chunks = Layout::vertical([
        Constraint::Length(method_label_h),
        Constraint::Length(method_value_h),
        Constraint::Length(pre_body_gap_h),
        Constraint::Length(7), // field block
        Constraint::Length(1), // submit
        Constraint::Length(1), // gap
        Constraint::Min(2),    // error — absorbs leftover so long CTAP
                               // messages don't overflow
    ])
    .split(body);

    if show_method {
        draw_radio(f, app, chunks[0], chunks[1]);
    }

    match app.auth_setup.form.method {
        AuthMethod::None => draw_none_body(f, chunks[3]),
        AuthMethod::Password => draw_password_body(f, app, chunks[3]),
        AuthMethod::SecurityKey => draw_security_key_body(f, app, chunks[3]),
    }

    draw_submit(f, app, chunks[4]);

    if let Some(err) = &app.auth_setup.form.error {
        f.render_widget(
            Paragraph::new(Span::styled(err.as_str(), Style::default().fg(Color::Red)))
                .wrap(Wrap { trim: false }),
            chunks[6],
        );
    }
}

fn draw_radio(f: &mut Frame, app: &App, label_area: Rect, value_area: Rect) {
    let focused = app.auth_setup.form.focused == AuthSetupField::Method;
    f.render_widget(
        Paragraph::new(Span::styled(
            "Protect credentials at rest with",
            label_style(focused),
        )),
        label_area,
    );
    let bg = if focused { BG_FOCUSED } else { BG_UNFOCUSED };
    let mut line: Vec<Span> = vec![Span::styled(" ", Style::default().bg(bg))];
    for m in AuthMethod::ORDER.iter() {
        line.push(chip(m.label(), *m == app.auth_setup.form.method, focused));
        line.push(Span::styled("  ", Style::default().bg(bg)));
    }
    f.render_widget(
        Paragraph::new(Line::from(line)).style(Style::default().bg(bg)),
        value_area,
    );
}

fn chip<'a>(label: &'a str, selected: bool, parent_focused: bool) -> Span<'a> {
    let style = match (selected, parent_focused) {
        (true, true) => Style::default().fg(Color::Black).bg(Color::Yellow),
        (true, false) => Style::default().fg(Color::Black).bg(Color::Gray),
        _ => Style::default().fg(Color::Gray).bg(BG_FOCUSED),
    };
    let glyph = if selected { "●" } else { "○" };
    Span::styled(format!(" {glyph} {label} "), style)
}

fn draw_none_body(f: &mut Frame, area: Rect) {
    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Credentials will be stored at .aic-edit/keys.plain (mode 600).",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            "Filesystem permissions are the only protection.",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "You can add a factor later from the Auth Settings screen.",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    f.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
}

fn draw_password_body(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(2),
    ])
    .split(area);

    secret_field::draw(
        f,
        chunks[0],
        "Master password",
        &app.auth_setup.form.password,
        app.auth_setup.form.focused == AuthSetupField::Password,
        None,
    );
    secret_field::draw(
        f,
        chunks[2],
        "Confirm",
        &app.auth_setup.form.confirm,
        app.auth_setup.form.focused == AuthSetupField::Confirm,
        None,
    );
}

fn draw_security_key_body(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(secret_field::HEIGHT),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    secret_field::draw(
        f,
        chunks[0],
        "Security key PIN",
        &app.auth_setup.form.pin,
        app.auth_setup.form.focused == AuthSetupField::Pin,
        if app.auth_setup.form.busy {
            Some(security_key::TAP_MESSAGE)
        } else {
            None
        },
    );
    draw_text_field(
        f,
        chunks[2],
        chunks[3],
        "Label",
        &app.auth_setup.form.label,
        app.auth_setup.form.focused == AuthSetupField::Label,
    );
}

fn draw_text_field(
    f: &mut Frame,
    label_area: Rect,
    value_area: Rect,
    label: &str,
    value: &str,
    focused: bool,
) {
    f.render_widget(
        Paragraph::new(Span::styled(label.to_string(), label_style(focused))),
        label_area,
    );
    let bg = if focused { BG_FOCUSED } else { BG_UNFOCUSED };
    let cursor = if focused { "▏" } else { " " };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default().bg(bg)),
            Span::styled(value.to_string(), Style::default().fg(Color::White).bg(bg)),
            Span::styled(cursor, Style::default().fg(Color::Yellow).bg(bg)),
        ]))
        .style(Style::default().bg(bg)),
        value_area,
    );
}

fn draw_submit(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.auth_setup.form.focused == AuthSetupField::Submit;
    let style = if focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        // Same dark fill as an unselected input — the leading space then
        // reads as the button's left padding instead of shifting the text
        // right of the field labels above.
        Style::default().fg(Color::Green).bg(BG_UNFOCUSED)
    };
    let label = match app.auth_setup.form.method {
        AuthMethod::None => " Continue without encryption ",
        AuthMethod::Password => " Set password and continue ",
        AuthMethod::SecurityKey => " Enrol security key and continue ",
    };
    f.render_widget(Paragraph::new(Span::styled(label, style)), area);
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
