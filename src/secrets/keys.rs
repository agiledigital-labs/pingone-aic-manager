//! ESV-secret key bindings. Tables own dispatch and both hint surfaces.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::keymap::{
    Bind, HintTarget, Trigger, help_only, hidden, hint, pick, save_chord_bind,
};
use crate::app::{App, InputMode};
use crate::secrets::ops;
use crate::secrets::screen::Mode;
use crate::secrets::state::{CreateField, DetailFocus};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Cancel,
    Next,
    Prev,
    Create,
    Toggle,
    PrevChoice,
    NextChoice,
    SaveDescription,
    Up,
    Down,
    AddVersion,
    ToggleVersion,
    Destroy,
    Yes,
    No,
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) -> crate::Result<()> {
    match mode {
        Mode::Create => handle_create_key(app, key),
        Mode::Versions => handle_versions_key(app, key),
        Mode::AddVersion => handle_add_version_key(app, key),
        Mode::DeleteConfirm => handle_delete_confirm_key(app, key),
        Mode::VersionDestroyConfirm => handle_version_destroy_confirm_key(app, key),
    }
}
pub fn footer_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let InputMode::Secrets(mode) = app.input_mode else {
        return Vec::new();
    };
    if matches!(mode, Mode::DeleteConfirm | Mode::VersionDestroyConfirm) {
        return Vec::new();
    }
    hints(mode, app, HintTarget::Footer)
}
pub fn help_lines(mode: Mode, app: &App) -> Option<Vec<(&'static str, &'static str)>> {
    Some(hints(mode, app, HintTarget::Help))
}
fn hints(mode: Mode, app: &App, target: HintTarget) -> Vec<(&'static str, &'static str)> {
    match mode {
        Mode::Create => pick(
            &create_binds(app.secret.create.as_ref().map(|f| f.focused)),
            target,
        ),
        Mode::Versions => pick(&versions_binds(app.secret.detail_focus), target),
        Mode::AddVersion => pick(&add_version_binds(), target),
        Mode::DeleteConfirm => pick(&delete_binds(), target),
        Mode::VersionDestroyConfirm => pick(&destroy_binds(), target),
    }
}

fn create_binds(focused: Option<CreateField>) -> Vec<Bind<Act>> {
    let mut out = vec![
        hint(&[Trigger::TAB], "Tab", "next field", Act::Next),
        hidden(&[Trigger::BACKTAB], "Shift-Tab", "back", Act::Prev),
    ];
    match focused {
        Some(CreateField::Value | CreateField::Save) => out.push(hint(
            &[Trigger::ENTER, Trigger::Ctrl('s')],
            "Enter",
            "create",
            Act::Create,
        )),
        Some(CreateField::Encoding) => {
            out.push(hint(&[Trigger::LEFT], "←/→", "change", Act::PrevChoice));
            out.push(hidden(
                &[Trigger::RIGHT, Trigger::SPACE],
                "←/→",
                "change",
                Act::NextChoice,
            ));
            out.push(hint(&[Trigger::ENTER], "Enter", "next", Act::Next));
            out.push(save_chord_bind(Act::Create, "create"));
        }
        Some(CreateField::Placeholders | CreateField::Json) => {
            out.push(hint(
                &[
                    Trigger::ENTER,
                    Trigger::SPACE,
                    Trigger::LEFT,
                    Trigger::RIGHT,
                ],
                "←/→",
                "change",
                Act::Toggle,
            ));
            out.push(save_chord_bind(Act::Create, "create"));
        }
        Some(_) => {
            out.push(hint(&[Trigger::ENTER], "Enter", "next", Act::Next));
            out.push(save_chord_bind(Act::Create, "create"));
        }
        None => {}
    }
    out.push(hint(&[Trigger::ESC], "Esc", "cancel", Act::Cancel));
    out
}
fn versions_binds(focus: DetailFocus) -> Vec<Bind<Act>> {
    match focus {
        DetailFocus::Description => vec![
            hint(
                &[Trigger::TAB, Trigger::BACKTAB],
                "Tab",
                "versions",
                Act::Next,
            ),
            hint(
                &[Trigger::ENTER, Trigger::Ctrl('s')],
                "Enter",
                "save description",
                Act::SaveDescription,
            ),
            hint(&[Trigger::ESC], "Esc", "close", Act::Cancel),
        ],
        DetailFocus::Versions => vec![
            hint(
                &[Trigger::TAB, Trigger::BACKTAB],
                "Tab",
                "edit description",
                Act::Next,
            ),
            hint(
                &[Trigger::UP, Trigger::Char('k')],
                "↑/↓",
                "navigate",
                Act::Up,
            ),
            hidden(
                &[Trigger::DOWN, Trigger::Char('j')],
                "↑/↓",
                "navigate",
                Act::Down,
            ),
            hint(
                &[Trigger::Char('e'), Trigger::Char('d')],
                "e/d",
                "enable/disable",
                Act::ToggleVersion,
            ),
            hint(
                &[Trigger::Char('x'), Trigger::Code(KeyCode::Delete)],
                "x",
                "destroy",
                Act::Destroy,
            ),
            hint(&[Trigger::Ctrl('n')], "^N", "add version", Act::AddVersion),
            hint(&[Trigger::ESC], "Esc", "close", Act::Cancel),
        ],
    }
}
fn add_version_binds() -> Vec<Bind<Act>> {
    vec![
        help_only(
            &[Trigger::ENTER, Trigger::Ctrl('s')],
            "Enter",
            "add version",
            Act::Create,
        ),
        help_only(&[Trigger::ESC], "Esc", "cancel", Act::Cancel),
    ]
}
fn confirms(yes: &'static str) -> Vec<Bind<Act>> {
    vec![
        hint(
            &[Trigger::Char('y'), Trigger::Char('Y')],
            "y",
            yes,
            Act::Yes,
        ),
        hint(
            &[Trigger::Char('n'), Trigger::Char('N'), Trigger::ESC],
            "n/Esc",
            "cancel",
            Act::No,
        ),
    ]
}
fn delete_binds() -> Vec<Bind<Act>> {
    confirms("delete secret + all versions")
}
fn destroy_binds() -> Vec<Bind<Act>> {
    confirms("destroy version (irreversible)")
}

fn handle_create_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let Some(focused) = app.secret.create.as_ref().map(|f| f.focused) else {
        return Ok(());
    };
    if let Some(act) = Bind::resolve(&create_binds(Some(focused)), &key) {
        match act {
            Act::Cancel => {
                app.secret.create = None;
                app.input_mode = InputMode::Normal;
            }
            Act::Next | Act::Prev => {
                if let Some(f) = app.secret.create.as_mut() {
                    f.focused = if act == Act::Next {
                        f.focused.next()
                    } else {
                        f.focused.prev()
                    };
                }
            }
            Act::Create => ops::commit_create(app),
            Act::Toggle => {
                if let Some(f) = app.secret.create.as_mut() {
                    match focused {
                        CreateField::Placeholders => f.use_in_placeholders = !f.use_in_placeholders,
                        CreateField::Json => f.as_json = !f.as_json,
                        _ => {}
                    }
                }
            }
            Act::PrevChoice | Act::NextChoice => {
                if let Some(f) = app.secret.create.as_mut() {
                    f.encoding = if act == Act::PrevChoice {
                        f.encoding.prev()
                    } else {
                        f.encoding.next()
                    };
                }
            }
            _ => {}
        };
        return Ok(());
    }
    if let Some(f) = app.secret.create.as_mut() {
        f.error = None;
        match focused {
            CreateField::Id => {
                f.id.handle_key(&key);
            }
            CreateField::Description => {
                f.description.handle_key(&key);
            }
            CreateField::Value => {
                f.value.handle_key(&key);
            }
            _ => {}
        }
    }
    Ok(())
}
fn handle_add_version_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    if let Some(act) = Bind::resolve(&add_version_binds(), &key) {
        match act {
            Act::Create => ops::commit_add_version(app),
            Act::Cancel => {
                app.secret.add_version = None;
                app.input_mode = InputMode::Secrets(Mode::Versions);
            }
            _ => {}
        };
        return Ok(());
    }
    if let Some(f) = app.secret.add_version.as_mut() {
        f.error = None;
        f.value.handle_key(&key);
    }
    Ok(())
}
fn handle_versions_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let focus = app.secret.detail_focus;
    if let Some(act) = Bind::resolve(&versions_binds(focus), &key) {
        match act {
            Act::Cancel => app.input_mode = InputMode::Normal,
            Act::Next => {
                app.secret.detail_focus = match focus {
                    DetailFocus::Versions => DetailFocus::Description,
                    DetailFocus::Description => DetailFocus::Versions,
                }
            }
            Act::SaveDescription => ops::commit_description(app),
            Act::Up => app.secret.version_selected = app.secret.version_selected.saturating_sub(1),
            Act::Down => {
                if let Some(crate::secrets::state::VersionsView::Loaded { versions, .. }) =
                    crate::secrets::state::versions_view(app)
                {
                    app.secret.version_selected =
                        (app.secret.version_selected + 1).min(versions.len().saturating_sub(1));
                }
            }
            Act::AddVersion => crate::secrets::screen::open_add_version(app),
            Act::ToggleVersion => crate::secrets::screen::toggle_selected_version(app),
            Act::Destroy => crate::secrets::screen::destroy_selected_version(app),
            _ => {}
        };
        return Ok(());
    }
    if focus == DetailFocus::Description {
        app.secret.description.handle_key(&key);
    }
    Ok(())
}
fn handle_delete_confirm_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    if let Some(act) = Bind::resolve(&delete_binds(), &key) {
        match act {
            Act::Yes => crate::secrets::screen::confirm_delete(app),
            Act::No => {
                app.secret.pending_delete = None;
                app.input_mode = InputMode::Normal;
            }
            _ => {}
        }
    }
    Ok(())
}
fn handle_version_destroy_confirm_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    if let Some(act) = Bind::resolve(&destroy_binds(), &key) {
        match act {
            Act::Yes => crate::secrets::screen::confirm_version_destroy(app),
            Act::No => {
                app.secret.pending_version_destroy = None;
                app.input_mode = InputMode::Secrets(Mode::Versions);
            }
            _ => {}
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    const ALL_CREATE: [CreateField; 7] = [
        CreateField::Id,
        CreateField::Description,
        CreateField::Encoding,
        CreateField::Placeholders,
        CreateField::Json,
        CreateField::Value,
        CreateField::Save,
    ];
    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::empty())
    }
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }
    #[test]
    fn save_is_advertised_where_enter_does_not_create() {
        for focus in ALL_CREATE {
            let binds = create_binds(Some(focus));
            assert_eq!(Bind::resolve(&binds, &ctrl('s')), Some(Act::Create));
            let save = Bind::footer_hints(&binds)
                .iter()
                .any(|(label, _)| *label == "^S");
            let enter = Bind::resolve(&binds, &key(KeyCode::Enter)) == Some(Act::Create);
            assert_eq!(save, !enter, "{focus:?}");
        }
    }
    #[test]
    fn text_keys_are_unbound() {
        for focus in [
            CreateField::Id,
            CreateField::Description,
            CreateField::Value,
        ] {
            for code in [
                KeyCode::Left,
                KeyCode::Char(' '),
                KeyCode::Char('a'),
                KeyCode::Backspace,
            ] {
                assert!(Bind::resolve(&create_binds(Some(focus)), &key(code)).is_none());
            }
        }
    }
    #[test]
    fn value_and_save_enter_create() {
        for focus in [CreateField::Value, CreateField::Save] {
            assert_eq!(
                Bind::resolve(&create_binds(Some(focus)), &key(KeyCode::Enter)),
                Some(Act::Create)
            );
        }
    }
    #[test]
    fn confirms_are_yes_no_without_save() {
        for binds in [delete_binds(), destroy_binds()] {
            assert_eq!(
                Bind::resolve(&binds, &key(KeyCode::Char('y'))),
                Some(Act::Yes)
            );
            assert_eq!(
                Bind::resolve(&binds, &key(KeyCode::Char('n'))),
                Some(Act::No)
            );
            assert_eq!(Bind::resolve(&binds, &key(KeyCode::Esc)), Some(Act::No));
            assert_eq!(Bind::resolve(&binds, &ctrl('s')), None);
        }
    }
}
