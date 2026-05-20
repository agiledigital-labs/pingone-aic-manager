//! Bordered text-input widget shared across the onboarding forms.
//!
//! A [`TextField`] owns its label, current value, and rendering kind. Forms
//! hold field instances directly rather than pairs of `String` + label string;
//! changing a label or the way a field renders is a single edit here.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// Single bordered row, plaintext value, horizontal scroll on overflow.
    SingleLine,
    /// Single bordered row, value rendered as a fixed-size mask
    /// (`head••••••••tail  (N chars)`).
    Masked,
    /// Multi-line bordered text area; vertical scroll keeps the cursor visible.
    TextArea,
}

#[derive(Debug, Clone)]
pub struct TextField {
    pub label: String,
    pub value: String,
    pub kind: FieldKind,
}

impl TextField {
    pub fn single_line(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: String::new(),
            kind: FieldKind::SingleLine,
        }
    }

    pub fn masked(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: String::new(),
            kind: FieldKind::Masked,
        }
    }

    pub fn textarea(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: String::new(),
            kind: FieldKind::TextArea,
        }
    }

    pub fn with_initial(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    pub fn push_char(&mut self, c: char) {
        self.value.push(c);
    }

    /// Append a literal newline. Caller should only call this when the kind is
    /// `TextArea`; for single-line fields, callers map Enter to "advance focus"
    /// rather than to "insert newline".
    pub fn push_newline(&mut self) {
        self.value.push('\n');
    }

    pub fn backspace(&mut self) {
        self.value.pop();
    }

    pub fn trimmed(&self) -> &str {
        self.value.trim()
    }

    pub fn is_empty(&self) -> bool {
        self.value.trim().is_empty()
    }

    pub fn set(&mut self, value: impl Into<String>) {
        self.value = value.into();
    }

    /// Recommended row height for layouts. TextArea returns a small default;
    /// callers typically override it with a larger `Constraint::Min(n)`.
    pub fn height_hint(&self) -> u16 {
        match self.kind {
            FieldKind::SingleLine | FieldKind::Masked => 3,
            FieldKind::TextArea => 6,
        }
    }

    pub fn draw(&self, f: &mut Frame, area: Rect, focused: bool) {
        match self.kind {
            FieldKind::SingleLine => draw_single_line(f, area, &self.label, &self.value, focused, false),
            FieldKind::Masked => draw_single_line(f, area, &self.label, &self.value, focused, true),
            FieldKind::TextArea => draw_textarea(f, area, &self.label, &self.value, focused),
        }
    }
}

// ---- Drawing primitives ----

fn label_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    }
}

fn border_color(focused: bool) -> Color {
    if focused { Color::Yellow } else { Color::DarkGray }
}

fn draw_single_line(
    f: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    focused: bool,
    mask: bool,
) {
    let title = Span::styled(format!(" {label} "), label_style(focused));
    let inner_width = area.width.saturating_sub(2) as usize;
    let display = if mask {
        mask_for_display(value)
    } else {
        value.replace('\n', " ")
    };
    let value_chars = display.chars().count();
    let scroll_x = if value_chars >= inner_width.saturating_sub(1) {
        (value_chars + 2).saturating_sub(inner_width) as u16
    } else {
        0
    };
    let cursor = if focused { "▏" } else { " " };
    let line = Line::from(vec![
        Span::styled(
            display,
            Style::default().fg(if focused { Color::Yellow } else { Color::Gray }),
        ),
        Span::styled(cursor, Style::default().fg(Color::Yellow)),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color(focused)))
        .title(title);
    f.render_widget(
        Paragraph::new(line).block(block).scroll((0, scroll_x)),
        area,
    );
}

fn draw_textarea(f: &mut Frame, area: Rect, label: &str, value: &str, focused: bool) {
    let title = Span::styled(format!(" {label} "), label_style(focused));
    let body_style = Style::default().fg(if focused { Color::Yellow } else { Color::Gray });
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color(focused)))
        .title(title);

    let inner_width = area.width.saturating_sub(2) as usize;
    let inner_height = area.height.saturating_sub(2) as usize;
    let wrapped = count_wrapped_lines(value, inner_width);
    let scroll_y = wrapped.saturating_sub(inner_height) as u16;

    f.render_widget(
        Paragraph::new(value.to_string())
            .style(body_style)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((scroll_y, 0)),
        area,
    );
}

// Compact masking that doesn't scale with input length — first 4 chars + bullets
// + last 4 chars + an explicit length suffix. A paste of any size yields the same
// ~25-char display so the field can't overflow on the user.
fn mask_for_display(value: &str) -> String {
    let trimmed = value.trim_end_matches(['\n', '\r']);
    if trimmed.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let n = chars.len();
    if n <= 8 {
        return "•".repeat(n);
    }
    let head: String = chars.iter().take(4).collect();
    let tail: String = chars.iter().rev().take(4).rev().collect();
    format!("{head}••••••••{tail}  ({n} chars)")
}

fn count_wrapped_lines(text: &str, width: usize) -> usize {
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

// ---- Factories for the recurring fields across the three onboarding forms ----
//
// Centralising these lets us change a label or render-kind in one place. Form
// modules call e.g. `fields::tenant_name()` at construction time.

pub mod fields {
    use super::TextField;

    pub fn tenant_name() -> TextField {
        TextField::single_line("Tenant name")
    }

    pub fn hostname() -> TextField {
        TextField::single_line("Tenant hostname  (e.g. openam-mytenant-prod.forgeblocks.com)")
    }

    pub fn cookie_name() -> TextField {
        TextField::single_line("Cookie name  (per-tenant random hex)")
    }

    pub fn cookie_value() -> TextField {
        TextField::masked("Session cookie value")
    }

    pub fn realm() -> TextField {
        TextField::single_line("Realm  (root for platform admins; alpha/bravo for end users)")
            .with_initial("root")
    }

    pub fn username() -> TextField {
        TextField::single_line("Username")
    }

    pub fn password() -> TextField {
        TextField::masked("Password")
    }

    pub fn sa_uuid() -> TextField {
        TextField::single_line("Service account UUID")
    }

    pub fn jwk() -> TextField {
        TextField::textarea("Private JWK JSON")
    }
}
