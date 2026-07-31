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
//! For textarea, `↑` / `↓` walk one *logical* (`\n`-delimited) line up/down,
//! preserving the column where possible. They deliberately don't step by
//! visual row: the widget doesn't know its render width at key-handling time.
//!
//! The terminal's native cursor is shown when the field is focused, so the
//! user always sees where they're typing.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
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
    /// `prefix_len()..=value.chars().count()`. Sits at the end after
    /// `with_initial` / `set`; updated by every edit / nav action.
    pub cursor: usize,
    pub kind: FieldKind,
    /// A fixed, non-editable leading portion of `value` (e.g. the `esv-`
    /// prefix on ESV ids). The cursor can't move before it and backspace /
    /// delete won't remove it; the user only edits the suffix.
    pub locked_prefix: String,
}

impl TextField {
    pub fn single_line(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: String::new(),
            cursor: 0,
            kind: FieldKind::SingleLine,
            locked_prefix: String::new(),
        }
    }

    pub fn masked(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: String::new(),
            cursor: 0,
            kind: FieldKind::Masked,
            locked_prefix: String::new(),
        }
    }

    pub fn textarea(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: String::new(),
            cursor: 0,
            kind: FieldKind::TextArea,
            locked_prefix: String::new(),
        }
    }

    pub fn with_initial(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self.cursor = self.value.chars().count();
        self
    }

    /// Seed `value` with a fixed prefix the user can't edit or delete (they
    /// type the suffix). The cursor starts just past the prefix.
    pub fn with_locked_prefix(mut self, prefix: impl Into<String>) -> Self {
        let prefix = prefix.into();
        self.value = prefix.clone();
        self.cursor = prefix.chars().count();
        self.locked_prefix = prefix;
        self
    }

    /// Number of leading chars protected from editing / cursor movement.
    fn prefix_len(&self) -> usize {
        self.locked_prefix.chars().count()
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
        // Won't delete into the locked prefix.
        if self.cursor <= self.prefix_len() {
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

    /// Delete the char at the cursor (no-op past the end or within the prefix).
    pub fn delete_forward(&mut self) {
        let total = self.value.chars().count();
        if self.cursor >= total || self.cursor < self.prefix_len() {
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
        if self.cursor > self.prefix_len() {
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
            self.cursor = start.max(self.prefix_len());
        } else {
            self.cursor = self.prefix_len();
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

    // One wrap decision, used for both the rendered rows and the cursor
    // position — see `wrap_rows` for why we don't let `Paragraph` wrap.
    let rows = wrap_rows(&field.value, inner_width);
    let (cursor_row, cursor_col) = position_in_rows(&rows, field.cursor);
    let total_rows = rows.len();
    let mut scroll_y = total_rows.saturating_sub(inner_height);
    // Make sure the cursor row is visible — if the user navigates upward
    // into the scrolled-off region, scroll back up.
    if cursor_row < scroll_y {
        scroll_y = cursor_row;
    } else if cursor_row >= scroll_y + inner_height {
        scroll_y = cursor_row + 1 - inner_height;
    }
    let body_style = Style::default().fg(value_fg(focused)).bg(bg);

    let chars: Vec<char> = field.value.chars().collect();
    let lines: Vec<Line> = rows
        .iter()
        .map(|row| Line::from(chars[row.start..row.end].iter().collect::<String>()))
        .collect();
    f.render_widget(
        Paragraph::new(lines)
            .style(body_style)
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

/// One visual row of a wrapped textarea: the half-open char range `start..end`
/// of the buffer that it displays.
struct Row {
    start: usize,
    end: usize,
    /// True when the row ended because the column filled rather than because a
    /// `\n` terminated it. The distinction only matters for a cursor sitting at
    /// `end`: on a wrapped row that position is column 0 of the next row, but on
    /// a newline-terminated row it stays here, just before the newline.
    wrapped: bool,
}

/// Break `text` into the visual rows a textarea of `width` columns displays.
///
/// This hard-wraps at the column boundary rather than at word boundaries, and
/// the caller renders these rows verbatim instead of handing the raw string to
/// `Paragraph::wrap`. Both halves of that are deliberate: ratatui's wrap is a
/// *word* wrap, so a long word that doesn't fit moves to the next row whole and
/// leaves the row before it short. Nothing reports that decision back, so any
/// cursor math has to guess where the word went — and guessing character wrap
/// puts the cursor left of the text the user is typing, by however many columns
/// the wrap left blank. Deriving the rows and the cursor from this one function
/// keeps them in agreement by construction.
///
/// Columns are counted in `char`s, so double-width glyphs still misplace the
/// cursor. That predates this function and needs `unicode-width` to fix.
fn wrap_rows(text: &str, width: usize) -> Vec<Row> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut start = 0usize;
    let mut col = 0usize;
    let mut total = 0usize;
    for (idx, c) in text.chars().enumerate() {
        total = idx + 1;
        if c == '\n' {
            rows.push(Row {
                start,
                end: idx,
                wrapped: false,
            });
            start = idx + 1;
            col = 0;
        } else {
            col += 1;
            if col == width {
                rows.push(Row {
                    start,
                    end: idx + 1,
                    wrapped: true,
                });
                start = idx + 1;
                col = 0;
            }
        }
    }
    // The trailing row is always present, even when empty: a buffer ending in a
    // newline or on a wrap boundary still needs a row for the cursor to sit on.
    rows.push(Row {
        start,
        end: total.max(start),
        wrapped: false,
    });
    rows
}

/// Locate a char-index cursor within pre-computed `rows` as `(row, column)`.
fn position_in_rows(rows: &[Row], cursor: usize) -> (usize, usize) {
    for (idx, row) in rows.iter().enumerate() {
        let at_end = cursor == row.end;
        // A cursor at the end of a wrapped row renders at column 0 of the next
        // one; the final row has no next one to fall through to.
        if cursor < row.end || (at_end && !(row.wrapped && idx + 1 < rows.len())) {
            return (idx, cursor.saturating_sub(row.start));
        }
    }
    match rows.last() {
        Some(row) => (rows.len() - 1, row.end.saturating_sub(row.start)),
        None => (0, 0),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    /// The rendered rows and the cursor come from one wrap, so the char under
    /// the cursor is always the char the cursor sits on. This is the property
    /// that broke when `Paragraph`'s word wrap decided the rows instead.
    fn rendered_char_at_cursor(text: &str, cursor: usize, width: usize) -> Option<char> {
        let rows = wrap_rows(text, width);
        let (row, col) = position_in_rows(&rows, cursor);
        let chars: Vec<char> = text.chars().collect();
        let range = &rows[row];
        chars.get(range.start + col).copied().filter(|_| {
            // Only meaningful inside the row's own span.
            range.start + col < range.end
        })
    }

    #[test]
    fn cursor_tracks_the_end_of_a_long_wrapped_word() {
        // A single word longer than the row: word wrap would move it whole and
        // leave row 0 blank, which is what used to desync the cursor.
        let text = "aaaa bbbbbbbbbb";
        let width = 8;
        let rows = wrap_rows(text, width);
        let rendered: Vec<String> = rows
            .iter()
            .map(|r| {
                text.chars().collect::<Vec<_>>()[r.start..r.end]
                    .iter()
                    .collect()
            })
            .collect();
        // Hard wrap fills row 0 to the column boundary. Word wrap would have
        // rendered "aaaa" then "bbbbbbbbbb", leaving row 0 four columns short
        // while the cursor math still counted them as used.
        assert_eq!(rendered, vec!["aaaa bbb", "bbbbbbb"]);

        // Cursor at end-of-buffer lands just past the last rendered char.
        let (row, col) = position_in_rows(&rows, text.chars().count());
        assert_eq!((row, col), (1, 7));

        // And every interior position points at the char it should.
        for (i, expected) in text.chars().enumerate() {
            assert_eq!(
                rendered_char_at_cursor(text, i, width),
                Some(expected),
                "cursor {i} of {text:?}"
            );
        }
    }

    #[test]
    fn wrap_boundary_puts_the_cursor_on_the_next_row() {
        let rows = wrap_rows("abcd", 2);
        assert_eq!(position_in_rows(&rows, 0), (0, 0));
        assert_eq!(position_in_rows(&rows, 1), (0, 1));
        // End of a filled row is column 0 of the next, not column `width`.
        assert_eq!(position_in_rows(&rows, 2), (1, 0));
        assert_eq!(position_in_rows(&rows, 4), (2, 0));
    }

    #[test]
    fn newline_terminated_row_keeps_the_cursor_before_the_newline() {
        let rows = wrap_rows("ab\ncd", 8);
        // Unlike a wrap, a `\n` leaves the cursor at the end of its own row.
        assert_eq!(position_in_rows(&rows, 2), (0, 2));
        assert_eq!(position_in_rows(&rows, 3), (1, 0));
        assert_eq!(position_in_rows(&rows, 5), (1, 2));
    }

    #[test]
    fn trailing_newline_gets_a_row_to_sit_on() {
        let rows = wrap_rows("ab\n", 8);
        assert_eq!(rows.len(), 2);
        assert_eq!(position_in_rows(&rows, 3), (1, 0));
    }

    #[test]
    fn empty_buffer_has_one_row() {
        let rows = wrap_rows("", 8);
        assert_eq!(rows.len(), 1);
        assert_eq!(position_in_rows(&rows, 0), (0, 0));
    }

    #[test]
    fn rows_never_exceed_the_render_width() {
        // Nothing overflows the area, so the caller can render rows verbatim
        // with no `Wrap` to second-guess the line breaks.
        let text = "short\nan extremely long unbroken token aaaaaaaaaaaaaaaaaaaa\n\nx";
        for width in 1..20 {
            for row in wrap_rows(text, width) {
                assert!(
                    row.end - row.start <= width,
                    "row {}..{} exceeds width {width}",
                    row.start,
                    row.end
                );
            }
        }
    }

    #[test]
    fn locked_prefix_cannot_be_deleted_or_crossed() {
        let mut f = TextField::single_line("id").with_locked_prefix("esv-");
        assert_eq!(f.value, "esv-");
        assert_eq!(f.cursor, 4);

        // Typing appends after the prefix.
        for c in "name".chars() {
            f.handle_key(&key(KeyCode::Char(c)));
        }
        assert_eq!(f.value, "esv-name");

        // Backspace stops at the prefix boundary.
        for _ in 0..20 {
            f.handle_key(&key(KeyCode::Backspace));
        }
        assert_eq!(f.value, "esv-");

        // Home / Left clamp to the prefix; forward-delete can't eat it.
        f.handle_key(&key(KeyCode::Home));
        assert_eq!(f.cursor, 4);
        f.handle_key(&key(KeyCode::Left));
        assert_eq!(f.cursor, 4);
        f.handle_key(&key(KeyCode::Delete));
        assert_eq!(f.value, "esv-");
    }
}

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
