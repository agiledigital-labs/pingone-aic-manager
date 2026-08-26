//! Access-tab interaction: lazy refresh, selection, editable forms, and confirms.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;

use crate::access::ops;
use crate::access::state::{Document, LoadState, RoleIndexState, RuleFormState};
use crate::app::event::{AppEvent, ToastKind};
use crate::app::{App, InputMode};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Search,
    Create,
    Edit,
    DeleteConfirm,
}

pub const FLAGS_LEGEND: [(&str, &str); 2] = [
    ("A", "grant is gated by a customAuthz script"),
    ("D", "rule is byte-identical to another rule"),
];

#[derive(Debug)]
pub enum Event {
    Loaded {
        tenant: String,
        result: std::result::Result<Value, String>,
    },
    RolesLoaded {
        tenant: String,
        result: std::result::Result<crate::access::spec::RoleIndex, String>,
    },
    WriteResult {
        tenant: String,
        after: Value,
        undo_id: crate::undo::UndoId,
        resume_mode: ops::ResumeMode,
        result: std::result::Result<(), ops::WriteFailure>,
    },
    UndoResult {
        tenant: String,
        undo_id: crate::undo::UndoId,
        result: std::result::Result<Value, ops::UndoFailure>,
    },
}

pub fn apply_event(app: &mut App, event: Event) {
    match event {
        Event::Loaded { tenant, result } => {
            app.access.refreshing.remove(&tenant);
            apply_refresh(app, tenant, result);
        }
        Event::RolesLoaded { tenant, result } => {
            app.access.role_refreshing.remove(&tenant);
            let state = match result {
                Ok(index) => RoleIndexState::Loaded(index),
                Err(error) => RoleIndexState::Failed(error),
            };
            app.access.role_indices.insert(tenant, state);
        }
        Event::WriteResult {
            tenant,
            after,
            undo_id,
            resume_mode,
            result,
        } => ops::apply_write_result(app, tenant, after, undo_id, resume_mode, result),
        Event::UndoResult {
            tenant,
            undo_id,
            result,
        } => ops::apply_undo_result(app, tenant, undo_id, result),
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) {
    match mode {
        Mode::Search => handle_search_key(app, key),
        Mode::Create | Mode::Edit => handle_form_key(app, key, mode),
        Mode::DeleteConfirm => handle_delete_confirm_key(app, key),
    }
}

pub fn footer_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let InputMode::Access(mode) = app.input_mode else {
        return Vec::new();
    };
    match mode {
        Mode::Search => vec![
            ("↑/↓", "navigate"),
            ("Enter", "keep filter"),
            ("Esc", "clear + exit"),
        ],
        Mode::Create | Mode::Edit | Mode::DeleteConfirm => Vec::new(),
    }
}

pub fn help_lines(mode: Mode) -> Option<Vec<(&'static str, &'static str)>> {
    match mode {
        Mode::Search => Some(vec![
            FLAGS_LEGEND[0],
            FLAGS_LEGEND[1],
            ("Type", "edit search query"),
            ("Backspace", "delete character"),
            ("Enter", "keep filter and return to list"),
            ("Esc", "clear filter and return to list"),
            ("↑/↓", "move selection"),
            ("PgUp/PgDn", "move by page"),
            ("F1", "show keybinds"),
        ]),
        Mode::Create => Some(vec![
            ("Type", "edit focused field; ? is text"),
            ("Tab/Shift-Tab", "move between fields"),
            ("Enter", "advance, or save on the Save row"),
            ("^S", "review the change"),
            ("y", "confirm after review"),
            ("n", "return from review to the form"),
            ("Esc", "cancel; from review, return to the form"),
            ("F1", "show keybinds"),
        ]),
        Mode::Edit => Some(vec![
            ("Type", "edit focused field; ? is text"),
            ("Tab/Shift-Tab", "move between fields"),
            ("Enter", "advance, or save on the Save row"),
            ("^S", "review the change"),
            ("^X", "clear the focused optional key"),
            ("^U", "leave the focused optional key unchanged"),
            ("y", "confirm after review"),
            ("n", "return from review to the form"),
            ("Esc", "cancel; from review, return to the form"),
            ("F1", "show keybinds"),
        ]),
        Mode::DeleteConfirm => Some(vec![
            ("y", "delete the selected indexed rule"),
            ("n/Esc", "cancel"),
            ("F1", "show keybinds"),
        ]),
    }
}

pub fn refresh(app: &mut App, force: bool) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if !app.is_unlocked()
        || app.access.refreshing.contains(&tenant)
        || (!force && app.access.data.contains_key(&tenant))
    {
        return;
    }

    app.access.data.insert(tenant.clone(), LoadState::Loading);
    app.access.refreshing.insert(tenant.clone());
    let tx = app.events.tx.clone();
    let document_tenant = tenant.clone();
    tokio::spawn(async move {
        let result = crate::access::api::get_access(&document_tenant)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Access(Event::Loaded {
            tenant: document_tenant,
            result,
        }));
    });

    if !app.access.role_refreshing.contains(&tenant) {
        app.access
            .role_indices
            .insert(tenant.clone(), RoleIndexState::Loading);
        app.access.role_refreshing.insert(tenant.clone());
        let tx = app.events.tx.clone();
        tokio::spawn(async move {
            let result = crate::access::api::role_index(&tenant)
                .await
                .map_err(|error| error.to_string());
            let _ = tx.send(AppEvent::Access(Event::RolesLoaded { tenant, result }));
        });
    }
}

pub fn row_count(app: &App) -> usize {
    app.access
        .matches(app.active_tenant().map(|tenant| tenant.name.as_str()))
        .len()
}

pub fn current_selection(app: &App) -> usize {
    app.access.selected
}

pub fn select(app: &mut App, index: usize) {
    let count = row_count(app);
    app.access.select(index, count);
}

pub fn scroll_detail(app: &mut App, delta: isize) {
    app.access.detail_scroll.scroll(delta);
}

pub fn filter_active(app: &App) -> bool {
    !app.access.query.is_empty()
}

pub fn clear_filter(app: &mut App) {
    app.access.reset_view();
}

pub fn primary(app: &mut App) {
    start_edit(app);
}

pub fn delete(app: &mut App) {
    start_delete(app);
}

pub fn new_item(app: &mut App) {
    // `^N` and `o` both mean "one below the cursor", which on an empty list is
    // the same as appending. `O` puts it above instead. Position is for reading
    // only — rules are OR-ed — so neither changes what the new rule grants.
    start_create_at(app, below_cursor(app));
}

pub fn new_item_above(app: &mut App) {
    start_create_at(app, document_index(app));
}

pub fn new_item_below(app: &mut App) {
    start_create_at(app, below_cursor(app));
}

/// The document index of the selected row, or `None` when nothing is selected.
///
/// The list is filtered and the selection indexes the FILTERED rows, so a row's
/// position on screen is not its position in `configs`. Everything that writes
/// has to go through here.
fn document_index(app: &App) -> Option<usize> {
    let tenant = app.active_tenant().map(|tenant| tenant.name.clone())?;
    app.access
        .matches(Some(tenant.as_str()))
        .get(app.access.selected)
        .map(|matched| matched.row.summary.index)
}

fn below_cursor(app: &App) -> Option<usize> {
    document_index(app).map(|index| index + 1)
}

/// Move the selected rule one place towards the top of the document.
pub fn move_up(app: &mut App) {
    if reordering_blocked_by_filter(app) {
        return;
    }
    let Some(index) = document_index(app) else {
        return;
    };
    match index.checked_sub(1) {
        Some(target) => start_move(app, index, target),
        None => app.push_toast(ToastKind::Info, "Already the first rule"),
    }
}

/// Move the selected rule one place towards the end of the document.
pub fn move_down(app: &mut App) {
    if reordering_blocked_by_filter(app) {
        return;
    }
    let Some(index) = document_index(app) else {
        return;
    };
    start_move(app, index, index + 1);
}

/// A filtered list is scored, not ordered, so "up" is not a direction on it.
///
/// `K` would move the rule one place in the DOCUMENT while the displayed order
/// — which ranks by match score — need not change at all, so the operator sees
/// a write they cannot see the effect of. Better to say why than to guess what
/// they meant.
fn reordering_blocked_by_filter(app: &mut App) -> bool {
    if !filter_active(app) {
        return false;
    }
    app.push_toast(
        ToastKind::Info,
        "Clear the filter to reorder — a filtered list is ranked, not ordered",
    );
    true
}

fn start_move(app: &mut App, from: usize, to: usize) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if app.access.in_flight_writes.contains(&tenant) {
        app.push_toast(ToastKind::Info, "An Access write is already in progress");
        return;
    }
    let Some(document) = app.access.document(&tenant) else {
        return;
    };
    if to >= ops::rules(&document.value).map_or(0, Vec::len) {
        app.push_toast(ToastKind::Info, "Already the last rule");
        return;
    }
    // Built before `submit_write` takes `app` mutably.
    let request = ops::request_from_move(&tenant, document, from, to);
    match request {
        Ok(request) => {
            // Swapping two byte-identical rules produces the same document.
            // Nothing distinguishes them, so there is nothing to write — and a
            // whole-document PUT, a backup and an undo entry for a no-op is a
            // cost the operator did not ask for.
            if request.after == request.previous_document {
                app.push_toast(ToastKind::Info, "That rule is identical to its neighbour");
                return;
            }
            app.access.follow = Some((tenant, to));
            ops::submit_write(app, request);
        }
        Err(error) => app.push_toast(ToastKind::Error, error.to_string()),
    }
}

fn start_create_at(app: &mut App, at: Option<usize>) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if app.access.in_flight_writes.contains(&tenant) {
        app.push_toast(ToastKind::Info, "An Access write is already in progress");
        return;
    }
    let Some(document) = app.access.document(&tenant) else {
        return;
    };
    let mut form = crate::access::state::RuleFormState::create_at(tenant.clone(), document, at);
    let (known_roles, note) = app.access.role_validation(&tenant);
    form.set_role_validation(known_roles, note);
    app.access.form = Some(form);
    app.input_mode = InputMode::Access(Mode::Create);
}

fn start_edit(app: &mut App) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if app.access.in_flight_writes.contains(&tenant) {
        app.push_toast(ToastKind::Info, "An Access write is already in progress");
        return;
    }
    let matches = app.access.matches(Some(&tenant));
    let selected = app.access.selected.min(matches.len().saturating_sub(1));
    let Some(row) = matches.get(selected).map(|item| item.row.clone()) else {
        return;
    };
    let Some(document) = app.access.document(&tenant) else {
        return;
    };
    let mut form = crate::access::state::RuleFormState::edit(tenant.clone(), document, &row);
    let (known_roles, note) = app.access.role_validation(&tenant);
    form.set_role_validation(known_roles, note);
    app.access.form = Some(form);
    app.input_mode = InputMode::Access(Mode::Edit);
}

fn start_delete(app: &mut App) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if app.access.in_flight_writes.contains(&tenant) {
        app.push_toast(ToastKind::Info, "An Access write is already in progress");
        return;
    }
    let matches = app.access.matches(Some(&tenant));
    let selected = app.access.selected.min(matches.len().saturating_sub(1));
    let Some(row) = matches.get(selected).map(|item| item.row.clone()) else {
        return;
    };
    let Some(document) = app.access.document(&tenant) else {
        return;
    };
    app.access.pending_delete = Some(crate::access::state::DeleteState::new(
        tenant, document, &row,
    ));
    app.input_mode = InputMode::Access(Mode::DeleteConfirm);
}

fn apply_refresh(app: &mut App, tenant: String, result: std::result::Result<Value, String>) {
    let result =
        result.and_then(|value| Document::from_value(value).map_err(|error| error.to_string()));
    match result {
        Ok(document) => {
            app.access
                .data
                .insert(tenant.clone(), LoadState::Loaded(document));
        }
        Err(error) => {
            app.access
                .data
                .insert(tenant.clone(), LoadState::Failed(error.clone()));
            if app
                .active_tenant()
                .is_some_and(|active| active.name == tenant)
            {
                app.push_toast(ToastKind::Error, format!("Access rules failed: {error}"));
            }
        }
    }

    follow_moved_rule(app, &tenant);
    if app
        .active_tenant()
        .is_some_and(|active| active.name == tenant)
    {
        let count = row_count(app);
        app.access.clamp_selection(count);
    }
}

/// Put the cursor back on the rule a reorder just moved.
///
/// Resolved against the current rows, because a document index is not a row:
/// the list is filtered and the filter may not even contain the moved rule, in
/// which case there is nothing to follow and the selection stays where it is.
///
/// Called from both paths that can replace the document — the write's own
/// result, which swaps the cached document in without any refresh, and a later
/// refresh — and it is a no-op unless the tenant matches, so a refresh for
/// another tenant cannot consume it.
pub(super) fn follow_moved_rule(app: &mut App, tenant: &str) {
    if app
        .access
        .follow
        .as_ref()
        .is_none_or(|(owner, _)| owner != tenant)
    {
        return;
    }
    let Some((_, index)) = app.access.follow.take() else {
        return;
    };
    if app
        .active_tenant()
        .is_none_or(|active| active.name != tenant)
    {
        return;
    }
    let row = app
        .access
        .matches(Some(tenant))
        .iter()
        .position(|matched| matched.row.summary.index == index);
    if let Some(row) = row {
        let count = row_count(app);
        app.access.select(row, count);
    }
}

fn handle_search_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            clear_filter(app);
            app.input_mode = InputMode::Normal;
            return;
        }
        KeyCode::Enter => {
            app.input_mode = InputMode::Normal;
            return;
        }
        KeyCode::Up => return move_selection(app, -1),
        KeyCode::Down => return move_selection(app, 1),
        KeyCode::PageUp => return move_selection(app, -10),
        KeyCode::PageDown => return move_selection(app, 10),
        _ => {}
    }

    let before = app.access.query.value().to_string();
    if app.access.query.handle_key(&key) && app.access.query.value() != before {
        app.access.selected = 0;
        app.access.scroll = 0;
        app.access.detail_scroll.reset();
    }
}

fn handle_form_key(app: &mut App, key: KeyEvent, mode: Mode) {
    let confirming = app
        .access
        .form
        .as_ref()
        .is_some_and(RuleFormState::confirming);
    if confirming {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let request = app.access.form.as_ref().map(ops::request_from_form);
                match request {
                    Some(Ok(request)) => ops::submit_write(app, request),
                    Some(Err(error)) => {
                        if let Some(form) = app.access.form.as_mut() {
                            form.unreview();
                            form.error = Some(error.to_string());
                        }
                    }
                    None => app.input_mode = InputMode::Normal,
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                if let Some(form) = app.access.form.as_mut() {
                    form.unreview();
                }
            }
            _ => {}
        }
        return;
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Esc => {
            app.access.form = None;
            app.input_mode = InputMode::Normal;
            return;
        }
        KeyCode::Tab => {
            if let Some(form) = app.access.form.as_mut() {
                form.focused = form.focused.next();
            }
            return;
        }
        KeyCode::BackTab => {
            if let Some(form) = app.access.form.as_mut() {
                form.focused = form.focused.prev();
            }
            return;
        }
        KeyCode::Char('x') if ctrl && mode == Mode::Edit => {
            if let Some(field) = app
                .access
                .form
                .as_mut()
                .and_then(|form| form.optional_mut())
            {
                field.set_clear();
            }
            return;
        }
        KeyCode::Char('u') if ctrl && mode == Mode::Edit => {
            if let Some(field) = app
                .access
                .form
                .as_mut()
                .and_then(|form| form.optional_mut())
            {
                field.set_unchanged();
            }
            return;
        }
        KeyCode::Char('s') if ctrl => {
            review_form(app);
            return;
        }
        KeyCode::Enter => {
            if app
                .access
                .form
                .as_ref()
                .is_some_and(|form| form.focused == crate::access::state::FormFocus::Save)
            {
                review_form(app);
            } else if let Some(form) = app.access.form.as_mut() {
                form.focused = form.focused.next();
            }
            return;
        }
        _ => {}
    }

    let Some(form) = app.access.form.as_mut() else {
        return;
    };
    match form.focused {
        crate::access::state::FormFocus::Pattern => {
            form.pattern.handle_key(&key);
        }
        crate::access::state::FormFocus::Roles => {
            form.roles.handle_key(&key);
        }
        crate::access::state::FormFocus::Methods => {
            form.methods.handle_key(&key);
        }
        crate::access::state::FormFocus::Actions
        | crate::access::state::FormFocus::CustomAuthz
        | crate::access::state::FormFocus::ExcludePatterns => {
            if let Some(field) = form.optional_mut() {
                field.handle_key(&key);
            }
        }
        crate::access::state::FormFocus::Save => {}
    }
}

fn review_form(app: &mut App) {
    let Some(tenant) = app.access.form.as_ref().map(|form| form.tenant.clone()) else {
        return;
    };
    let (known_roles, note) = app.access.role_validation(&tenant);
    let Some(form) = app.access.form.as_mut() else {
        return;
    };
    form.set_role_validation(known_roles, note);
    form.review();
    match ops::request_from_form(form) {
        Ok(request) => {
            form.error = None;
            form.review_warnings = request.warnings;
        }
        Err(error) => {
            form.unreview();
            form.error = Some(error.to_string());
        }
    }
}

fn handle_delete_confirm_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let request = if let Some(delete) = app.access.pending_delete.as_mut() {
                delete.confirm();
                ops::request_from_delete(delete)
            } else {
                return;
            };
            match request {
                Ok(request) => ops::submit_write(app, request),
                Err(error) => app.push_toast(ToastKind::Error, error.to_string()),
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.access.pending_delete = None;
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
}

fn move_selection(app: &mut App, delta: isize) {
    let count = row_count(app);
    app.access.move_selection(count, delta);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::View;
    use crate::config::{Tenant, TenantTheme};

    fn tenant(name: &str) -> Tenant {
        Tenant {
            name: name.into(),
            base_url: "https://test.invalid".into(),
            theme: TenantTheme::Sandbox,
            sa_id: None,
            scopes: Vec::new(),
            provenance: crate::config::Provenance::default(),
        }
    }

    fn app_with_rules(tenant_name: &str) -> App {
        let mut app = App::for_test(vec![tenant(tenant_name)], View::Access);
        let document =
            crate::access::state::Document::from_value(crate::access::six_rule_fixture())
                .expect("fixture document");
        app.access.data.insert(
            tenant_name.into(),
            crate::access::state::LoadState::Loaded(document),
        );
        app
    }

    /// The cursor has to end up on the rule that moved, not the row it left.
    ///
    /// The write's own result swaps the cached document in **without a
    /// refresh**, so a follow consumed only by `apply_refresh` never fires: the
    /// selection keeps its row, the moved rule slides out from under it, and the
    /// next `J` moves whatever was displaced instead.
    #[test]
    fn the_cursor_follows_the_moved_rule_through_the_writes_own_result() {
        let mut app = app_with_rules("sandbox");
        app.access.select(1, row_count(&app));
        let moved =
            crate::access::ops::rules(&app.access.document("sandbox").expect("document").value)
                .expect("rules")[1]
                .clone();

        let after = crate::access::ops::move_rule(
            &app.access.document("sandbox").expect("document").value,
            1,
            3,
        )
        .expect("move")
        .document;
        app.access.follow = Some(("sandbox".into(), 3));
        crate::access::ops::apply_write_result(
            &mut app,
            "sandbox".into(),
            after,
            crate::undo::UndoId::default(),
            crate::access::ops::ResumeMode::List,
            Ok(()),
        );

        let selected = app.access.matches(Some("sandbox"))[app.access.selected]
            .row
            .raw
            .clone();
        assert_eq!(selected, moved, "selection stayed on the row, not the rule");
        assert!(app.access.follow.is_none(), "the follow must be consumed");
    }

    /// A follow left armed by one tenant must not steer another tenant's list.
    #[test]
    fn a_follow_is_not_consumed_by_another_tenants_refresh() {
        let mut app = app_with_rules("sandbox");
        app.access.select(2, row_count(&app));
        app.access.follow = Some(("other".into(), 0));
        follow_moved_rule(&mut app, "sandbox");
        assert_eq!(app.access.selected, 2, "another tenant's follow moved us");
        assert!(app.access.follow.is_some(), "and it was consumed");
    }

    /// Ranked rows are not ordered rows, so "up" is not a direction on them.
    #[test]
    fn reordering_is_refused_while_a_filter_is_active() {
        let mut app = app_with_rules("sandbox");
        app.access.query.set("duplicate");
        let before = app
            .access
            .document("sandbox")
            .expect("document")
            .value
            .clone();
        move_up(&mut app);
        move_down(&mut app);
        assert_eq!(
            app.access.document("sandbox").expect("document").value,
            before,
            "a filtered reorder must not write"
        );
        assert!(app.access.follow.is_none());
    }
}
