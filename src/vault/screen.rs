//! Nested vault modes/events and the small dispatch layer for the three
//! credential screens.

use crossterm::event::KeyEvent;

use crate::app::App;
use crate::config::wraps::Wrap;

use super::auth::UnlockOk;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Setup,
    Unlock,
    Settings,
    SettingsConfirm,
    SettingsRename,
}

#[derive(Debug)]
pub enum Event {
    UnlockFinished(std::result::Result<UnlockOk, String>),
    EnrollmentFinished(std::result::Result<Wrap, String>),
}

pub async fn apply_event(app: &mut App, event: Event) {
    match event {
        Event::UnlockFinished(result) => super::unlock::handle_result(app, result).await,
        Event::EnrollmentFinished(result) => super::setup::handle_enroll_result(app, result).await,
    }
}

pub async fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) -> crate::Result<()> {
    match mode {
        Mode::Setup => super::setup::handle_key(app, key).await?,
        Mode::Unlock => super::unlock::handle_key(app, key),
        Mode::Settings => super::settings::handle_key(app, key)?,
        Mode::SettingsConfirm => super::settings::handle_confirm_key(app, key).await?,
        Mode::SettingsRename => super::settings::handle_rename_key(app, key)?,
    }
    Ok(())
}
