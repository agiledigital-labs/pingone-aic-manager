//! First-run "set a master password" form. Centered on an otherwise-empty
//! screen — no outer frame, no intro paragraph; just the choice toggle, the
//! password + confirm fields, and a submit button.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::app::{App, MpField};

const BG_UNFOCUSED: Color = Color::Indexed(234);
const BG_FOCUSED: Color = Color::Indexed(236);

pub fn draw(f: &mut Frame, app: &App) {
    let area = centered_form(f.area(), 60, 14);

    let chunks = Layout::vertical([
        Constraint::Length(1), // choice label
        Constraint::Length(1), // choice value
        Constraint::Length(1), // gap
        Constraint::Length(1), // password label
        Constraint::Length(1), // password value
        Constraint::Length(1), // confirm label
        Constraint::Length(1), // confirm value
        Constraint::Length(1), // gap
        Constraint::Length(1), // submit
        Constraint::Length(1), // gap
        Constraint::Length(2), // error
        Constraint::Length(1), // hint
    ])
    .split(area);

    draw_choice(f, app, chunks[0], chunks[1]);

    let pw_focusable = app.mp_form.want_password;
    draw_password_field(
        f,
        chunks[3],
        chunks[4],
        "Master password",
        &app.mp_form.password,
        app.mp_form.focused == MpField::Password,
        pw_focusable,
    );
    draw_password_field(
        f,
        chunks[5],
        chunks[6],
        "Confirm",
        &app.mp_form.confirm,
        app.mp_form.focused == MpField::Confirm,
        pw_focusable,
    );

    draw_submit(f, app, chunks[8]);

    if let Some(err) = &app.mp_form.error {
        f.render_widget(
            Paragraph::new(Span::styled(
                err.as_str(),
                Style::default().fg(Color::Red),
            ))
            .wrap(Wrap { trim: false }),
            chunks[10],
        );
    }

    f.render_widget(
        Paragraph::new("Tab/Shift-Tab navigate · ←/→ or Space toggle · Enter submit · Esc quit")
            .style(Style::default().fg(Color::DarkGray)),
        chunks[11],
    );
}

fn draw_choice(f: &mut Frame, app: &App, label_area: Rect, value_area: Rect) {
    let focused = app.mp_form.focused == MpField::Choice;
    f.render_widget(
        Paragraph::new(Span::styled("Encrypt credentials at rest", label_style(focused))),
        label_area,
    );
    let bg = if focused { BG_FOCUSED } else { BG_UNFOCUSED };
    let yes = chip("Set password", app.mp_form.want_password, focused);
    let no = chip("Skip (don't encrypt)", !app.mp_form.want_password, focused);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default().bg(bg)),
            yes,
            Span::styled("  ", Style::default().bg(bg)),
            no,
            Span::styled(" ", Style::default().bg(bg)),
        ]))
        .style(Style::default().bg(bg)),
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

fn draw_password_field(
    f: &mut Frame,
    label_area: Rect,
    value_area: Rect,
    label: &str,
    value: &str,
    focused: bool,
    enabled: bool,
) {
    let label_style = if !enabled {
        Style::default().fg(Color::DarkGray)
    } else if focused {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    f.render_widget(
        Paragraph::new(Span::styled(label.to_string(), label_style)),
        label_area,
    );

    let bg = if focused && enabled {
        BG_FOCUSED
    } else {
        BG_UNFOCUSED
    };
    let masked: String = if enabled {
        "•".repeat(value.chars().count())
    } else {
        String::new()
    };
    let cursor = if focused && enabled { "▏" } else { " " };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default().bg(bg)),
            Span::styled(
                masked,
                Style::default().fg(Color::Yellow).bg(bg),
            ),
            Span::styled(cursor, Style::default().fg(Color::Yellow).bg(bg)),
        ]))
        .style(Style::default().bg(bg)),
        value_area,
    );
}

fn draw_submit(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.mp_form.focused == MpField::Submit;
    let style = if focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let label = if app.mp_form.want_password {
        " Set password and continue "
    } else {
        " Continue without password "
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
