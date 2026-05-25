use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::app::App;
use crate::screens::unlock::Focus as UnlockFocus;
use crate::security_key;
use crate::ui::widgets::secret_field;

/// Re-export so `app.rs` can compare `unlock_error` against the tap status.
pub use crate::security_key::TAP_MESSAGE;

const UNLOCKING: &str = "Unlocking…";

pub fn draw(f: &mut Frame, app: &App) {
    let has_yk = app.wraps.has_security_key();
    let has_pw = app.wraps.has_password();
    let both = has_yk && has_pw;
    // Single-field layout: when only one method exists, show that one; when
    // both exist, show whichever the focus is on (Tab toggles).
    let show_pin = if both {
        app.unlock.focus == UnlockFocus::SecurityKeyPin
    } else {
        has_yk
    };

    let area = centered_form(f.area(), 60, 10);
    let chunks = Layout::vertical([
        Constraint::Length(secret_field::HEIGHT), // active field
        Constraint::Length(1),                    // gap above hints
        Constraint::Length(1),                    // footer hints
        Constraint::Length(1),                    // gap above error
        Constraint::Length(3),                    // error (wraps)
    ])
    .split(area);

    let waiting_for_tap = app.unlock.error.as_deref() == Some(TAP_MESSAGE);

    if show_pin {
        secret_field::draw(
            f,
            chunks[0],
            "Security key PIN",
            &app.unlock.pin_input,
            true,
            if waiting_for_tap {
                Some(security_key::TAP_MESSAGE)
            } else {
                None
            },
        );
    } else {
        secret_field::draw(
            f,
            chunks[0],
            "Master password",
            &app.unlock.input,
            true,
            if app.unlock.busy { Some(UNLOCKING) } else { None },
        );
    }

    draw_footer_hints(f, chunks[2], both);

    // Error row sits below the hints with a 1-row gap. The waiting-for-tap
    // status is shown inside the PIN field's status row, not down here.
    if let Some(err) = &app.unlock.error {
        if !waiting_for_tap {
            f.render_widget(
                Paragraph::new(Span::styled(
                    err.as_str(),
                    Style::default().fg(Color::Red),
                ))
                .wrap(Wrap { trim: false }),
                chunks[4],
            );
        }
    }
}

fn draw_footer_hints(f: &mut Frame, area: Rect, has_tab: bool) {
    let mut spans: Vec<Span> = Vec::new();
    if has_tab {
        spans.extend(footer_hint("Tab", "switch method"));
    }
    spans.extend(footer_hint("Enter", "submit"));
    spans.extend(footer_hint("Esc", "quit"));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn footer_hint(key: &'static str, desc: &'static str) -> Vec<Span<'static>> {
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

/// Center a fixed-size form within the full terminal — no outer block, just
/// the form fields floating in the empty screen.
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
