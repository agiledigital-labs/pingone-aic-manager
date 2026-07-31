//! ESV key bindings. Tables drive dispatch, footer hints, and F1 help.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::keymap::{Bind, HintTarget, Trigger, hidden, hint, pick, save_chord_bind};
use crate::app::{App, InputMode};
use crate::esv::ops;
use crate::esv::screen::{Mode, cancel_edit, commit_save};
use crate::esv::state::{EditField, EsvView};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Cancel,
    Next,
    Prev,
    Save,
    InsertNewline,
    PrevType,
    NextType,
    Keep,
    Up,
    Down,
    PageUp,
    PageDown,
    Yes,
    No,
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) -> crate::Result<()> {
    match mode {
        Mode::Search => handle_search_key(app, key),
        Mode::Edit => handle_edit_key(app, key),
        Mode::RestartConfirm => handle_restart_confirm_key(app, key),
        Mode::DeleteConfirm => handle_delete_confirm_key(app, key),
    }
}

pub fn footer_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let InputMode::Esv(mode) = app.input_mode else {
        return Vec::new();
    };
    if matches!(mode, Mode::RestartConfirm | Mode::DeleteConfirm) {
        return Vec::new();
    }
    hints(mode, app, HintTarget::Footer)
}

pub fn help_lines(mode: Mode, app: &App) -> Option<Vec<(&'static str, &'static str)>> {
    let mut out = hints(mode, app, HintTarget::Help);
    if mode == Mode::Search {
        out.extend([
            ("Type", "edit search query"),
            ("Backspace", "delete character"),
            ("↑/↓", "move selection"),
            ("PgUp/PgDn", "move by page"),
            ("F1", "show keybinds"),
        ]);
    }
    Some(out)
}

fn hints(mode: Mode, app: &App, target: HintTarget) -> Vec<(&'static str, &'static str)> {
    match mode {
        Mode::Search => pick(&search_binds(), target),
        Mode::Edit => pick(
            &edit_binds(
                app.esv.editing.as_ref().map(|e| e.focused),
                app.esv.editing.as_ref().is_some_and(|e| e.creating),
            ),
            target,
        ),
        Mode::RestartConfirm => pick(&restart_binds(), target),
        Mode::DeleteConfirm => pick(&delete_binds(), target),
    }
}

fn search_binds() -> Vec<Bind<Act>> {
    vec![
        hint(&[Trigger::ENTER], "Enter", "keep filter", Act::Keep),
        hint(&[Trigger::ESC], "Esc", "clear + exit", Act::Cancel),
        hidden(&[Trigger::UP], "↑", "move selection", Act::Up),
        hidden(&[Trigger::DOWN], "↓", "move selection", Act::Down),
        hidden(
            &[Trigger::Code(KeyCode::PageUp)],
            "PgUp",
            "move by page",
            Act::PageUp,
        ),
        hidden(
            &[Trigger::Code(KeyCode::PageDown)],
            "PgDn",
            "move by page",
            Act::PageDown,
        ),
    ]
}

/// Pure form table. `creating` determines whether the otherwise read-only Id
/// row exists in the tab order.
fn edit_binds(focused: Option<EditField>, creating: bool) -> Vec<Bind<Act>> {
    let mut out = vec![
        hint(&[Trigger::TAB], "Tab", "navigate", Act::Next),
        hidden(&[Trigger::BACKTAB], "Shift-Tab", "back", Act::Prev),
    ];
    match focused {
        Some(EditField::Save) => out.push(hint(
            &[Trigger::ENTER, Trigger::Ctrl('s')],
            "Enter",
            "save",
            Act::Save,
        )),
        Some(EditField::Value) => {
            out.push(hidden(
                &[Trigger::ENTER],
                "Enter",
                "insert newline",
                Act::InsertNewline,
            ));
            out.push(save_chord_bind(Act::Save, "save"));
        }
        Some(EditField::Type) => {
            out.push(hint(&[Trigger::LEFT], "←/→", "change type", Act::PrevType));
            out.push(hidden(
                &[Trigger::RIGHT],
                "←/→",
                "change type",
                Act::NextType,
            ));
            out.push(hint(&[Trigger::ENTER], "Enter", "next", Act::Next));
            out.push(save_chord_bind(Act::Save, "save"));
        }
        Some(EditField::Id) if creating => {
            out.push(hint(&[Trigger::ENTER], "Enter", "next", Act::Next));
            out.push(save_chord_bind(Act::Save, "save"));
        }
        Some(EditField::Description) => {
            out.push(hint(&[Trigger::ENTER], "Enter", "next", Act::Next));
            out.push(save_chord_bind(Act::Save, "save"));
        }
        _ => {}
    }
    out.push(hint(&[Trigger::ESC], "Esc", "cancel", Act::Cancel));
    out
}
fn confirm_binds(yes: &'static str) -> Vec<Bind<Act>> {
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
fn restart_binds() -> Vec<Bind<Act>> {
    confirm_binds("restart tenant runtime")
}
fn delete_binds() -> Vec<Bind<Act>> {
    confirm_binds("delete variable")
}

fn handle_search_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    if app.esv.view == EsvView::Mappings {
        crate::secretmap::screen::handle_key(app, key, crate::secretmap::screen::Mode::Search);
        return Ok(());
    }
    if let Some(act) = Bind::resolve(&search_binds(), &key) {
        match act {
            Act::Cancel => {
                if app.esv.view == EsvView::Secrets {
                    app.secret.list.query.clear();
                    app.secret.list.selected = 0;
                    app.secret.list.scroll = 0;
                } else {
                    app.esv.reset_view();
                }
                app.input_mode = InputMode::Normal;
            }
            Act::Keep => app.input_mode = InputMode::Normal,
            Act::Up => crate::app::keymap::move_selection(app, -1),
            Act::Down => crate::app::keymap::move_selection(app, 1),
            Act::PageUp => crate::app::keymap::move_selection(app, -10),
            Act::PageDown => crate::app::keymap::move_selection(app, 10),
            _ => {}
        };
        return Ok(());
    }
    let list = if app.esv.view == EsvView::Secrets {
        &mut app.secret.list
    } else {
        &mut app.esv.list
    };
    let before = list.query.value().to_string();
    if list.query.handle_key(&key) && list.query.value() != before {
        list.selected = 0;
        list.scroll = 0;
    }
    Ok(())
}

pub fn handle_edit_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let Some((focused, creating)) = app.esv.editing.as_ref().map(|e| (e.focused, e.creating))
    else {
        return Ok(());
    };
    if let Some(act) = Bind::resolve(&edit_binds(Some(focused), creating), &key) {
        match act {
            Act::Cancel => cancel_edit(app),
            Act::Save => commit_save(app),
            Act::Next | Act::Prev => {
                if let Some(e) = app.esv.editing.as_mut() {
                    e.focused = if act == Act::Next {
                        e.focused.next(creating)
                    } else {
                        e.focused.prev(creating)
                    };
                }
            }
            Act::InsertNewline => {
                if let Some(e) = app.esv.editing.as_mut() {
                    e.value.push_newline();
                }
            }
            Act::PrevType | Act::NextType => {
                if let Some(e) = app.esv.editing.as_mut() {
                    e.expr_type = e.expr_type.cycle(if act == Act::PrevType { -1 } else { 1 });
                }
            }
            _ => {}
        };
        return Ok(());
    }
    if let Some(edit) = app.esv.editing.as_mut() {
        match focused {
            EditField::Id if creating => {
                edit.id_input.handle_key(&key);
            }
            EditField::Description => {
                edit.description.handle_key(&key);
            }
            EditField::Value => {
                edit.value.handle_key(&key);
            }
            _ => {}
        }
    }
    Ok(())
}
fn handle_restart_confirm_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    if let Some(act) = Bind::resolve(&restart_binds(), &key) {
        match act {
            Act::Yes => ops::trigger_restart(app),
            Act::No => app.input_mode = InputMode::Normal,
            _ => {}
        }
    }
    Ok(())
}
fn handle_delete_confirm_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    if let Some(act) = Bind::resolve(&delete_binds(), &key) {
        match act {
            Act::Yes => crate::esv::screen::confirm_delete(app),
            Act::No => {
                app.esv.pending_delete = None;
                app.input_mode = InputMode::Normal;
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
    const ALL_EDIT: [EditField; 5] = [
        EditField::Id,
        EditField::Description,
        EditField::Type,
        EditField::Value,
        EditField::Save,
    ];
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }
    #[test]
    fn save_advertisement_matches_enter_commit() {
        for creating in [false, true] {
            for focus in ALL_EDIT {
                if !creating && focus == EditField::Id {
                    continue;
                }
                let binds = edit_binds(Some(focus), creating);
                assert_eq!(Bind::resolve(&binds, &ctrl('s')), Some(Act::Save));
                let save = Bind::footer_hints(&binds)
                    .iter()
                    .any(|(label, _)| *label == "^S");
                let enter_saves = Bind::resolve(&binds, &key(KeyCode::Enter)) == Some(Act::Save);
                assert_eq!(save, !enter_saves, "{focus:?}, creating={creating}");
            }
        }
    }
    #[test]
    fn text_editing_keys_are_unbound() {
        for focus in [EditField::Id, EditField::Description, EditField::Value] {
            for code in [
                KeyCode::Left,
                KeyCode::Char(' '),
                KeyCode::Char('a'),
                KeyCode::Backspace,
            ] {
                assert!(
                    Bind::resolve(&edit_binds(Some(focus), true), &key(code)).is_none(),
                    "{focus:?} {code:?}"
                );
            }
        }
    }
    #[test]
    fn value_enter_inserts_newline() {
        assert_eq!(
            Bind::resolve(
                &edit_binds(Some(EditField::Value), true),
                &key(KeyCode::Enter)
            ),
            Some(Act::InsertNewline)
        );
    }
    #[test]
    fn confirms_are_yes_no_without_save() {
        for binds in [restart_binds(), delete_binds()] {
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
