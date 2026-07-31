//! Managed-objects tab key semantics: this is the single description of the
//! tab's keys, whose tables drive dispatch plus both footer and help hints. The
//! tables are pure functions of focus, so they can be unit-tested without
//! `App::new()`, which reads configuration from disk.

use crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;

use crate::app::keymap::{Bind, HintTarget, Trigger, hidden, hint, pick, save_chord_bind};
use crate::app::{App, InputMode};
use crate::managed::ops;
use crate::managed::screen::{
    Mode, relationship_target_matches, start_add_field, start_relationship_create,
};
use crate::managed::state::{
    AddFieldFocus, AddKind, EditFieldFocus, NewObjectFocus, RefPropDraft, RefPropFocus,
    RelationshipFocus, RelationshipFormState,
};

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) {
    match mode {
        Mode::Search => handle_search_key(app, key),
        Mode::EditField => handle_edit_key(app, key),
        Mode::AddChooseKind => handle_add_choose_key(app, key),
        Mode::AddField => handle_add_field_key(app, key),
        Mode::Relationship => handle_relationship_key(app, key),
        Mode::RelationshipTarget => handle_relationship_target_key(app, key),
        Mode::RefProp => handle_ref_prop_key(app, key),
        Mode::AddHook => handle_add_hook_key(app, key),
        Mode::DeleteFieldConfirm => handle_delete_confirm_key(app, key),
        Mode::DeleteObjectConfirm => handle_delete_object_confirm_key(app, key),
        Mode::RenameField => handle_rename_field_key(app, key),
        Mode::RenameObject => handle_rename_object_key(app, key),
        Mode::RenameObjectConfirm => handle_rename_object_confirm_key(app, key),
        Mode::NewObject => handle_new_object_key(app, key),
    }
}

pub fn footer_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let InputMode::Managed(mode) = app.input_mode else {
        return Vec::new();
    };
    hints(mode, app, HintTarget::Footer)
}

/// F1 help rows. Identical to the footer for every mode except `Search`, where
/// the overlay has room to spell out the list navigation the footer leaves
/// implicit. Deriving both from `hints` is what stops the two from drifting.
pub fn help_lines(mode: Mode, app: &App) -> Option<Vec<(&'static str, &'static str)>> {
    let mut out = hints(mode, app, HintTarget::Help);
    if mode == Mode::Search {
        out.extend([
            ("↑/↓", "move selection"),
            ("PgUp/PgDn", "move by page"),
            ("F1", "show keybinds"),
        ]);
    }
    Some(out)
}

/// The keys `mode` responds to, given the current focus.
///
/// `^S` commits from any field, but it is advertised only where `Enter` won't
/// do the job — on a Save row `Enter` already says so, and two hints for one
/// action just costs footer width. The `^S` label carries the form's own verb
/// (add / create / save) so the footer says what the key does, not what the
/// chord is conventionally called.
fn hints(mode: Mode, app: &App, target: HintTarget) -> Vec<(&'static str, &'static str)> {
    match mode {
        Mode::Search => pick(&search_binds(), target),
        Mode::EditField => pick(
            &edit_field_binds(app.managed.editing.as_ref().map(|edit| edit.focused)),
            target,
        ),
        Mode::AddChooseKind => pick(&add_choose_binds(), target),
        Mode::AddField => pick(
            &add_field_binds(app.managed.add_field.as_ref().map(|d| d.focused)),
            target,
        ),
        Mode::Relationship => pick(
            &relationship_binds(app.managed.relationship_form.as_ref().map(|d| d.focused)),
            target,
        ),
        Mode::RelationshipTarget => pick(&relationship_target_binds(), target),
        Mode::RefProp => pick(
            &ref_prop_binds(app.managed.ref_prop_draft.as_ref().map(|d| d.focused)),
            target,
        ),
        Mode::AddHook => pick(&add_hook_binds(), target),
        Mode::DeleteFieldConfirm => pick(&delete_field_binds(), target),
        Mode::DeleteObjectConfirm => pick(&delete_object_binds(), target),
        Mode::RenameField => pick(&rename_field_binds(), target),
        Mode::RenameObject => pick(&rename_object_binds(), target),
        Mode::RenameObjectConfirm => pick(&rename_object_confirm_binds(), target),
        Mode::NewObject => pick(
            &new_object_binds(app.managed.new_object.as_ref().map(|d| d.focused)),
            target,
        ),
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SearchAct {
    Cancel,
    Keep,
    Up,
    Down,
    PageUp,
    PageDown,
}
fn search_binds() -> Vec<Bind<SearchAct>> {
    use SearchAct::*;
    vec![
        hint(&[Trigger::ENTER], "Enter", "keep filter", Keep),
        hint(&[Trigger::ESC], "Esc", "clear + exit", Cancel),
        hidden(&[Trigger::UP], "↑", "move selection", Up),
        hidden(&[Trigger::DOWN], "↓", "move selection", Down),
        hidden(
            &[Trigger::Code(KeyCode::PageUp)],
            "PgUp",
            "move by page",
            PageUp,
        ),
        hidden(
            &[Trigger::Code(KeyCode::PageDown)],
            "PgDn",
            "move by page",
            PageDown,
        ),
    ]
}
fn run_search(app: &mut App, act: SearchAct) {
    match act {
        SearchAct::Cancel => {
            app.managed.reset_view();
            app.input_mode = InputMode::Normal;
        }
        SearchAct::Keep => app.input_mode = InputMode::Normal,
        SearchAct::Up => crate::app::keymap::move_selection(app, -1),
        SearchAct::Down => crate::app::keymap::move_selection(app, 1),
        SearchAct::PageUp => crate::app::keymap::move_selection(app, -10),
        SearchAct::PageDown => crate::app::keymap::move_selection(app, 10),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AddChooseAct {
    Cancel,
    Toggle,
    Continue,
}
fn add_choose_binds() -> Vec<Bind<AddChooseAct>> {
    use AddChooseAct::*;
    vec![
        hint(
            &[
                Trigger::TAB,
                Trigger::BACKTAB,
                Trigger::LEFT,
                Trigger::RIGHT,
                Trigger::SPACE,
            ],
            "←/→ or Tab",
            "choose kind",
            Toggle,
        ),
        hint(&[Trigger::ENTER], "Enter", "continue", Continue),
        hint(&[Trigger::ESC], "Esc", "cancel", Cancel),
    ]
}
fn run_add_choose(app: &mut App, act: AddChooseAct) {
    match act {
        AddChooseAct::Cancel => ops::cancel_active_draft(app),
        AddChooseAct::Toggle => {
            if let Some(d) = app.managed.add_choose.as_mut() {
                d.kind.toggle();
            }
        }
        AddChooseAct::Continue => match app.managed.add_choose.take().map(|d| d.kind) {
            Some(AddKind::Field) => start_add_field(app),
            Some(AddKind::Relationship) => start_relationship_create(app),
            None => app.input_mode = InputMode::Normal,
        },
    }
}

/// Actions shared by the add-field, reference-property, new-object, and
/// relationship forms. One vocabulary, four `run_*` functions — the forms agree
/// on what a key *means* and differ only in which draft it acts on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FormAct {
    Cancel,
    Next,
    Prev,
    Save,
    ToggleBool,
    NextChoice,
    PrevChoice,
    PickTarget,
    AddRef,
    EditRef,
    DeleteRef,
    Up,
    Down,
}

/// The shared navigation bindings for form tables; each caller appends its
/// cancel binding after focus-specific hints so `Esc` remains last.
fn form_base<A: Copy>(next: A, prev: A) -> Vec<Bind<A>> {
    vec![
        hint(&[Trigger::TAB], "Tab", "navigate", next),
        hidden(&[Trigger::BACKTAB], "Shift-Tab", "back", prev),
    ]
}
fn add_field_binds(focused: Option<AddFieldFocus>) -> Vec<Bind<FormAct>> {
    let mut out = form_base(FormAct::Next, FormAct::Prev);
    match focused {
        Some(AddFieldFocus::Save) => out.push(hint(
            &[Trigger::ENTER, Trigger::Ctrl('s')],
            "Enter",
            "add",
            FormAct::Save,
        )),
        Some(AddFieldFocus::Type) => {
            out.push(hint(
                &[Trigger::ENTER, Trigger::RIGHT, Trigger::SPACE],
                "←/→",
                "change type",
                FormAct::NextChoice,
            ));
            out.push(hidden(
                &[Trigger::LEFT],
                "←/→",
                "change type",
                FormAct::PrevChoice,
            ));
            out.push(save_chord_bind(FormAct::Save, "add"));
        }
        Some(focus) if focus.is_bool() => {
            out.push(hint(
                &[
                    Trigger::ENTER,
                    Trigger::SPACE,
                    Trigger::LEFT,
                    Trigger::RIGHT,
                ],
                "Space",
                "toggle",
                FormAct::ToggleBool,
            ));
            out.push(save_chord_bind(FormAct::Save, "add"));
        }
        Some(_) => {
            out.push(hint(&[Trigger::ENTER], "Enter", "next", FormAct::Next));
            out.push(save_chord_bind(FormAct::Save, "add"));
        }
        None => {}
    }
    out.push(hint(&[Trigger::ESC], "Esc", "cancel", FormAct::Cancel));
    out
}
fn ref_prop_binds(focused: Option<RefPropFocus>) -> Vec<Bind<FormAct>> {
    let mut out = form_base(FormAct::Next, FormAct::Prev);
    match focused {
        Some(RefPropFocus::Save) => out.push(hint(
            &[Trigger::ENTER, Trigger::Ctrl('s')],
            "Enter",
            "save",
            FormAct::Save,
        )),
        Some(RefPropFocus::Type) => {
            out.push(hint(
                &[Trigger::RIGHT, Trigger::SPACE],
                "←/→",
                "change type",
                FormAct::NextChoice,
            ));
            out.push(hidden(
                &[Trigger::LEFT],
                "←/→",
                "change type",
                FormAct::PrevChoice,
            ));
            out.push(hint(&[Trigger::ENTER], "Enter", "next", FormAct::Next));
            out.push(save_chord_bind(FormAct::Save, "save"));
        }
        Some(_) => {
            out.push(hint(&[Trigger::ENTER], "Enter", "next", FormAct::Next));
            out.push(save_chord_bind(FormAct::Save, "save"));
        }
        None => {}
    }
    out.push(hint(&[Trigger::ESC], "Esc", "cancel", FormAct::Cancel));
    out
}
fn new_object_binds(focused: Option<NewObjectFocus>) -> Vec<Bind<FormAct>> {
    let mut out = form_base(FormAct::Next, FormAct::Prev);
    match focused {
        Some(NewObjectFocus::Save) => out.push(hint(
            &[Trigger::ENTER, Trigger::Ctrl('s')],
            "Enter",
            "create",
            FormAct::Save,
        )),
        Some(_) => {
            out.push(hint(&[Trigger::ENTER], "Enter", "next", FormAct::Next));
            out.push(save_chord_bind(FormAct::Save, "create"));
        }
        None => {}
    }
    out.push(hint(&[Trigger::ESC], "Esc", "cancel", FormAct::Cancel));
    out
}
fn relationship_binds(focused: Option<RelationshipFocus>) -> Vec<Bind<FormAct>> {
    let mut out = form_base(FormAct::Next, FormAct::Prev);
    match focused {
        Some(RelationshipFocus::Save) => out.push(hint(
            &[Trigger::ENTER, Trigger::Ctrl('s')],
            "Enter",
            "save",
            FormAct::Save,
        )),
        Some(RelationshipFocus::Target) => {
            out.push(hint(
                &[Trigger::ENTER],
                "Enter",
                "pick target",
                FormAct::PickTarget,
            ));
            out.push(save_chord_bind(FormAct::Save, "save"));
        }
        Some(RelationshipFocus::Forward | RelationshipFocus::Reverse) => {
            out.push(hint(&[Trigger::LEFT], "←/→", "change", FormAct::PrevChoice));
            out.push(hidden(
                &[Trigger::RIGHT, Trigger::SPACE],
                "←/→",
                "change",
                FormAct::NextChoice,
            ));
            out.push(hint(&[Trigger::ENTER], "Enter", "next", FormAct::Next));
            out.push(save_chord_bind(FormAct::Save, "save"));
        }
        Some(RelationshipFocus::RefProperties) => {
            out.push(hint(
                &[Trigger::Ctrl('a')],
                "Ctrl-A",
                "add",
                FormAct::AddRef,
            ));
            out.push(hint(&[Trigger::ENTER], "Enter", "edit", FormAct::EditRef));
            out.push(hint(
                &[Trigger::Char('d'), Trigger::Code(KeyCode::Delete)],
                "d",
                "delete",
                FormAct::DeleteRef,
            ));
            out.push(hidden(
                &[Trigger::UP, Trigger::Char('k')],
                "↑/↓",
                "navigate",
                FormAct::Up,
            ));
            out.push(hidden(
                &[Trigger::DOWN, Trigger::Char('j')],
                "↑/↓",
                "navigate",
                FormAct::Down,
            ));
            out.push(save_chord_bind(FormAct::Save, "save"));
        }
        Some(focus) if focus.is_bool() => {
            out.push(hint(
                &[Trigger::ENTER, Trigger::SPACE],
                "Space",
                "toggle",
                FormAct::ToggleBool,
            ));
            out.push(save_chord_bind(FormAct::Save, "save"));
        }
        Some(_) => {
            out.push(hint(&[Trigger::ENTER], "Enter", "next", FormAct::Next));
            out.push(save_chord_bind(FormAct::Save, "save"));
        }
        None => {}
    }
    out.push(hint(&[Trigger::ESC], "Esc", "cancel", FormAct::Cancel));
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SimpleAct {
    Cancel,
    Save,
    Yes,
    No,
    Up,
    Down,
    PageUp,
    PageDown,
}
fn rename_field_binds() -> Vec<Bind<SimpleAct>> {
    vec![
        hint(
            &[Trigger::ENTER, Trigger::Ctrl('s')],
            "Enter",
            "rename",
            SimpleAct::Save,
        ),
        hint(&[Trigger::ESC], "Esc", "cancel", SimpleAct::Cancel),
        hidden(&[Trigger::TAB], "Tab", "navigate", SimpleAct::Up),
        hidden(&[Trigger::BACKTAB], "Shift-Tab", "back", SimpleAct::Down),
    ]
}
fn rename_object_binds() -> Vec<Bind<SimpleAct>> {
    vec![
        hint(
            &[Trigger::ENTER, Trigger::Ctrl('s')],
            "Enter",
            "continue",
            SimpleAct::Save,
        ),
        hint(&[Trigger::ESC], "Esc", "cancel", SimpleAct::Cancel),
    ]
}
fn confirm_binds(yes: &'static str) -> Vec<Bind<SimpleAct>> {
    vec![
        hint(
            &[Trigger::Char('y'), Trigger::Char('Y')],
            "y",
            yes,
            SimpleAct::Yes,
        ),
        hint(
            &[Trigger::Char('n'), Trigger::Char('N'), Trigger::ESC],
            "n/Esc",
            "cancel",
            SimpleAct::No,
        ),
    ]
}
fn delete_field_binds() -> Vec<Bind<SimpleAct>> {
    confirm_binds("delete")
}
fn delete_object_binds() -> Vec<Bind<SimpleAct>> {
    confirm_binds("delete")
}
fn rename_object_confirm_binds() -> Vec<Bind<SimpleAct>> {
    confirm_binds("rename")
}
fn relationship_target_binds() -> Vec<Bind<SimpleAct>> {
    vec![
        hint(&[Trigger::ENTER], "Enter", "choose target", SimpleAct::Save),
        hint(&[Trigger::UP], "↑/↓", "navigate", SimpleAct::Up),
        hidden(&[Trigger::DOWN], "↑/↓", "navigate", SimpleAct::Down),
        hidden(
            &[Trigger::Code(KeyCode::PageUp)],
            "PgUp",
            "move by page",
            SimpleAct::PageUp,
        ),
        hidden(
            &[Trigger::Code(KeyCode::PageDown)],
            "PgDn",
            "move by page",
            SimpleAct::PageDown,
        ),
        hint(&[Trigger::ESC], "Esc", "back", SimpleAct::Cancel),
    ]
}
fn add_hook_binds() -> Vec<Bind<SimpleAct>> {
    vec![
        hint(&[Trigger::ENTER], "Enter", "register hook", SimpleAct::Save),
        hidden(
            &[Trigger::Ctrl('s')],
            "^S",
            "register hook",
            SimpleAct::Save,
        ),
        hint(
            &[Trigger::UP, Trigger::Char('k')],
            "↑/↓",
            "navigate",
            SimpleAct::Up,
        ),
        hidden(
            &[Trigger::DOWN, Trigger::Char('j')],
            "↑/↓",
            "navigate",
            SimpleAct::Down,
        ),
        hidden(
            &[Trigger::Code(KeyCode::PageUp)],
            "PgUp",
            "move by page",
            SimpleAct::PageUp,
        ),
        hidden(
            &[Trigger::Code(KeyCode::PageDown)],
            "PgDn",
            "move by page",
            SimpleAct::PageDown,
        ),
        hint(&[Trigger::ESC], "Esc", "cancel", SimpleAct::Cancel),
    ]
}

fn handle_search_key(app: &mut App, key: KeyEvent) {
    if let Some(act) = Bind::resolve(&search_binds(), &key) {
        run_search(app, act);
        return;
    }

    let before = app.managed.query.value().to_string();
    if app.managed.query.handle_key(&key) && app.managed.query.value() != before {
        app.managed.selected = 0;
        app.managed.scroll = 0;
        app.managed.property_selected = 0;
    }
}
fn handle_add_choose_key(app: &mut App, key: KeyEvent) {
    if let Some(act) = Bind::resolve(&add_choose_binds(), &key) {
        run_add_choose(app, act);
    }
}

fn handle_rename_field_key(app: &mut App, key: KeyEvent) {
    if let Some(act) = Bind::resolve(&rename_field_binds(), &key) {
        match act {
            SimpleAct::Save => ops::commit_rename_field(app),
            SimpleAct::Cancel => ops::cancel_active_draft(app),
            SimpleAct::Up | SimpleAct::Down => {
                if let Some(rename) = app.managed.renaming.as_mut() {
                    ops::advance_rename_field_focus(rename, act == SimpleAct::Up);
                }
            }
            _ => {}
        }
        return;
    }
    match key.code {
        KeyCode::Esc => ops::cancel_active_draft(app),
        KeyCode::Tab | KeyCode::BackTab => {
            if let Some(rename) = app.managed.renaming.as_mut() {
                ops::advance_rename_field_focus(rename, key.code == KeyCode::Tab);
            }
        }
        KeyCode::Enter => ops::commit_rename_field(app),
        _ => {
            if let Some(rename) = app.managed.renaming.as_mut() {
                rename.error = None;
                rename.key.handle_key(&key);
            }
        }
    }
}

fn handle_rename_object_key(app: &mut App, key: KeyEvent) {
    if let Some(act) = Bind::resolve(&rename_object_binds(), &key) {
        if act == SimpleAct::Cancel {
            ops::cancel_active_draft(app);
            return;
        }
        if act != SimpleAct::Save {
            return;
        }
        let Some(mut draft) = app.managed.renaming_object.take() else {
            return;
        };
        let existing = crate::managed::api::objects(&draft.original_doc)
            .map(|objects| {
                objects
                    .iter()
                    .filter_map(|object| {
                        object
                            .get("name")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    })
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        if let Err(error) = crate::managed::state::validate_object_name(
            &draft.key.value,
            &existing,
            &draft.old_name,
        ) {
            draft.error = Some(error);
            app.managed.renaming_object = Some(draft);
            return;
        }
        ops::start_record_count(app, draft);
        return;
    }
    if let Some(draft) = app.managed.renaming_object.as_mut() {
        draft.error = None;
        draft.key.handle_key(&key);
    }
}

fn handle_rename_object_confirm_key(app: &mut App, key: KeyEvent) {
    if let Some(act) = Bind::resolve(&rename_object_confirm_binds(), &key) {
        if act == SimpleAct::No {
            ops::cancel_active_draft(app);
            return;
        }
        if act != SimpleAct::Yes {
            return;
        }
        if app
            .active_tenant()
            .is_some_and(|tenant| tenant.theme == crate::config::tenant::TenantTheme::Production)
        {
            let Some(confirm) = app.managed.rename_object_confirm.as_ref() else {
                return;
            };
            let request = ops::RenameObjectRequest {
                tenant_name: confirm.draft.tenant_name.clone(),
                old_name: confirm.draft.old_name.clone(),
                new_name: confirm.draft.key.value.clone(),
                previous_doc: confirm.draft.original_doc.clone(),
            };
            app.prod_confirm.pending = Some(crate::app::prod_confirm::PendingProdAction::Managed(
                ops::ProdAction::RenameObject(Box::new(request)),
            ));
            app.input_mode = InputMode::ProdConfirm;
        } else {
            let Some(confirm) = app.managed.rename_object_confirm.take() else {
                return;
            };
            let request = ops::RenameObjectRequest {
                tenant_name: confirm.draft.tenant_name,
                old_name: confirm.draft.old_name,
                new_name: confirm.draft.key.value,
                previous_doc: confirm.draft.original_doc,
            };
            ops::execute_rename_object(app, request, false);
        }
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') => ops::cancel_active_draft(app),
        KeyCode::Char('y') => {
            if app.active_tenant().is_some_and(|tenant| {
                tenant.theme == crate::config::tenant::TenantTheme::Production
            }) {
                let Some(confirm) = app.managed.rename_object_confirm.as_ref() else {
                    return;
                };
                let request = ops::RenameObjectRequest {
                    tenant_name: confirm.draft.tenant_name.clone(),
                    old_name: confirm.draft.old_name.clone(),
                    new_name: confirm.draft.key.value.clone(),
                    previous_doc: confirm.draft.original_doc.clone(),
                };
                app.prod_confirm.pending =
                    Some(crate::app::prod_confirm::PendingProdAction::Managed(
                        ops::ProdAction::RenameObject(Box::new(request)),
                    ));
                app.input_mode = InputMode::ProdConfirm;
            } else {
                let Some(confirm) = app.managed.rename_object_confirm.take() else {
                    return;
                };
                let request = ops::RenameObjectRequest {
                    tenant_name: confirm.draft.tenant_name,
                    old_name: confirm.draft.old_name,
                    new_name: confirm.draft.key.value,
                    previous_doc: confirm.draft.original_doc,
                };
                ops::execute_rename_object(app, request, false);
            }
        }
        _ => {}
    }
}

fn handle_new_object_key(app: &mut App, key: KeyEvent) {
    let Some(focused) = app.managed.new_object.as_ref().map(|draft| draft.focused) else {
        app.input_mode = InputMode::Normal;
        return;
    };
    if let Some(act) = Bind::resolve(&new_object_binds(Some(focused)), &key) {
        run_new_object(app, act);
        return;
    }
    match key.code {
        KeyCode::Esc => {
            ops::cancel_active_draft(app);
            return;
        }
        KeyCode::Tab => {
            if let Some(draft) = app.managed.new_object.as_mut() {
                draft.focused = draft.focused.next();
            }
            return;
        }
        KeyCode::BackTab => {
            if let Some(draft) = app.managed.new_object.as_mut() {
                draft.focused = draft.focused.prev();
            }
            return;
        }
        KeyCode::Enter if focused == NewObjectFocus::Save => {
            commit_new_object(app);
            return;
        }
        KeyCode::Enter => {
            if let Some(draft) = app.managed.new_object.as_mut() {
                draft.focused = draft.focused.next();
            }
            return;
        }
        _ => {}
    }
    let Some(draft) = app.managed.new_object.as_mut() else {
        return;
    };
    draft.error = None;
    match focused {
        NewObjectFocus::Name => {
            draft.name.handle_key(&key);
        }
        NewObjectFocus::Title => {
            draft.title.handle_key(&key);
        }
        NewObjectFocus::Description => {
            draft.description.handle_key(&key);
        }
        NewObjectFocus::Save => {}
    }
}

fn run_new_object(app: &mut App, act: FormAct) {
    match act {
        FormAct::Cancel => ops::cancel_active_draft(app),
        FormAct::Save => commit_new_object(app),
        FormAct::Next | FormAct::Prev => {
            if let Some(d) = app.managed.new_object.as_mut() {
                d.focused = if act == FormAct::Next {
                    d.focused.next()
                } else {
                    d.focused.prev()
                };
            }
        }
        _ => {}
    }
}

/// Validate the new-object draft and either queue a prod confirmation or write
/// it. The draft is taken out so a name-validation failure can put it back with
/// the error attached; the prod-confirm branch puts it back too, because the
/// user may still cancel at the confirmation and expect their input intact.
fn commit_new_object(app: &mut App) {
    let Some(mut draft) = app.managed.new_object.take() else {
        return;
    };
    let existing = crate::managed::api::objects(&draft.original_doc)
        .map(|objects| {
            objects
                .iter()
                .filter_map(|object| {
                    object
                        .get("name")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if let Err(error) =
        crate::managed::state::validate_object_name(&draft.name.value, &existing, "")
    {
        draft.error = Some(error);
        app.managed.new_object = Some(draft);
        return;
    }
    let request = ops::CreateObjectRequest {
        tenant_name: draft.tenant_name.clone(),
        name: draft.name.value.clone(),
        title: draft.title.value.clone(),
        description: draft.description.value.clone(),
        previous_doc: draft.original_doc.clone(),
    };
    if app
        .active_tenant()
        .is_some_and(|tenant| tenant.theme == crate::config::tenant::TenantTheme::Production)
    {
        app.managed.new_object = Some(draft);
        app.prod_confirm.pending = Some(crate::app::prod_confirm::PendingProdAction::Managed(
            ops::ProdAction::CreateObject(Box::new(request)),
        ));
        app.input_mode = InputMode::ProdConfirm;
    } else {
        ops::execute_create_object(app, request, false);
    }
}

/// What a key does in the field-edit form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditFieldAct {
    Cancel,
    FocusNext,
    FocusPrev,
    Save,
    ToggleBool,
}

/// The field-edit form's bindings for `focused`.
///
/// This is the single description of the form's keys: `handle_edit_key`
/// dispatches through it and both hint renderers read it, so the footer cannot
/// advertise a key the handler ignores (or miss one it honours).
///
/// A pure function of focus rather than of `&App`, which is what makes it
/// testable — `App::new()` reads config from disk.
fn edit_field_binds(focused: Option<EditFieldFocus>) -> Vec<Bind<EditFieldAct>> {
    use EditFieldAct::*;
    let mut out = vec![
        hint(&[Trigger::TAB], "Tab", "navigate", FocusNext),
        // Dispatch-only: Shift-Tab has never been advertised here.
        hidden(&[Trigger::BACKTAB], "Shift-Tab", "back", FocusPrev),
    ];
    match focused {
        // On the Save row `Enter` and `^S` do the same thing, so they share one
        // binding and the footer names the discoverable one.
        Some(EditFieldFocus::Save) => out.push(hint(
            &[Trigger::ENTER, Trigger::Ctrl('s')],
            "Enter",
            "save",
            Save,
        )),
        Some(focus) if focus.is_bool() => {
            out.push(hint(
                &[Trigger::SPACE, Trigger::LEFT, Trigger::RIGHT],
                "Space",
                "toggle",
                ToggleBool,
            ));
            out.push(hint(&[Trigger::ENTER], "Enter", "toggle", ToggleBool));
            out.push(save_chord_bind(Save, "save"));
        }
        Some(_) => {
            out.push(hint(&[Trigger::ENTER], "Enter", "next", FocusNext));
            out.push(save_chord_bind(Save, "save"));
        }
        None => {}
    }
    out.push(hint(&[Trigger::ESC], "Esc", "cancel", Cancel));
    out
}

fn run_edit_field(app: &mut App, act: EditFieldAct) {
    match act {
        EditFieldAct::Cancel => ops::cancel_active_draft(app),
        EditFieldAct::Save => ops::commit_edit(app),
        EditFieldAct::FocusNext | EditFieldAct::FocusPrev => {
            if let Some(edit) = app.managed.editing.as_mut() {
                ops::advance_focus(edit, act == EditFieldAct::FocusNext);
            }
        }
        EditFieldAct::ToggleBool => {
            if let Some(edit) = app.managed.editing.as_mut() {
                edit.toggle_focused_bool();
                edit.error = None;
            }
        }
    }
}

fn handle_edit_key(app: &mut App, key: KeyEvent) {
    let Some(focused) = app.managed.editing.as_ref().map(|edit| edit.focused) else {
        app.input_mode = InputMode::Normal;
        return;
    };

    if let Some(act) = Bind::resolve(&edit_field_binds(Some(focused)), &key) {
        run_edit_field(app, act);
        return;
    }

    // Nothing in the table claimed the key, so it's text input for whichever
    // field has focus.
    let Some(edit) = app.managed.editing.as_mut() else {
        return;
    };
    edit.error = None;
    match focused {
        EditFieldFocus::Key if edit.caps.rename_key => {
            edit.key.handle_key(&key);
        }
        EditFieldFocus::Title => {
            edit.title.handle_key(&key);
        }
        EditFieldFocus::Description => {
            edit.description.handle_key(&key);
        }
        _ => {}
    }
}

fn handle_add_field_key(app: &mut App, key: KeyEvent) {
    let Some(focused) = app.managed.add_field.as_ref().map(|draft| draft.focused) else {
        app.input_mode = InputMode::Normal;
        return;
    };
    if let Some(act) = Bind::resolve(&add_field_binds(Some(focused)), &key) {
        run_add_field(app, act);
        return;
    }
    match key.code {
        KeyCode::Esc => {
            ops::cancel_active_draft(app);
            return;
        }
        KeyCode::Tab => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                ops::advance_add_field_focus(draft, true);
            }
            return;
        }
        KeyCode::BackTab => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                ops::advance_add_field_focus(draft, false);
            }
            return;
        }
        KeyCode::Enter if focused == AddFieldFocus::Save => {
            ops::commit_add_field(app);
            return;
        }
        KeyCode::Enter if focused == AddFieldFocus::Type => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                draft.field_type = draft.field_type.next();
                draft.error = None;
            }
            return;
        }
        KeyCode::Left if focused == AddFieldFocus::Type => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                draft.field_type = draft.field_type.prev();
                draft.error = None;
            }
            return;
        }
        KeyCode::Right if focused == AddFieldFocus::Type => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                draft.field_type = draft.field_type.next();
                draft.error = None;
            }
            return;
        }
        KeyCode::Enter if focused.is_bool() => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                draft.toggle_focused_bool();
                draft.error = None;
            }
            return;
        }
        KeyCode::Char(' ') if focused == AddFieldFocus::Type => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                draft.field_type = draft.field_type.next();
                draft.error = None;
            }
            return;
        }
        KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if focused.is_bool() => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                draft.toggle_focused_bool();
                draft.error = None;
            }
            return;
        }
        KeyCode::Enter => {
            if let Some(draft) = app.managed.add_field.as_mut() {
                ops::advance_add_field_focus(draft, true);
            }
            return;
        }
        _ => {}
    }

    let Some(draft) = app.managed.add_field.as_mut() else {
        return;
    };
    draft.error = None;
    match focused {
        AddFieldFocus::Key => {
            draft.key.handle_key(&key);
        }
        AddFieldFocus::Title => {
            draft.title.handle_key(&key);
        }
        AddFieldFocus::Description => {
            draft.description.handle_key(&key);
        }
        _ => {}
    }
}

fn run_add_field(app: &mut App, act: FormAct) {
    match act {
        FormAct::Cancel => ops::cancel_active_draft(app),
        FormAct::Save => ops::commit_add_field(app),
        FormAct::Next | FormAct::Prev => {
            if let Some(d) = app.managed.add_field.as_mut() {
                ops::advance_add_field_focus(d, act == FormAct::Next);
            }
        }
        FormAct::ToggleBool => {
            if let Some(d) = app.managed.add_field.as_mut() {
                d.toggle_focused_bool();
                d.error = None;
            }
        }
        FormAct::NextChoice | FormAct::PrevChoice => {
            if let Some(d) = app.managed.add_field.as_mut() {
                d.field_type = if act == FormAct::NextChoice {
                    d.field_type.next()
                } else {
                    d.field_type.prev()
                };
                d.error = None;
            }
        }
        _ => {}
    }
}

fn handle_relationship_key(app: &mut App, key: KeyEvent) {
    let Some(focused) = app
        .managed
        .relationship_form
        .as_ref()
        .map(|draft| draft.focused)
    else {
        app.input_mode = InputMode::Normal;
        return;
    };
    if let Some(act) = Bind::resolve(&relationship_binds(Some(focused)), &key) {
        run_relationship(app, act);
        return;
    }
    if focused == RelationshipFocus::RefProperties {
        match key.code {
            KeyCode::Esc => ops::cancel_active_draft(app),
            KeyCode::Tab => {
                if let Some(draft) = app.managed.relationship_form.as_mut() {
                    draft.focused = draft.focused.next(draft.reverse);
                }
            }
            KeyCode::BackTab => {
                if let Some(draft) = app.managed.relationship_form.as_mut() {
                    draft.focused = draft.focused.prev(draft.reverse);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(draft) = app.managed.relationship_form.as_mut() {
                    draft.ref_selected = draft.ref_selected.saturating_sub(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(draft) = app.managed.relationship_form.as_mut() {
                    draft.ref_selected =
                        (draft.ref_selected + 1).min(draft.ref_properties.len().saturating_sub(1));
                }
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(draft) = app.managed.relationship_form.as_mut() {
                    draft.remove_selected_ref_property();
                    draft.error = None;
                }
            }
            KeyCode::Enter => {
                let ref_prop_draft = app.managed.relationship_form.as_ref().and_then(|draft| {
                    draft
                        .ref_properties
                        .get(draft.ref_selected)
                        .map(|property| RefPropDraft::edit(draft.ref_selected, property))
                });
                if let Some(ref_prop_draft) = ref_prop_draft {
                    app.managed.ref_prop_draft = Some(ref_prop_draft);
                    app.input_mode = InputMode::Managed(Mode::RefProp);
                } else if let Some(draft) = app.managed.relationship_form.as_mut() {
                    draft.focused = draft.focused.next(draft.reverse);
                }
            }
            _ => {}
        }
        return;
    }
    match key.code {
        KeyCode::Esc => {
            ops::cancel_active_draft(app);
            return;
        }
        KeyCode::Tab => {
            if let Some(draft) = app.managed.relationship_form.as_mut() {
                draft.focused = draft.focused.next(draft.reverse);
            }
            return;
        }
        KeyCode::BackTab => {
            if let Some(draft) = app.managed.relationship_form.as_mut() {
                draft.focused = draft.focused.prev(draft.reverse);
            }
            return;
        }
        KeyCode::Enter if focused == RelationshipFocus::Save => {
            ops::commit_relationship(app);
            return;
        }
        KeyCode::Enter if focused == RelationshipFocus::Target => {
            app.input_mode = InputMode::Managed(Mode::RelationshipTarget);
            return;
        }
        KeyCode::Enter if focused.is_bool() => {
            if let Some(draft) = app.managed.relationship_form.as_mut() {
                toggle_relationship_bool(draft);
                draft.error = None;
            }
            return;
        }
        KeyCode::Char(' ') if focused.is_bool() => {
            if let Some(draft) = app.managed.relationship_form.as_mut() {
                toggle_relationship_bool(draft);
                draft.error = None;
            }
            return;
        }
        KeyCode::Left | KeyCode::Right
            if matches!(
                focused,
                RelationshipFocus::Forward | RelationshipFocus::Reverse
            ) =>
        {
            if let Some(draft) = app.managed.relationship_form.as_mut() {
                if focused == RelationshipFocus::Forward {
                    draft.forward = if key.code == KeyCode::Left {
                        draft.forward.prev()
                    } else {
                        draft.forward.next()
                    };
                } else {
                    draft.reverse = if key.code == KeyCode::Left {
                        draft.reverse.prev()
                    } else {
                        draft.reverse.next()
                    };
                    if draft.reverse == crate::managed::state::ReverseCardinality::None
                        && draft.focused == RelationshipFocus::ReverseKey
                    {
                        draft.focused = RelationshipFocus::Searchable;
                    }
                }
                draft.error = None;
            }
            return;
        }
        KeyCode::Char(' ')
            if matches!(
                focused,
                RelationshipFocus::Forward | RelationshipFocus::Reverse
            ) =>
        {
            if let Some(draft) = app.managed.relationship_form.as_mut() {
                if focused == RelationshipFocus::Forward {
                    draft.forward = draft.forward.next();
                } else {
                    draft.reverse = draft.reverse.next();
                }
                draft.error = None;
            }
            return;
        }
        KeyCode::Enter => {
            if let Some(draft) = app.managed.relationship_form.as_mut() {
                draft.focused = draft.focused.next(draft.reverse);
            }
            return;
        }
        _ => {}
    }

    let Some(draft) = app.managed.relationship_form.as_mut() else {
        return;
    };
    draft.error = None;
    match focused {
        RelationshipFocus::Key => {
            draft.key.handle_key(&key);
        }
        RelationshipFocus::Title => {
            draft.title.handle_key(&key);
        }
        RelationshipFocus::Description => {
            draft.description.handle_key(&key);
        }
        RelationshipFocus::ReverseKey => {
            draft.reverse_key.handle_key(&key);
        }
        _ => {}
    }
}

fn run_relationship(app: &mut App, act: FormAct) {
    match act {
        FormAct::Cancel => ops::cancel_active_draft(app),
        FormAct::Save => ops::commit_relationship(app),
        FormAct::Next | FormAct::Prev => {
            if let Some(d) = app.managed.relationship_form.as_mut() {
                d.focused = if act == FormAct::Next {
                    d.focused.next(d.reverse)
                } else {
                    d.focused.prev(d.reverse)
                };
            }
        }
        FormAct::PickTarget => app.input_mode = InputMode::Managed(Mode::RelationshipTarget),
        FormAct::AddRef => {
            app.managed.ref_prop_draft = Some(RefPropDraft::new_add());
            app.input_mode = InputMode::Managed(Mode::RefProp);
        }
        FormAct::EditRef => {
            let draft = app.managed.relationship_form.as_ref().and_then(|d| {
                d.ref_properties
                    .get(d.ref_selected)
                    .map(|p| RefPropDraft::edit(d.ref_selected, p))
            });
            if let Some(d) = draft {
                app.managed.ref_prop_draft = Some(d);
                app.input_mode = InputMode::Managed(Mode::RefProp);
            } else if let Some(d) = app.managed.relationship_form.as_mut() {
                d.focused = d.focused.next(d.reverse);
            }
        }
        FormAct::DeleteRef => {
            if let Some(d) = app.managed.relationship_form.as_mut() {
                d.remove_selected_ref_property();
                d.error = None;
            }
        }
        FormAct::Up => {
            if let Some(d) = app.managed.relationship_form.as_mut() {
                d.ref_selected = d.ref_selected.saturating_sub(1);
            }
        }
        FormAct::Down => {
            if let Some(d) = app.managed.relationship_form.as_mut() {
                d.ref_selected = (d.ref_selected + 1).min(d.ref_properties.len().saturating_sub(1));
            }
        }
        FormAct::ToggleBool => {
            if let Some(d) = app.managed.relationship_form.as_mut() {
                toggle_relationship_bool(d);
                d.error = None;
            }
        }
        FormAct::NextChoice | FormAct::PrevChoice => {
            if let Some(d) = app.managed.relationship_form.as_mut() {
                let next = act == FormAct::NextChoice;
                if d.focused == RelationshipFocus::Forward {
                    d.forward = if next {
                        d.forward.next()
                    } else {
                        d.forward.prev()
                    };
                } else {
                    d.reverse = if next {
                        d.reverse.next()
                    } else {
                        d.reverse.prev()
                    };
                    if d.reverse == crate::managed::state::ReverseCardinality::None
                        && d.focused == RelationshipFocus::ReverseKey
                    {
                        d.focused = RelationshipFocus::Searchable;
                    }
                }
                d.error = None;
            }
        }
    }
}

fn handle_ref_prop_key(app: &mut App, key: KeyEvent) {
    let Some(focused) = app
        .managed
        .ref_prop_draft
        .as_ref()
        .map(|draft| draft.focused)
    else {
        app.input_mode = InputMode::Managed(Mode::Relationship);
        return;
    };
    if let Some(act) = Bind::resolve(&ref_prop_binds(Some(focused)), &key) {
        run_ref_prop(app, act);
        return;
    }
    match key.code {
        KeyCode::Esc => {
            app.managed.ref_prop_draft = None;
            app.input_mode = InputMode::Managed(Mode::Relationship);
            return;
        }
        KeyCode::Tab => {
            if let Some(draft) = app.managed.ref_prop_draft.as_mut() {
                draft.focused = draft.focused.next();
            }
            return;
        }
        KeyCode::BackTab => {
            if let Some(draft) = app.managed.ref_prop_draft.as_mut() {
                draft.focused = draft.focused.prev();
            }
            return;
        }
        KeyCode::Left if focused == RefPropFocus::Type => {
            if let Some(draft) = app.managed.ref_prop_draft.as_mut() {
                draft.kind = draft.kind.prev();
                draft.error = None;
            }
            return;
        }
        KeyCode::Right | KeyCode::Char(' ') if focused == RefPropFocus::Type => {
            if let Some(draft) = app.managed.ref_prop_draft.as_mut() {
                draft.kind = draft.kind.next();
                draft.error = None;
            }
            return;
        }
        KeyCode::Enter if focused == RefPropFocus::Save => {
            commit_ref_prop_draft(app);
            return;
        }
        KeyCode::Enter => {
            if let Some(draft) = app.managed.ref_prop_draft.as_mut() {
                draft.focused = draft.focused.next();
            }
            return;
        }
        _ => {}
    }

    let Some(draft) = app.managed.ref_prop_draft.as_mut() else {
        return;
    };
    draft.error = None;
    match focused {
        RefPropFocus::Name => {
            draft.name.handle_key(&key);
        }
        RefPropFocus::Label => {
            draft.label.handle_key(&key);
        }
        RefPropFocus::Type | RefPropFocus::Save => {}
    }
}

/// Fold the draft `_refProperties` sub-property back into the relationship
/// form it belongs to. The draft is taken out so a validation failure can put
/// it back with the error attached rather than leaving a half-committed one.
fn commit_ref_prop_draft(app: &mut App) {
    let Some(mut draft) = app.managed.ref_prop_draft.take() else {
        return;
    };
    let result = app
        .managed
        .relationship_form
        .as_mut()
        .ok_or_else(|| "Relationship form is no longer active".to_string())
        .and_then(|form| ops::commit_ref_prop(form, &draft));
    match result {
        Ok(()) => app.input_mode = InputMode::Managed(Mode::Relationship),
        Err(error) => {
            draft.error = Some(error);
            app.managed.ref_prop_draft = Some(draft);
        }
    }
}

fn run_ref_prop(app: &mut App, act: FormAct) {
    match act {
        FormAct::Cancel => {
            app.managed.ref_prop_draft = None;
            app.input_mode = InputMode::Managed(Mode::Relationship);
        }
        FormAct::Save => commit_ref_prop_draft(app),
        FormAct::Next | FormAct::Prev => {
            if let Some(d) = app.managed.ref_prop_draft.as_mut() {
                d.focused = if act == FormAct::Next {
                    d.focused.next()
                } else {
                    d.focused.prev()
                };
            }
        }
        FormAct::NextChoice | FormAct::PrevChoice => {
            if let Some(d) = app.managed.ref_prop_draft.as_mut() {
                d.kind = if act == FormAct::NextChoice {
                    d.kind.next()
                } else {
                    d.kind.prev()
                };
                d.error = None;
            }
        }
        _ => {}
    }
}

fn toggle_relationship_bool(draft: &mut RelationshipFormState) {
    match draft.focused {
        RelationshipFocus::Searchable => draft.searchable = !draft.searchable,
        RelationshipFocus::Viewable => draft.viewable = !draft.viewable,
        RelationshipFocus::UserEditable => draft.user_editable = !draft.user_editable,
        RelationshipFocus::Required => draft.required = !draft.required,
        RelationshipFocus::Validate => draft.validate = !draft.validate,
        _ => {}
    }
}

fn handle_relationship_target_key(app: &mut App, key: KeyEvent) {
    if let Some(act) = Bind::resolve(&relationship_target_binds(), &key) {
        match act {
            SimpleAct::Cancel => app.input_mode = InputMode::Managed(Mode::Relationship),
            SimpleAct::Save => {
                let matches = relationship_target_matches(app);
                let selected = app
                    .managed
                    .relationship_form
                    .as_ref()
                    .map(|d| d.target_selected.min(matches.len().saturating_sub(1)))
                    .unwrap_or(0);
                if let Some(name) = matches.get(selected).cloned() {
                    if let Some(d) = app.managed.relationship_form.as_mut() {
                        d.target_name = Some(name);
                        d.focused = RelationshipFocus::Target;
                        d.error = None;
                    }
                    app.input_mode = InputMode::Managed(Mode::Relationship);
                }
            }
            SimpleAct::Up => move_relationship_target(app, -1),
            SimpleAct::Down => move_relationship_target(app, 1),
            SimpleAct::PageUp => move_relationship_target(app, -10),
            SimpleAct::PageDown => move_relationship_target(app, 10),
            _ => {}
        }
        return;
    }
    match key.code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Managed(Mode::Relationship);
            return;
        }
        KeyCode::Enter => {
            let matches = relationship_target_matches(app);
            let selected = app
                .managed
                .relationship_form
                .as_ref()
                .map(|draft| draft.target_selected.min(matches.len().saturating_sub(1)))
                .unwrap_or(0);
            if let Some(name) = matches.get(selected).cloned() {
                if let Some(draft) = app.managed.relationship_form.as_mut() {
                    draft.target_name = Some(name);
                    draft.focused = RelationshipFocus::Target;
                    draft.error = None;
                }
                app.input_mode = InputMode::Managed(Mode::Relationship);
            }
            return;
        }
        KeyCode::Up => {
            move_relationship_target(app, -1);
            return;
        }
        KeyCode::Down => {
            move_relationship_target(app, 1);
            return;
        }
        KeyCode::PageUp => {
            move_relationship_target(app, -10);
            return;
        }
        KeyCode::PageDown => {
            move_relationship_target(app, 10);
            return;
        }
        _ => {}
    }

    let before = app
        .managed
        .relationship_form
        .as_ref()
        .map(|draft| draft.target_query.value().to_string())
        .unwrap_or_default();
    if let Some(draft) = app.managed.relationship_form.as_mut() {
        if draft.target_query.handle_key(&key) && draft.target_query.value() != before {
            draft.target_selected = 0;
        }
    }
}

fn move_relationship_target(app: &mut App, delta: isize) {
    let n = relationship_target_matches(app).len();
    if n == 0 {
        return;
    }
    if let Some(draft) = app.managed.relationship_form.as_mut() {
        let cur = draft.target_selected.min(n - 1) as isize;
        draft.target_selected = (cur + delta).clamp(0, n as isize - 1) as usize;
    }
}

fn handle_add_hook_key(app: &mut App, key: KeyEvent) {
    if let Some(act) = Bind::resolve(&add_hook_binds(), &key) {
        match act {
            SimpleAct::Cancel => ops::cancel_active_draft(app),
            SimpleAct::Save => ops::commit_add_hook(app),
            SimpleAct::Up => move_hook(app, -1),
            SimpleAct::Down => move_hook(app, 1),
            SimpleAct::PageUp => move_hook(app, -10),
            SimpleAct::PageDown => move_hook(app, 10),
            _ => {}
        };
        return;
    }
    match key.code {
        KeyCode::Esc => {
            ops::cancel_active_draft(app);
        }
        KeyCode::Enter => {
            ops::commit_add_hook(app);
        }
        KeyCode::Up | KeyCode::Char('k') => move_hook(app, -1),
        KeyCode::Down | KeyCode::Char('j') => move_hook(app, 1),
        KeyCode::PageUp => move_hook(app, -10),
        KeyCode::PageDown => move_hook(app, 10),
        _ => {}
    }
}

fn move_hook(app: &mut App, delta: isize) {
    let Some(draft) = app.managed.add_hook.as_mut() else {
        return;
    };
    if draft.events.is_empty() {
        return;
    }
    let cur = draft.selected.min(draft.events.len() - 1) as isize;
    draft.selected = (cur + delta).clamp(0, draft.events.len() as isize - 1) as usize;
}

fn handle_delete_confirm_key(app: &mut App, key: KeyEvent) {
    if let Some(act) = Bind::resolve(&delete_field_binds(), &key) {
        match act {
            SimpleAct::Yes => ops::commit_delete_field(app),
            SimpleAct::No => {
                app.managed.pending_delete = None;
                app.input_mode = InputMode::Normal;
            }
            _ => {}
        };
        return;
    }
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => ops::commit_delete_field(app),
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.managed.pending_delete = None;
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
}

fn handle_delete_object_confirm_key(app: &mut App, key: KeyEvent) {
    if let Some(act) = Bind::resolve(&delete_object_binds(), &key) {
        if act == SimpleAct::No {
            ops::cancel_active_draft(app);
            return;
        }
        if act == SimpleAct::Yes {
            let Some(draft) = app.managed.pending_object_delete.as_ref() else {
                return;
            };
            let request = ops::DeleteObjectRequest {
                tenant_name: draft.tenant_name.clone(),
                object_name: draft.object_name.clone(),
                previous_doc: draft.original_doc.clone(),
            };
            if app.active_tenant().is_some_and(|tenant| {
                tenant.theme == crate::config::tenant::TenantTheme::Production
            }) {
                app.prod_confirm.pending =
                    Some(crate::app::prod_confirm::PendingProdAction::Managed(
                        ops::ProdAction::DeleteObject(Box::new(request)),
                    ));
                app.input_mode = InputMode::ProdConfirm;
            } else {
                ops::execute_delete_object(app, request, false);
            }
            return;
        }
    }
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let Some(draft) = app.managed.pending_object_delete.as_ref() else {
                return;
            };
            let request = ops::DeleteObjectRequest {
                tenant_name: draft.tenant_name.clone(),
                object_name: draft.object_name.clone(),
                previous_doc: draft.original_doc.clone(),
            };
            if app.active_tenant().is_some_and(|tenant| {
                tenant.theme == crate::config::tenant::TenantTheme::Production
            }) {
                app.prod_confirm.pending =
                    Some(crate::app::prod_confirm::PendingProdAction::Managed(
                        ops::ProdAction::DeleteObject(Box::new(request)),
                    ));
                app.input_mode = InputMode::ProdConfirm;
            } else {
                ops::execute_delete_object(app, request, false);
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => ops::cancel_active_draft(app),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn resolve(focus: EditFieldFocus, key: KeyEvent) -> Option<EditFieldAct> {
        Bind::resolve(&edit_field_binds(Some(focus)), &key)
    }

    /// Every variant, so the property test below can't silently skip one.
    const ALL_FOCUS: [EditFieldFocus; 8] = [
        EditFieldFocus::Key,
        EditFieldFocus::Title,
        EditFieldFocus::Description,
        EditFieldFocus::Required,
        EditFieldFocus::Searchable,
        EditFieldFocus::Viewable,
        EditFieldFocus::UserEditable,
        EditFieldFocus::Save,
    ];

    #[test]
    fn save_chord_saves_from_every_focus() {
        for focus in ALL_FOCUS {
            assert_eq!(
                resolve(focus, ctrl(KeyCode::Char('s'))),
                Some(EditFieldAct::Save),
                "^S should save with {focus:?} focused"
            );
        }
    }

    #[test]
    fn enter_saves_only_on_the_save_row() {
        assert_eq!(
            resolve(EditFieldFocus::Save, key(KeyCode::Enter)),
            Some(EditFieldAct::Save)
        );
        assert_eq!(
            resolve(EditFieldFocus::Title, key(KeyCode::Enter)),
            Some(EditFieldAct::FocusNext)
        );
        assert_eq!(
            resolve(EditFieldFocus::Required, key(KeyCode::Enter)),
            Some(EditFieldAct::ToggleBool)
        );
    }

    /// The rule the footer is supposed to follow: `^S` earns its width exactly
    /// where `Enter` won't save. Both sides come from the one table, so this
    /// can't drift the way the hand-written footer did.
    #[test]
    fn save_chord_is_advertised_exactly_where_enter_does_not_save() {
        for focus in ALL_FOCUS {
            let binds = edit_field_binds(Some(focus));
            let enter_saves =
                Bind::resolve(&binds, &key(KeyCode::Enter)) == Some(EditFieldAct::Save);
            let advertises_chord = Bind::footer_hints(&binds)
                .iter()
                .any(|(label, _)| *label == "^S");
            assert_ne!(
                enter_saves, advertises_chord,
                "{focus:?}: Enter-saves={enter_saves} but ^S-in-footer={advertises_chord}"
            );
        }
    }

    #[test]
    fn bool_rows_toggle_on_space_and_arrows() {
        for code in [KeyCode::Char(' '), KeyCode::Left, KeyCode::Right] {
            assert_eq!(
                resolve(EditFieldFocus::Searchable, key(code)),
                Some(EditFieldAct::ToggleBool),
                "{code:?} should toggle a bool row"
            );
        }
    }

    /// Arrows and Space must stay unbound on a text row so they reach the
    /// `TextField` as cursor movement and input.
    #[test]
    fn text_rows_leave_editing_keys_to_the_field() {
        for code in [
            KeyCode::Char(' '),
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Char('a'),
            KeyCode::Backspace,
        ] {
            assert_eq!(
                resolve(EditFieldFocus::Title, key(code)),
                None,
                "{code:?} should fall through to the focused field"
            );
        }
    }

    #[test]
    fn every_footer_hint_has_a_binding_that_fires() {
        for focus in ALL_FOCUS {
            let binds = edit_field_binds(Some(focus));
            for bind in binds.iter().filter(|bind| bind.footer) {
                assert!(
                    !bind.triggers.is_empty(),
                    "{focus:?}: advertised {} has no trigger",
                    bind.label
                );
            }
        }
    }

    const ALL_ADD_FIELD_FOCUS: [AddFieldFocus; 9] = [
        AddFieldFocus::Key,
        AddFieldFocus::Title,
        AddFieldFocus::Description,
        AddFieldFocus::Type,
        AddFieldFocus::Searchable,
        AddFieldFocus::Viewable,
        AddFieldFocus::UserEditable,
        AddFieldFocus::Required,
        AddFieldFocus::Save,
    ];
    const ALL_REF_PROP_FOCUS: [RefPropFocus; 4] = [
        RefPropFocus::Name,
        RefPropFocus::Label,
        RefPropFocus::Type,
        RefPropFocus::Save,
    ];
    const ALL_NEW_OBJECT_FOCUS: [NewObjectFocus; 4] = [
        NewObjectFocus::Name,
        NewObjectFocus::Title,
        NewObjectFocus::Description,
        NewObjectFocus::Save,
    ];
    const ALL_RELATIONSHIP_FOCUS: [RelationshipFocus; 14] = [
        RelationshipFocus::Key,
        RelationshipFocus::Title,
        RelationshipFocus::Description,
        RelationshipFocus::Target,
        RelationshipFocus::Forward,
        RelationshipFocus::Reverse,
        RelationshipFocus::ReverseKey,
        RelationshipFocus::Searchable,
        RelationshipFocus::Viewable,
        RelationshipFocus::UserEditable,
        RelationshipFocus::Required,
        RelationshipFocus::Validate,
        RelationshipFocus::RefProperties,
        RelationshipFocus::Save,
    ];

    fn chord_advertised_exactly_when_enter_does_not_save<A: Copy + PartialEq>(
        binds: Vec<Bind<A>>,
        save: A,
    ) {
        let enter_saves = Bind::resolve(&binds, &key(KeyCode::Enter)) == Some(save);
        let advertised = Bind::footer_hints(&binds)
            .iter()
            .any(|(label, _)| *label == "^S");
        assert_ne!(enter_saves, advertised);
    }

    #[test]
    fn form_save_chords_are_advertised_only_off_save_rows() {
        for focus in ALL_ADD_FIELD_FOCUS {
            chord_advertised_exactly_when_enter_does_not_save(
                add_field_binds(Some(focus)),
                FormAct::Save,
            );
        }
        for focus in ALL_REF_PROP_FOCUS {
            chord_advertised_exactly_when_enter_does_not_save(
                ref_prop_binds(Some(focus)),
                FormAct::Save,
            );
        }
        for focus in ALL_NEW_OBJECT_FOCUS {
            chord_advertised_exactly_when_enter_does_not_save(
                new_object_binds(Some(focus)),
                FormAct::Save,
            );
        }
        for focus in ALL_RELATIONSHIP_FOCUS {
            chord_advertised_exactly_when_enter_does_not_save(
                relationship_binds(Some(focus)),
                FormAct::Save,
            );
        }
    }

    #[test]
    fn form_text_rows_leave_editing_keys_unbound() {
        for binds in [
            add_field_binds(Some(AddFieldFocus::Title)),
            ref_prop_binds(Some(RefPropFocus::Label)),
            new_object_binds(Some(NewObjectFocus::Name)),
        ] {
            for code in [
                KeyCode::Char(' '),
                KeyCode::Left,
                KeyCode::Right,
                KeyCode::Char('a'),
                KeyCode::Backspace,
            ] {
                assert_eq!(
                    Bind::resolve(&binds, &key(code)),
                    None,
                    "{code:?} should reach text input"
                );
            }
        }
    }

    #[test]
    fn confirms_bind_decisions_but_never_save_chord() {
        for binds in [
            delete_field_binds(),
            delete_object_binds(),
            rename_object_confirm_binds(),
        ] {
            assert_eq!(
                Bind::resolve(&binds, &key(KeyCode::Char('y'))),
                Some(SimpleAct::Yes)
            );
            assert_eq!(
                Bind::resolve(&binds, &key(KeyCode::Char('n'))),
                Some(SimpleAct::No)
            );
            assert_eq!(
                Bind::resolve(&binds, &key(KeyCode::Esc)),
                Some(SimpleAct::No)
            );
            assert_eq!(Bind::resolve(&binds, &ctrl(KeyCode::Char('s'))), None);
        }
    }
}
