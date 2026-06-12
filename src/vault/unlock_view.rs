use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Style},
    text::Span,
    widgets::{Paragraph, Wrap},
};

use crate::app::App;
use crate::ui::modal_chrome::Modal;
use crate::ui::widgets::secret_field;

use super::security_key;
use super::unlock::Focus as UnlockFocus;

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

    let hints: &[(&str, &str)] = if both {
        &[
            ("Tab", "switch method"),
            ("Enter", "submit"),
            ("Esc", "quit"),
        ]
    } else {
        &[("Enter", "submit"), ("Esc", "quit")]
    };

    // field (3) + gap (1) + error (3)
    const BODY: u16 = secret_field::HEIGHT + 1 + 3;
    let body = Modal {
        title: "Unlock",
        status: None,
        hints,
        body_height: BODY,
    }
    .draw(f, f.area());

    let waiting_for_tap = app.unlock.error.as_deref() == Some(security_key::TAP_MESSAGE);
    let chunks = Layout::vertical([
        Constraint::Length(secret_field::HEIGHT), // active field
        Constraint::Length(1),                    // gap
        Constraint::Length(3),                    // error (wraps)
        Constraint::Min(0),
    ])
    .split(body);

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
            if app.unlock.busy {
                Some(UNLOCKING)
            } else {
                None
            },
        );
    }

    // The waiting-for-tap status is rendered inside the PIN field's status
    // row, not here. Other errors land below the gap.
    if let Some(err) = &app.unlock.error {
        if !waiting_for_tap {
            f.render_widget(
                Paragraph::new(Span::styled(err.as_str(), Style::default().fg(Color::Red)))
                    .wrap(Wrap { trim: false }),
                chunks[2],
            );
        }
    }
}
