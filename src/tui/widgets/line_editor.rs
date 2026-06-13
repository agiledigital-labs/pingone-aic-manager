//! Single-line text input with a cursor. Shared by every place in the app
//! that needs editable text — the ESV search bar, future filter / rename /
//! one-shot inputs. Forms with bordered multi-row inputs build on this via
//! [`crate::tui::widgets::text_field::TextField`].
//!
//! Storage is a plain `String`; the cursor is a **char** index in `0..=N`
//! (not a byte offset) so callers don't have to think about UTF-8 boundaries.
//!
//! ## Key dispatch
//!
//! Use [`LineEditor::handle_key`] to route a `KeyEvent` to the editor:
//!
//! - `←` / `→`           — move cursor by one char
//! - `Home` / `End`      — jump to start / end
//! - `Ctrl-A` / `Ctrl-E` — emacs-style start / end
//! - `Backspace`         — delete char before the cursor
//! - `Delete`            — delete char at the cursor
//! - any other `Char(c)` without `Ctrl` — insert at the cursor
//!
//! Returns `true` if the key was consumed; callers can fall through to
//! their own handler for keys like `↑` / `↓` that the editor deliberately
//! doesn't claim (e.g. so the ESV search can scroll the results list
//! while the user is still typing).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Default)]
pub struct LineEditor {
    value: String,
    /// Char index (not byte offset). Always in `0..=value.chars().count()`.
    cursor: usize,
}

impl LineEditor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_value(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.chars().count();
        Self { value, cursor }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    pub fn len_chars(&self) -> usize {
        self.value.chars().count()
    }

    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
    }

    pub fn set(&mut self, value: impl Into<String>) {
        self.value = value.into();
        self.cursor = self.value.chars().count();
    }

    /// Insert a char at the cursor and step past it.
    pub fn insert_char(&mut self, c: char) {
        let byte = self.cursor_byte();
        self.value.insert(byte, c);
        self.cursor += 1;
    }

    /// Delete the char immediately before the cursor (no-op at column 0).
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
        let total = self.len_chars();
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

    pub fn left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn right(&mut self) {
        let total = self.len_chars();
        if self.cursor < total {
            self.cursor += 1;
        }
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.len_chars();
    }

    /// Route a `KeyEvent` to the editor. Returns `true` when the key was
    /// consumed. See module docs for the supported bindings — anything else
    /// (notably `↑`/`↓`/`PageUp`/`PageDown`/`Enter`/`Esc`/`Tab`) falls
    /// through so the caller can route it to whatever it owns the focus of.
    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Left => self.left(),
            KeyCode::Right => self.right(),
            KeyCode::Home => self.home(),
            KeyCode::End => self.end(),
            KeyCode::Char('a') if ctrl => self.home(),
            KeyCode::Char('e') if ctrl => self.end(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete_forward(),
            KeyCode::Char(c) if !ctrl => self.insert_char(c),
            _ => return false,
        }
        true
    }

    fn cursor_byte(&self) -> usize {
        self.value
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.value.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn insert_at_cursor_then_move() {
        let mut e = LineEditor::new();
        for c in "hello".chars() {
            e.insert_char(c);
        }
        assert_eq!(e.value(), "hello");
        assert_eq!(e.cursor(), 5);
        e.left();
        e.left();
        e.insert_char('X');
        assert_eq!(e.value(), "helXlo");
        assert_eq!(e.cursor(), 4);
    }

    #[test]
    fn backspace_and_delete_forward() {
        let mut e = LineEditor::with_value("abcdef");
        e.left();
        e.left();
        // cursor between d and e
        e.backspace();
        assert_eq!(e.value(), "abcef");
        assert_eq!(e.cursor(), 3);
        e.delete_forward();
        assert_eq!(e.value(), "abcf");
        assert_eq!(e.cursor(), 3);
    }

    #[test]
    fn home_end_clamp() {
        let mut e = LineEditor::with_value("hi");
        e.left();
        e.left();
        e.left(); // clamps
        assert_eq!(e.cursor(), 0);
        e.end();
        assert_eq!(e.cursor(), 2);
        e.right(); // clamps
        assert_eq!(e.cursor(), 2);
    }

    #[test]
    fn multibyte_chars() {
        let mut e = LineEditor::new();
        for c in "café".chars() {
            e.insert_char(c);
        }
        assert_eq!(e.value(), "café");
        assert_eq!(e.cursor(), 4);
        e.left();
        e.backspace(); // delete 'f', not the trailing byte of 'é'
        assert_eq!(e.value(), "caé");
        assert_eq!(e.cursor(), 2);
    }

    #[test]
    fn handle_key_consumes_known_and_ignores_unknown() {
        let mut e = LineEditor::with_value("abc");
        assert!(e.handle_key(&key(KeyCode::Left)));
        assert_eq!(e.cursor(), 2);
        assert!(e.handle_key(&ctrl(KeyCode::Char('a')))); // emacs home
        assert_eq!(e.cursor(), 0);
        assert!(e.handle_key(&key(KeyCode::Char('z'))));
        assert_eq!(e.value(), "zabc");
        assert!(!e.handle_key(&key(KeyCode::Up)));
        assert!(!e.handle_key(&key(KeyCode::Enter)));
    }
}
