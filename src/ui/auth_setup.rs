//! First-run auth-setup form. Centered on an otherwise-empty screen — no
//! outer frame, no intro paragraph; just the method picker, the conditional
//! credential fields, and a submit button.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::app::{App, AuthMethod, AuthSetupField};
use crate::security_key;
use crate::ui::widgets::secret_field;

const BG_UNFOCUSED: Color = Color::Indexed(234);
const BG_FOCUSED: Color = Color::Indexed(236);

pub fn draw(f: &mut Frame, app: &App) {
    let area = centered_form(f.area(), 64, 20);

    let chunks = Layout::vertical([
        Constraint::Length(1), // method label
        Constraint::Length(1), // radio row
        Constraint::Length(1), // gap
        Constraint::Length(7), // conditional field block
        Constraint::Length(1), // submit
        Constraint::Length(1), // gap
        Constraint::Min(2),    // error — grows to absorb any leftover height,
                               // so long ctap-hid messages don't overflow.
        Constraint::Length(1), // hint
    ])
    .split(area);

    draw_radio(f, app, chunks[0], chunks[1]);

    match app.setup_form.method {
        AuthMethod::None => draw_none_body(f, chunks[3]),
        AuthMethod::Password => draw_password_body(f, app, chunks[3]),
        AuthMethod::SecurityKey => draw_security_key_body(f, app, chunks[3]),
    }

    draw_submit(f, app, chunks[4]);

    if let Some(err) = &app.setup_form.error {
        f.render_widget(
            Paragraph::new(Span::styled(err.as_str(), Style::default().fg(Color::Red)))
                .wrap(Wrap { trim: false }),
            chunks[6],
        );
    }

    let hint = if app.setup_form.busy {
        "Working…"
    } else {
        "Tab/Shift-Tab navigate · ←/→ change method · Enter submit · Esc quit"
    };
    f.render_widget(
        Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
        chunks[7],
    );
}

fn draw_radio(f: &mut Frame, app: &App, label_area: Rect, value_area: Rect) {
    let focused = app.setup_form.focused == AuthSetupField::Method;
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
        line.push(chip(m.label(), *m == app.setup_form.method, focused));
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
        Constraint::Length(2), // password label + value
        Constraint::Length(1), // gap
        Constraint::Length(2), // confirm label + value
    ])
    .split(area);

    secret_field::draw(
        f,
        chunks[0],
        "Master password",
        &app.setup_form.password,
        app.setup_form.focused == AuthSetupField::Password,
        None,
    );
    secret_field::draw(
        f,
        chunks[2],
        "Confirm",
        &app.setup_form.confirm,
        app.setup_form.focused == AuthSetupField::Confirm,
        None,
    );
}

fn draw_security_key_body(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(secret_field::HEIGHT), // PIN field (label + value + status)
        Constraint::Length(1),                    // gap
        Constraint::Length(1),                    // label label
        Constraint::Length(1),                    // label value
    ])
    .split(area);

    secret_field::draw(
        f,
        chunks[0],
        "Security key PIN",
        &app.setup_form.pin,
        app.setup_form.focused == AuthSetupField::Pin,
        if app.setup_form.busy {
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
        &app.setup_form.label,
        app.setup_form.focused == AuthSetupField::Label,
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
    let label_style = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    f.render_widget(
        Paragraph::new(Span::styled(label.to_string(), label_style)),
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
    let focused = app.setup_form.focused == AuthSetupField::Submit;
    let style = if focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let label = match app.setup_form.method {
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

fn centered_form(parent: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(parent.width);
    let h = height.min(parent.height);
    Rect {
        x: parent.x + (parent.width.saturating_sub(w)) / 2,
        y: parent.y + (parent.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}
