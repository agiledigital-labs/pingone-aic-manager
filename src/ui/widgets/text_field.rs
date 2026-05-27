//! Bordered text-input widget shared across the onboarding forms and the
//! ESV edit form.
//!
//! Storage is a [`String`] with a separate char-index `cursor`. Key
//! dispatch goes through [`TextField::handle_key`], which understands the
//! same edit/nav bindings as `LineEditor`:
//!
//! - `←` / `→` (and `Ctrl-A` / `Ctrl-E`, `Home` / `End`) — move the cursor
//! - `Backspace` / `Delete` — delete one char before / at the cursor
//! - `Enter` (textarea only) — insert a newline at the cursor
//! - any other `Char(c)` without `Ctrl` — insert at the cursor
//!
//! For textarea, `↑` / `↓` walk one visual line up/down using a best-effort
//! column-preserving heuristic — enough for editing JSON or plain text.
//!
//! The terminal's native cursor is shown when the field is focused, so the
//! user always sees where they're typing.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

/// Background shade for input value rows. A dim, near-black grey reads as
/// "input area" without screaming for attention; the focused variant is a
/// touch brighter so the active field stands out.
const BG_UNFOCUSED: Color = Color::Indexed(234); // #1c1c1c
const BG_FOCUSED: Color = Color::Indexed(236); // #303030

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
    /// Char index (not byte offset) into `value`. Always in
    /// `0..=value.chars().count()`. Sits at the end after `with_initial` /
    /// `set`; updated by every edit / nav action.
    pub cursor: usize,
    pub kind: FieldKind,
}

impl TextField {
    pub fn single_line(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: String::new(),
            cursor: 0,
            kind: FieldKind::SingleLine,
        }
    }

    pub fn masked(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: String::new(),
            cursor: 0,
            kind: FieldKind::Masked,
        }
    }

    pub fn textarea(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: String::new(),
            cursor: 0,
            kind: FieldKind::TextArea,
        }
    }

    pub fn with_initial(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self.cursor = self.value.chars().count();
        self
    }

    pub fn trimmed(&self) -> &str {
        self.value.trim()
    }

    pub fn is_empty(&self) -> bool {
        self.value.trim().is_empty()
    }

    pub fn set(&mut self, value: impl Into<String>) {
        self.value = value.into();
        self.cursor = self.value.chars().count();
    }

    /// Recommended row height for layouts. SingleLine/Masked want 2 rows
    /// (label + value); TextArea wants ~5+ and callers typically use
    /// `Constraint::Min(n)` to give them the leftover space.
    pub fn height_hint(&self) -> u16 {
        match self.kind {
            FieldKind::SingleLine | FieldKind::Masked => 2,
            FieldKind::TextArea => 5,
        }
    }

    /// Insert a char at the cursor and advance past it.
    pub fn push_char(&mut self, c: char) {
        let byte = self.cursor_byte();
        self.value.insert(byte, c);
        self.cursor += 1;
    }

    /// Insert a literal newline at the cursor (caller should only call this
    /// when `kind == TextArea`; single-line callers map Enter to "advance
    /// focus" instead).
    pub fn push_newline(&mut self) {
        self.push_char('\n');
    }

    /// Delete the char immediately before the cursor.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev_byte = self
            .value
            .char_indices()
            .nth(self.cursor - 1)
            .map(|(i, _)| i)
            .unwrap_or(0);
        let cur_byte = self.cursor_byte();
        self.value.replace_range(prev_byte..cur_byte, "");
        self.cursor -= 1;
    }

    /// Delete the char at the cursor (no-op past the end).
    pub fn delete_forward(&mut self) {
        let total = self.value.chars().count();
        if self.cursor >= total {
            return;
        }
        let cur_byte = self.cursor_byte();
        let next_byte = self
            .value
            .char_indices()
            .nth(self.cursor + 1)
            .map(|(i, _)| i)
            .unwrap_or(self.value.len());
        self.value.replace_range(cur_byte..next_byte, "");
    }

    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn cursor_right(&mut self) {
        let total = self.value.chars().count();
        if self.cursor < total {
            self.cursor += 1;
        }
    }

    /// Move to column 0 of the current visual line. For single-line this is
    /// the start of the buffer; for textarea it's the char just after the
    /// nearest preceding `\n`.
    pub fn cursor_home(&mut self) {
        if matches!(self.kind, FieldKind::TextArea) {
            let (start, _) = current_line_bounds(&self.value, self.cursor);
            self.cursor = start;
        } else {
            self.cursor = 0;
        }
    }

    /// Move to end of the current visual line / buffer.
    pub fn cursor_end(&mut self) {
        if matches!(self.kind, FieldKind::TextArea) {
            let (_, end) = current_line_bounds(&self.value, self.cursor);
            self.cursor = end;
        } else {
            self.cursor = self.value.chars().count();
        }
    }

    /// Move the cursor up one logical line in a textarea, preserving the
    /// column where possible. No-op on single-line / first line.
    pub fn cursor_up(&mut self) {
        if !matches!(self.kind, FieldKind::TextArea) {
            return;
        }
        let (start, _) = current_line_bounds(&self.value, self.cursor);
        if start == 0 {
            return;
        }
        let col = self.cursor - start;
        // Char immediately before `start` is the newline ending the prev
        // line. Move to that line's start, then advance by col.
        let prev_end = start - 1;
        let (prev_start, _) = current_line_bounds(&self.value, prev_end);
        let prev_len = prev_end - prev_start;
        self.cursor = prev_start + col.min(prev_len);
    }

    /// Move down one logical line, preserving column. No-op on last line.
    pub fn cursor_down(&mut self) {
        if !matches!(self.kind, FieldKind::TextArea) {
            return;
        }
        let (start, end) = current_line_bounds(&self.value, self.cursor);
        let total = self.value.chars().count();
        if end >= total {
            return;
        }
        let col = self.cursor - start;
        let next_start = end + 1; // skip the newline
        let (_, next_end) = current_line_bounds(&self.value, next_start);
        let next_len = next_end - next_start;
        self.cursor = next_start + col.min(next_len);
    }

    /// Route a `KeyEvent` to the field. Returns `true` when the key was
    /// consumed. Caller can fall through to its own dispatch for any
    /// `false` return (typically Tab / Shift-Tab / Enter / Esc).
    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Left => self.cursor_left(),
            KeyCode::Right => self.cursor_right(),
            KeyCode::Home => self.cursor_home(),
            KeyCode::End => self.cursor_end(),
            KeyCode::Up => self.cursor_up(),
            KeyCode::Down => self.cursor_down(),
            KeyCode::Char('a') if ctrl => self.cursor_home(),
            KeyCode::Char('e') if ctrl => self.cursor_end(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete_forward(),
            KeyCode::Char(c) if !ctrl => self.push_char(c),
            _ => return false,
        }
        true
    }

    pub fn draw(&self, f: &mut Frame, area: Rect, focused: bool) {
        match self.kind {
            FieldKind::SingleLine => draw_single_line(f, area, self, focused, false),
            FieldKind::Masked => draw_single_line(f, area, self, focused, true),
            FieldKind::TextArea => draw_textarea(f, area, self, focused),
        }
    }

    fn cursor_byte(&self) -> usize {
        self.value
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.value.len())
    }
}

// ---- Drawing primitives ----

fn label_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    }
}

fn value_bg(focused: bool) -> Color {
    if focused { BG_FOCUSED } else { BG_UNFOCUSED }
}

fn value_fg(focused: bool) -> Color {
    if focused { Color::Yellow } else { Color::Gray }
}

/// Render a labelled single-line input as `label` (line 0) + a dark-backed
/// value row (line 1). The terminal cursor is positioned at the edit
/// point when focused, so the user always sees where they're typing. The
/// row also scrolls horizontally so the cursor stays in view when the
/// value overflows.
fn draw_single_line(f: &mut Frame, area: Rect, field: &TextField, focused: bool, mask: bool) {
    if area.height == 0 {
        return;
    }
    // Label row.
    let label_area = Rect { height: 1, ..area };
    f.render_widget(
        Paragraph::new(Span::styled(field.label.to_string(), label_style(focused))),
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
    let inner_width = value_area.width.saturating_sub(1) as usize; // -1 for the leading gutter
    // Visible region: keep the cursor inside [scroll_x, scroll_x + inner_width).
    // We compute scroll_x from the cursor char index — for masked fields we
    // pin scroll to the end (the mask never grows beyond its compact form).
    let display: String = if mask {
        mask_for_display(&field.value)
    } else {
        field.value.replace('\n', " ")
    };
    let display_chars = display.chars().count();
    let cursor_col = if mask { display_chars } else { field.cursor };
    let scroll_x = if cursor_col >= inner_width {
        cursor_col + 1 - inner_width
    } else {
        0
    };

    let bg = value_bg(focused);
    let line = Line::from(vec![
        Span::styled(" ", Style::default().bg(bg)),
        Span::styled(display, Style::default().fg(value_fg(focused)).bg(bg)),
        // Pad the trailing portion of the row with the bg colour so the
        // dark strip extends to the right edge even when the value is
        // short.
        Span::styled(
            " ".repeat(value_area.width as usize),
            Style::default().bg(bg),
        ),
    ]);
    f.render_widget(
        Paragraph::new(line)
            .style(Style::default().bg(bg))
            .scroll((0, scroll_x as u16)),
        value_area,
    );

    if focused {
        let on_screen_col = cursor_col.saturating_sub(scroll_x);
        let cursor_x = value_area.x + 1 + on_screen_col as u16;
        if cursor_x < value_area.x + value_area.width {
            f.set_cursor_position(Position {
                x: cursor_x,
                y: value_area.y,
            });
        }
    }
}

/// Render a labelled multi-line textarea. Label is line 0; the rest is a
/// dark-backed wrapping region that scrolls vertically so the cursor stays in
/// view as content grows.
fn draw_textarea(f: &mut Frame, area: Rect, field: &TextField, focused: bool) {
    if area.height == 0 {
        return;
    }
    let label_area = Rect { height: 1, ..area };
    f.render_widget(
        Paragraph::new(Span::styled(field.label.to_string(), label_style(focused))),
        label_area,
    );

    if area.height < 2 {
        return;
    }
    let body_area = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: area.height - 1,
    };
    let bg = value_bg(focused);
    let inner_width = body_area.width.max(1) as usize;
    let inner_height = body_area.height as usize;

    // Visual (row, col) of every char position. Used both for scroll
    // alignment and for placing the terminal cursor.
    let (cursor_row, cursor_col) = visual_position(&field.value, field.cursor, inner_width);
    let total_rows = visual_position(&field.value, field.value.chars().count(), inner_width).0 + 1;
    let mut scroll_y = total_rows.saturating_sub(inner_height);
    // Make sure the cursor row is visible — if the user navigates upward
    // into the scrolled-off region, scroll back up.
    if cursor_row < scroll_y {
        scroll_y = cursor_row;
    } else if cursor_row >= scroll_y + inner_height {
        scroll_y = cursor_row + 1 - inner_height;
    }
    let body_style = Style::default().fg(value_fg(focused)).bg(bg);

    f.render_widget(
        Paragraph::new(field.value.clone())
            .style(body_style)
            .wrap(Wrap { trim: false })
            .scroll((scroll_y as u16, 0)),
        body_area,
    );

    if focused {
        let on_screen_row = cursor_row.saturating_sub(scroll_y);
        if on_screen_row < inner_height {
            f.set_cursor_position(Position {
                x: body_area.x + cursor_col as u16,
                y: body_area.y + on_screen_row as u16,
            });
        }
    }
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

/// Convert a char-index cursor into a visual `(row, col)` in a wrapped
/// textarea of the given `width`. Explicit `\n` advances to the next row;
/// otherwise we wrap whenever the column would equal `width`.
fn visual_position(text: &str, cursor: usize, width: usize) -> (usize, usize) {
    let mut row = 0usize;
    let mut col = 0usize;
    for (count, c) in text.chars().enumerate() {
        if count == cursor {
            break;
        }
        if c == '\n' {
            row += 1;
            col = 0;
        } else {
            col += 1;
            if col >= width {
                row += 1;
                col = 0;
            }
        }
    }
    (row, col)
}

/// Locate the start/end char indices of the line that contains `cursor`.
/// "Line" here means a `\n`-delimited segment of `text` — wrapping is
/// ignored, so Home/End jump to the logical line bounds regardless of how
/// the textarea has wrapped them visually.
fn current_line_bounds(text: &str, cursor: usize) -> (usize, usize) {
    let mut start = 0usize;
    for (idx, c) in text.chars().enumerate() {
        if idx >= cursor {
            break;
        }
        if c == '\n' {
            start = idx + 1;
        }
    }
    let mut end = start;
    for c in text.chars().skip(start) {
        if c == '\n' {
            break;
        }
        end += 1;
    }
    (start, end)
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
