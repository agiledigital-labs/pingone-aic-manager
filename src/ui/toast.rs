use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::App;
use crate::event::ToastKind;

#[derive(Debug, Clone)]
pub struct Toast {
    pub kind: ToastKind,
    pub message: String,
    pub ticks_remaining: u8,
}

impl Toast {
    pub fn new(kind: ToastKind, message: String) -> Self {
        // Error toasts stick around longer — they're usually the diagnostic.
        let ticks = match kind {
            ToastKind::Error => 30,
            ToastKind::Warning => 16,
            _ => 8,
        };
        Self {
            kind,
            message,
            ticks_remaining: ticks,
        }
    }
}

const TOAST_WIDTH: u16 = 60;
const TOAST_MAX_HEIGHT: u16 = 8;
const TOAST_MIN_HEIGHT: u16 = 3;

pub fn draw(f: &mut Frame, app: &App) {
    if app.toasts.is_empty() {
        return;
    }
    let area = f.area();
    let max_visible = 3usize;
    let mut next_y = area.y;

    for toast in app.toasts.iter().take(max_visible) {
        let (border_color, icon) = match toast.kind {
            ToastKind::Info => (Color::Cyan, "ℹ"),
            ToastKind::Success => (Color::Green, "✓"),
            ToastKind::Warning => (Color::Yellow, "⚠"),
            ToastKind::Error => (Color::Red, "✗"),
        };
        let body = format!("{icon} {}", toast.message);
        let inner_width = TOAST_WIDTH.saturating_sub(2) as usize;
        let lines_needed = wrapped_line_count(&body, inner_width) as u16;
        let height = lines_needed
            .saturating_add(2) // borders
            .clamp(TOAST_MIN_HEIGHT, TOAST_MAX_HEIGHT);

        if next_y + height > area.bottom() {
            break;
        }
        let x = area.x + area.width.saturating_sub(TOAST_WIDTH + 1);
        let rect = Rect {
            x,
            y: next_y,
            width: TOAST_WIDTH,
            height,
        };
        next_y += height + 1;

        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));
        let para = Paragraph::new(Line::from(Span::styled(
            body,
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        )))
        .wrap(Wrap { trim: false })
        .block(block);
        f.render_widget(para, rect);
    }
}

fn wrapped_line_count(text: &str, width: usize) -> usize {
    if width == 0 || text.is_empty() {
        return 0;
    }
    text.split('\n')
        .map(|line| {
            if line.is_empty() {
                1
            } else {
                line.chars().count().div_ceil(width)
            }
        })
        .sum()
}
