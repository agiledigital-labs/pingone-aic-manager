//! Delete-tenant confirmation modal.

use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::App;
use crate::offboard::screen::{Form, Mode};
use crate::offboard::spec::{self, PromptAction, TargetKind};
use crate::tui::modal_chrome::Modal;
use crate::tui::theme::style_for;

pub fn draw(f: &mut Frame, app: &App, mode: Mode) {
    match mode {
        Mode::Probing => draw_status(
            f,
            app.offboard.pending_name.as_deref().unwrap_or("tenant"),
            "Reading local artifacts…",
            &[("Esc", "cancel")],
        ),
        Mode::Working => draw_status(
            f,
            app.offboard
                .form
                .as_ref()
                .map(|form| form.tenant.name.as_str())
                .or(app.offboard.pending_name.as_deref())
                .unwrap_or("tenant"),
            "Removing tenant…",
            &[],
        ),
        Mode::Confirm => match app.offboard.form.as_ref() {
            Some(form) => draw_confirm(f, form),
            None => draw_status(f, "tenant", "Nothing to confirm.", &[("Esc", "cancel")]),
        },
    }
}

fn draw_status(f: &mut Frame, name: &str, status: &str, hints: &[(&str, &str)]) {
    let title = format!("Delete tenant {name}");
    let body = Modal {
        title: &title,
        status: Some(status),
        hints,
        body_height: 1,
    }
    .draw(f, f.area());
    f.render_widget(Paragraph::new(""), body);
}

fn draw_confirm(f: &mut Frame, form: &Form) {
    let theme = style_for(form.tenant.theme);
    let title = format!("Delete tenant {}", form.tenant.name);
    let status = format!("{}  ·  {}", form.tenant.base_url, theme.label);
    let lines = body_lines(form);
    let hints = [
        ("↑/↓", "move"),
        ("Space", "toggle"),
        ("Enter", "delete"),
        ("Esc", "cancel"),
    ];
    let body = Modal {
        title: &title,
        status: Some(&status),
        hints: &hints,
        body_height: (lines.len() as u16).max(1),
    }
    .draw(f, f.area());
    f.render_widget(Paragraph::new(lines), body);
}

fn body_lines(form: &Form) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            "aic cannot delete the remote service account or log API key.",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            "Ticking those rows removes the local credential only.",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(""),
        Line::from(Span::styled(
            spec::CONSOLE_CLEANUP_HEADING,
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let leftovers = leftover_lines(form);
    if leftovers.is_empty() {
        lines.push(Line::from(Span::styled(
            spec::console_cleanup_none_line(),
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for line in leftovers {
            lines.push(Line::from(Span::styled(
                line,
                Style::default().fg(Color::White),
            )));
        }
    }
    lines.push(Line::from(""));

    let visible = form.visible();
    for (idx, kind) in visible.iter().copied().enumerate() {
        let selected = idx == form.cursor;
        lines.extend(row_lines(form, kind, selected));
    }
    lines
}

fn leftover_lines(form: &Form) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(id) = &form.plan.manual.sa_id {
        lines.push(spec::console_cleanup_sa_line(id));
    }
    if let Some(id) = &form.plan.manual.api_key_id {
        lines.push(spec::console_cleanup_log_key_line(id));
    }
    lines
}

fn row_lines(form: &Form, kind: TargetKind, selected: bool) -> Vec<Line<'static>> {
    let action = form.plan.prompt_for(kind, &form.accepted);
    let id = spec::row_id(kind, &form.tenant, &form.inventory);
    let (mark, mark_color, inert) = match &action {
        PromptAction::Ask { .. } if form.accepted.contains(&kind) => ("[x]", Color::Green, false),
        PromptAction::Ask { .. } => ("[ ]", Color::White, false),
        PromptAction::Implied { .. } => ("[x]", Color::DarkGray, true),
        PromptAction::Refused { .. } => ("[-]", Color::DarkGray, true),
        PromptAction::Absent => return Vec::new(),
    };

    let row_bg = if selected {
        Some(Color::DarkGray)
    } else {
        None
    };
    let name_color = if inert { Color::DarkGray } else { Color::White };
    let mut first = vec![
        Span::styled(
            format!("{mark} "),
            with_bg(Style::default().fg(mark_color), row_bg),
        ),
        Span::styled(
            kind.label().to_string(),
            with_bg(Style::default().fg(name_color), row_bg),
        ),
    ];
    if let Some(id) = id {
        first.push(Span::styled(
            format!("  {id}"),
            with_bg(Style::default().fg(Color::Cyan), row_bg),
        ));
    }
    let mut lines = vec![Line::from(first)];

    match action {
        PromptAction::Refused { reason } => {
            lines.push(detail_line(reason.to_string(), Color::Yellow, selected));
        }
        PromptAction::Implied { by } => {
            lines.push(detail_line(
                format!("included with {}", by.label()),
                Color::DarkGray,
                selected,
            ));
        }
        PromptAction::Ask { default_on: false } => {
            lines.push(detail_line(
                "provided externally — off unless you turn it on".into(),
                Color::DarkGray,
                selected,
            ));
            if let Some(note) = kind.consequence() {
                lines.push(detail_line(note.to_string(), Color::DarkGray, selected));
            }
        }
        PromptAction::Ask { default_on: true } => {
            if let Some(note) = kind.consequence() {
                lines.push(detail_line(note.to_string(), Color::DarkGray, selected));
            }
        }
        PromptAction::Absent => {}
    }
    lines
}

fn detail_line(text: String, fg: Color, selected: bool) -> Line<'static> {
    let bg = if selected {
        Some(Color::DarkGray)
    } else {
        None
    };
    Line::from(Span::styled(
        format!("    {text}"),
        with_bg(Style::default().fg(fg).add_modifier(Modifier::ITALIC), bg),
    ))
}

fn with_bg(style: Style, bg: Option<Color>) -> Style {
    match bg {
        Some(bg) => style.bg(bg),
        None => style,
    }
}
