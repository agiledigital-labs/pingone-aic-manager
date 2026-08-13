//! Shared TUI chrome — passive building blocks with no feature knowledge:
//! reusable widgets, the theme, the header/tab strip, toasts, modal chrome,
//! the confirm popup, the F1 keybind-help overlay, and the per-tenant list
//! state helper. Feature verticals (`esv`, `secrets`, `scripts`, …) draw
//! *with* these; nothing in here dispatches on a specific feature's modes
//! beyond rendering what `app::keymap` advertises.
//!
//! Visual + interaction rules: `docs/DESIGN.md`. Don't redebate them.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub mod fuzzy;
pub mod header;
pub mod keybind_help;
pub mod list_chrome;
pub mod list_state;
pub mod modal_chrome;
pub mod popup_confirm;
pub mod theme;
pub mod toast;
pub mod widgets;

/// The narrowest terminal the TUI supports: everything must stay legible and
/// unambiguous here, and narrower than this degradation is allowed to be ugly.
///
/// **Nothing checks this at runtime.** It is the width layout tests should
/// render at, so the floor lives in one place instead of as a literal in each
/// assertion; the policy itself is stated in `docs/DESIGN.md`, along with the
/// consequences that follow from it — chiefly that a table of `Percentage`
/// constraints degrades proportionally where a set mixing in `Length` starves
/// its percentage columns and clips them with no ellipsis.
///
/// Deliberately distinct from [`modal_chrome::CONTENT_WIDTH`], which happens to
/// share the value: that one is how wide a modal wants to be, and is capped by
/// the screen rather than by this.
pub const MIN_TERMINAL_WIDTH: u16 = 80;

/// True when `key` is the universal save chord. Accepts a shifted `S` so a
/// stuck shift key doesn't turn saving into a dead key.
pub fn is_save_chord(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('s' | 'S')) && key.modifiers.contains(KeyModifiers::CONTROL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_chord_accepts_control_s_with_or_without_shift() {
        assert!(is_save_chord(&KeyEvent::new(
            KeyCode::Char('s'),
            KeyModifiers::CONTROL,
        )));
        assert!(is_save_chord(&KeyEvent::new(
            KeyCode::Char('S'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )));
    }

    #[test]
    fn save_chord_rejects_other_or_unmodified_chords() {
        assert!(!is_save_chord(&KeyEvent::new(
            KeyCode::Char('s'),
            KeyModifiers::NONE,
        )));
        assert!(!is_save_chord(&KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::CONTROL,
        )));
        assert!(!is_save_chord(&KeyEvent::new(
            KeyCode::Char('s'),
            KeyModifiers::ALT,
        )));
    }
}
