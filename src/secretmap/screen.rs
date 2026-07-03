//! Secret-mapping sub-view interaction: lazy list loading, search, ESV-alias
//! picker handling, write submission, and undo result application.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::event::ToastKind;
use crate::app::{App, InputMode};
use crate::secretmap::api;
use crate::secretmap::ops::{
    self, AliasReplacePlan, MappingDeletePlan, UndoFailure, UndoOutcome, WriteFailure, WriteOutcome,
};
use crate::secretmap::state::{DeleteMappingState, EditAliasState, LoadState, PickLabelState};
use crate::undo::UndoId;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Search,
    PickLabel,
    PickAlias,
    DeleteConfirm,
}

#[derive(Debug)]
pub enum Event {
    ListLoaded {
        tenant: String,
        mappings: Vec<api::Mapping>,
    },
    EsvSecretsLoaded {
        tenant: String,
        ids: Vec<String>,
    },
    ValidLabelsLoaded {
        tenant: String,
        ids: Vec<String>,
    },
    WriteResult {
        tenant: String,
        secret_id: String,
        undo_id: UndoId,
        snapshot: serde_json::Value,
        result: std::result::Result<WriteOutcome, WriteFailure>,
    },
    UndoResult {
        undo_id: UndoId,
        tenant: String,
        result: std::result::Result<UndoOutcome, UndoFailure>,
    },
    LoadFailed {
        tenant: String,
        esv_secrets: bool,
        message: String,
    },
    ValidLabelsFailed {
        tenant: String,
        message: String,
    },
}

pub fn apply_event(app: &mut App, event: Event) {
    match event {
        Event::ListLoaded { tenant, mappings } => {
            app.secretmap.refreshing.remove(&tenant);
            app.secretmap
                .data
                .insert(tenant.clone(), LoadState::Loaded(mappings));
            if app
                .active_tenant()
                .is_some_and(|active| active.name == tenant)
            {
                app.secretmap.clamp_selection(row_count(app));
                let n = app.secretmap.label_matches(Some(&tenant)).len();
                app.secretmap.clamp_label_selection(n);
            }
        }
        Event::EsvSecretsLoaded { tenant, ids } => {
            app.secretmap.esv_secret_loading.remove(&tenant);
            app.secretmap.esv_secret_failed.remove(&tenant);
            app.secretmap.esv_secret_ids.insert(tenant.clone(), ids);
            if app
                .active_tenant()
                .is_some_and(|active| active.name == tenant)
            {
                let n = app.secretmap.alias_matches(Some(&tenant)).len();
                app.secretmap.clamp_picker_selection(n);
            }
        }
        Event::ValidLabelsLoaded { tenant, ids } => {
            app.secretmap.valid_secret_loading.remove(&tenant);
            app.secretmap.valid_secret_failed.remove(&tenant);
            app.secretmap.valid_secret_ids.insert(tenant.clone(), ids);
            if app
                .active_tenant()
                .is_some_and(|active| active.name == tenant)
            {
                let n = app.secretmap.label_matches(Some(&tenant)).len();
                app.secretmap.clamp_label_selection(n);
            }
        }
        Event::WriteResult {
            tenant,
            secret_id,
            undo_id,
            snapshot,
            result,
        } => ops::apply_write_result(app, tenant, secret_id, undo_id, snapshot, result),
        Event::UndoResult {
            undo_id,
            tenant,
            result,
        } => ops::apply_undo_result(app, undo_id, tenant, result),
        Event::LoadFailed {
            tenant,
            esv_secrets,
            message,
        } => {
            if esv_secrets {
                app.secretmap.esv_secret_loading.remove(&tenant);
                app.secretmap
                    .esv_secret_failed
                    .insert(tenant.clone(), message.clone());
                if app
                    .active_tenant()
                    .is_some_and(|active| active.name == tenant)
                {
                    if let Some(edit) = app.secretmap.editing.as_mut() {
                        edit.error = Some(format!("ESV secret list failed: {message}"));
                    }
                    app.push_toast(
                        ToastKind::Error,
                        format!("ESV secret list failed: {message}"),
                    );
                }
            } else {
                app.secretmap.refreshing.remove(&tenant);
                app.secretmap
                    .data
                    .insert(tenant.clone(), LoadState::Failed(message.clone()));
                if app
                    .active_tenant()
                    .is_some_and(|active| active.name == tenant)
                {
                    app.push_toast(
                        ToastKind::Error,
                        format!("Secret mapping list failed: {message}"),
                    );
                }
            }
        }
        Event::ValidLabelsFailed { tenant, message } => {
            app.secretmap.valid_secret_loading.remove(&tenant);
            app.secretmap
                .valid_secret_failed
                .insert(tenant.clone(), message.clone());
            if app
                .active_tenant()
                .is_some_and(|active| active.name == tenant)
            {
                if let Some(pick) = app.secretmap.picking_label.as_mut() {
                    pick.error = Some(format!("Secret label list failed: {message}"));
                }
                app.push_toast(
                    ToastKind::Error,
                    format!("Secret label list failed: {message}"),
                );
            }
        }
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) {
    match mode {
        Mode::Search => handle_search_key(app, key),
        Mode::PickLabel => handle_label_picker_key(app, key),
        Mode::PickAlias => handle_picker_key(app, key),
        Mode::DeleteConfirm => handle_delete_confirm_key(app, key),
    }
}

pub fn footer_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let InputMode::Secretmap(mode) = app.input_mode else {
        return Vec::new();
    };
    match mode {
        Mode::Search => vec![("Enter", "keep filter"), ("Esc", "clear + exit")],
        Mode::PickLabel => vec![
            ("Enter", "choose label"),
            ("↑/↓", "navigate"),
            ("Esc", "cancel"),
        ],
        Mode::PickAlias => vec![
            ("Enter", "choose alias"),
            ("↑/↓", "navigate"),
            ("Esc", "cancel"),
        ],
        Mode::DeleteConfirm => vec![("y", "remove"), ("n/Esc", "cancel")],
    }
}

pub fn is_available(app: &App) -> bool {
    app.active_tenant()
        .is_some_and(|tenant| tenant.allows_secret_mappings())
}

pub fn refresh(app: &mut App, force: bool) {
    if !is_available(app) {
        return;
    }
    if force {
        if let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) {
            app.secretmap.invalidate_valid_label_cache(&tenant);
        }
    }
    ops::load_list(app, force);
    ops::load_esv_secrets(app, force);
}

pub fn start_search(app: &mut App) {
    if !is_available(app) {
        return;
    }
    app.input_mode = InputMode::Secretmap(Mode::Search);
}

pub fn row_count(app: &App) -> usize {
    if !is_available(app) {
        return 0;
    }
    app.secretmap
        .matches(app.active_tenant().map(|tenant| tenant.name.as_str()))
        .len()
}

pub fn current_selection(app: &App) -> usize {
    app.secretmap.selected
}

pub fn select(app: &mut App, idx: usize) {
    app.secretmap.select(idx);
}

pub fn clear_filter(app: &mut App) {
    app.secretmap.reset_view();
}

pub fn filter_active(app: &App) -> bool {
    !app.secretmap.query.is_empty()
}

pub fn primary(app: &mut App) {
    start_alias_picker(app);
}

pub fn delete(app: &mut App) {
    start_remove(app);
}

pub fn new_item(app: &mut App) {
    start_add(app);
}

pub fn scroll_detail(app: &mut App, delta: isize) {
    if delta.is_negative() {
        app.secretmap.detail_scroll = app
            .secretmap
            .detail_scroll
            .saturating_sub(delta.unsigned_abs());
    } else {
        app.secretmap.detail_scroll = app.secretmap.detail_scroll.saturating_add(delta as usize);
    }
}

pub fn start_alias_picker(app: &mut App) {
    if !is_available(app) {
        return;
    }
    let Some(tenant_name) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    let Some(mapping) = app.secretmap.selected_mapping(Some(&tenant_name)) else {
        return;
    };
    if app
        .secretmap
        .in_flight_writes
        .contains(&(tenant_name.clone(), mapping.secret_id.clone()))
    {
        app.push_toast(
            ToastKind::Info,
            format!("Write already in progress: {}", mapping.secret_id),
        );
        return;
    }

    app.secretmap.editing = Some(EditAliasState::new(tenant_name, mapping));
    app.input_mode = InputMode::Secretmap(Mode::PickAlias);
    ops::load_esv_secrets(app, false);
}

pub fn start_add(app: &mut App) {
    if !is_available(app) {
        return;
    }
    let Some(tenant_name) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    app.secretmap.picking_label = Some(PickLabelState::new(tenant_name));
    app.input_mode = InputMode::Secretmap(Mode::PickLabel);
    ops::load_list(app, false);
    ops::load_valid_secret_ids(app, false);
}

pub fn start_remove(app: &mut App) {
    if !is_available(app) {
        return;
    }
    let Some(tenant_name) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    let Some(mapping) = app.secretmap.selected_mapping(Some(&tenant_name)) else {
        return;
    };
    let Some(prior_alias) = mapping.alias.clone() else {
        app.push_toast(
            ToastKind::Info,
            format!("{} is already unmapped", mapping.secret_id),
        );
        return;
    };
    if app
        .secretmap
        .in_flight_writes
        .contains(&(tenant_name.clone(), mapping.secret_id.clone()))
    {
        app.push_toast(
            ToastKind::Info,
            format!("Write already in progress: {}", mapping.secret_id),
        );
        return;
    }

    let snapshot =
        crate::secretmap::state::mapping_snapshot(&mapping.secret_id, Some(prior_alias.as_str()));
    app.secretmap.pending_delete = Some(DeleteMappingState {
        tenant: tenant_name,
        realm: crate::secretmap::state::REALM.to_string(),
        secret_id: mapping.secret_id,
        prior_alias,
        snapshot,
    });
    app.input_mode = InputMode::Secretmap(Mode::DeleteConfirm);
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
        KeyCode::Up => return crate::app::keymap::move_selection(app, -1),
        KeyCode::Down => return crate::app::keymap::move_selection(app, 1),
        KeyCode::PageUp => return crate::app::keymap::move_selection(app, -10),
        KeyCode::PageDown => return crate::app::keymap::move_selection(app, 10),
        _ => {}
    }

    let before = app.secretmap.query.value().to_string();
    if app.secretmap.query.handle_key(&key) && app.secretmap.query.value() != before {
        app.secretmap.selected = 0;
        app.secretmap.scroll = 0;
        app.secretmap.detail_scroll = 0;
    }
}

fn handle_label_picker_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.secretmap.picking_label = None;
            app.input_mode = InputMode::Normal;
            return;
        }
        KeyCode::Enter => {
            select_picker_label(app);
            return;
        }
        KeyCode::Up => {
            move_label_picker(app, -1);
            return;
        }
        KeyCode::Down => {
            move_label_picker(app, 1);
            return;
        }
        KeyCode::PageUp => {
            move_label_picker(app, -10);
            return;
        }
        KeyCode::PageDown => {
            move_label_picker(app, 10);
            return;
        }
        _ => {}
    }

    let before = app
        .secretmap
        .picking_label
        .as_ref()
        .map(|pick| pick.query.value().to_string())
        .unwrap_or_default();
    if let Some(pick) = app.secretmap.picking_label.as_mut() {
        pick.error = None;
        if pick.query.handle_key(&key) && pick.query.value() != before {
            pick.selected = 0;
        }
    }
}

fn handle_picker_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.secretmap.editing = None;
            app.input_mode = InputMode::Normal;
            return;
        }
        KeyCode::Enter => {
            select_picker_alias(app);
            return;
        }
        KeyCode::Up => {
            move_picker(app, -1);
            return;
        }
        KeyCode::Down => {
            move_picker(app, 1);
            return;
        }
        KeyCode::PageUp => {
            move_picker(app, -10);
            return;
        }
        KeyCode::PageDown => {
            move_picker(app, 10);
            return;
        }
        _ => {}
    }

    let before = app
        .secretmap
        .editing
        .as_ref()
        .map(|edit| edit.query.value().to_string())
        .unwrap_or_default();
    if let Some(edit) = app.secretmap.editing.as_mut() {
        edit.error = None;
        if edit.query.handle_key(&key) && edit.query.value() != before {
            edit.selected = 0;
        }
    }
}

fn handle_delete_confirm_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let Some(delete) = app.secretmap.pending_delete.take() else {
                app.input_mode = InputMode::Normal;
                return;
            };
            let plan = MappingDeletePlan {
                tenant: delete.tenant,
                realm: delete.realm,
                secret_id: delete.secret_id,
                prior_alias: delete.prior_alias,
                snapshot: delete.snapshot,
            };
            ops::submit_remove(app, plan);
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.secretmap.pending_delete = None;
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
}

fn move_label_picker(app: &mut App, delta: isize) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    let n = app.secretmap.label_matches(Some(&tenant)).len();
    if n == 0 {
        return;
    }
    if let Some(pick) = app.secretmap.picking_label.as_mut() {
        let cur = pick.selected.min(n - 1) as isize;
        pick.selected = (cur + delta).clamp(0, n as isize - 1) as usize;
    }
}

fn move_picker(app: &mut App, delta: isize) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    let n = app.secretmap.alias_matches(Some(&tenant)).len();
    if n == 0 {
        return;
    }
    if let Some(edit) = app.secretmap.editing.as_mut() {
        let cur = edit.selected.min(n - 1) as isize;
        edit.selected = (cur + delta).clamp(0, n as isize - 1) as usize;
    }
}

fn select_picker_label(app: &mut App) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if app.secretmap.valid_secret_loading.contains(&tenant) {
        return;
    }
    if let Some(error) = app.secretmap.valid_secret_failed.get(&tenant).cloned() {
        if let Some(pick) = app.secretmap.picking_label.as_mut() {
            pick.error = Some(format!("Secret label list failed: {error}"));
        }
        return;
    }

    let matches = app.secretmap.label_matches(Some(&tenant));
    let selected = app
        .secretmap
        .picking_label
        .as_ref()
        .map(|pick| pick.selected.min(matches.len().saturating_sub(1)))
        .unwrap_or(0);
    let Some(secret_id) = matches.get(selected).map(|item| item.id.clone()) else {
        if let Some(pick) = app.secretmap.picking_label.as_mut() {
            pick.error = Some("No unmapped secret label selected".into());
        }
        return;
    };
    if app
        .secretmap
        .in_flight_writes
        .contains(&(tenant.clone(), secret_id.clone()))
    {
        if let Some(pick) = app.secretmap.picking_label.as_mut() {
            pick.error = Some(format!("Write already in progress: {secret_id}"));
        }
        return;
    }

    app.secretmap.picking_label = None;
    app.secretmap.editing = Some(EditAliasState::new(
        tenant,
        api::Mapping {
            secret_id,
            alias: None,
        },
    ));
    app.input_mode = InputMode::Secretmap(Mode::PickAlias);
    ops::load_esv_secrets(app, false);
}

fn select_picker_alias(app: &mut App) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if app.secretmap.esv_secret_loading.contains(&tenant) {
        return;
    }
    if let Some(error) = app.secretmap.esv_secret_failed.get(&tenant).cloned() {
        if let Some(edit) = app.secretmap.editing.as_mut() {
            edit.error = Some(format!("ESV secret list failed: {error}"));
        }
        return;
    }

    let matches = app.secretmap.alias_matches(Some(&tenant));
    let selected = app
        .secretmap
        .editing
        .as_ref()
        .map(|edit| edit.selected.min(matches.len().saturating_sub(1)))
        .unwrap_or(0);
    let Some(new_alias) = matches.get(selected).map(|item| item.id.clone()) else {
        if let Some(edit) = app.secretmap.editing.as_mut() {
            edit.error = Some("No ESV secret selected".into());
        }
        return;
    };

    let Some(edit) = app.secretmap.editing.as_ref() else {
        return;
    };
    if edit.prior_alias.as_deref() == Some(new_alias.as_str()) {
        app.push_toast(
            ToastKind::Info,
            format!("Mapping already points to {new_alias}"),
        );
        app.secretmap.editing = None;
        app.input_mode = InputMode::Normal;
        return;
    }

    let plan = AliasReplacePlan {
        tenant: edit.tenant.clone(),
        realm: edit.realm.clone(),
        secret_id: edit.secret_id.clone(),
        prior_alias: edit.prior_alias.clone(),
        new_alias,
        snapshot: edit.snapshot.clone(),
    };
    ops::submit_alias_replace(app, plan);
}
