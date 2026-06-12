//! Local credential vault: the DEK envelope and the password/FIDO2 flows
//! that unlock it, plus first-run setup and auth-factor management.
//!
//! The feature is split by responsibility:
//! - [`auth`] contains async-friendly unlock helpers shared by the TUI and CLI.
//! - [`security_key`] contains the synchronous CTAP2 `hmac-secret` integration.
//! - [`unlock`], [`setup`], and [`settings`] each own one screen's state and handlers.
//! - [`unlock_view`], [`setup_view`], and [`settings_view`] are the matching
//!   sibling render modules; all three screens use this uniform `*_view` shape.
//! - [`screen`] owns the nested input modes/events and dispatches keys/results.
//!
//! The encrypted storage primitives and persisted wrap schema remain in
//! [`crate::config::crypto`] and [`crate::config::wraps`] because the CLI login
//! path shares them. The in-memory DEK, wraps, and decrypted JWKs remain on
//! [`crate::app::App`] because they are cross-feature session state.
//!
//! Locking drops credentials from the resident agent but does not stop that
//! process; see the lock-vs-stop discussion in [`crate::agent`]. This is a
//! local-only feature with no AIC endpoints or API documentation. Project-wide
//! credential rules live in CLAUDE.md.

pub mod auth;
pub mod screen;
pub mod security_key;
pub mod settings;
pub mod settings_view;
pub mod setup;
pub mod setup_view;
pub mod unlock;
pub mod unlock_view;

use ratatui::Frame;

use crate::app::App;

pub fn draw(f: &mut Frame, app: &App, mode: screen::Mode) {
    match mode {
        screen::Mode::Setup => setup_view::draw(f, app),
        screen::Mode::Unlock => unlock_view::draw(f, app),
        screen::Mode::Settings => settings_view::draw(f, app),
        screen::Mode::SettingsConfirm => settings_view::draw_confirm(f, app),
        screen::Mode::SettingsRename => settings_view::draw_rename(f, app),
    }

    if app.keybind_help_open {
        crate::ui::keybind_help::draw(f, app);
    }
    crate::ui::toast::draw(f, app);
}
