//! Shared TUI chrome — passive building blocks with no feature knowledge:
//! reusable widgets, the theme, the header/tab strip, toasts, modal chrome,
//! the confirm popup, the F1 keybind-help overlay, and the per-tenant list
//! state helper. Feature verticals (`esv`, `secrets`, `scripts`, …) draw
//! *with* these; nothing in here dispatches on a specific feature's modes
//! beyond rendering what `app::keymap` advertises.
//!
//! Visual + interaction rules: `docs/DESIGN.md`. Don't redebate them.

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
