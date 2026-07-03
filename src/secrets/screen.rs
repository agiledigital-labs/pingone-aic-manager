//! ESV-secret input modes, background events, and key handling.
//!
//! The state struct lives on `App` as `app.secret`; handlers remain free
//! functions so global dispatch keeps one arm for the whole feature.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::event::ToastKind;
use crate::app::prod_confirm::PendingProdAction;
use crate::app::{App, InputMode};
use crate::config::tenant::TenantTheme;
use crate::esv::state::{LoadState, id_of};
use crate::secrets::ops;
use crate::secrets::state::{
    AddVersionForm, CreateField, CreateForm, DeletePlan, DetailFocus, Encoding, SecretOpKind,
    VersionsView, description_of, encoding_of, secret_in_cache, selected_secret, version_num,
    version_status, versions_view,
};
use crate::tui::widgets::TextField;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Create,
    Versions,
    AddVersion,
    DeleteConfirm,
    VersionDestroyConfirm,
}

#[derive(Debug)]
pub enum Event {
    OpResult {
        tenant: String,
        id: String,
        kind: SecretOpKind,
        label: String,
        reload_versions: bool,
        result: std::result::Result<serde_json::Value, String>,
    },
    VersionsListed {
        tenant: String,
        id: String,
        result: std::result::Result<Vec<serde_json::Value>, String>,
    },
}

pub fn apply_event(app: &mut App, event: Event) {
    match event {
        Event::OpResult {
            tenant,
            id,
            kind,
            label,
            reload_versions,
            result,
        } => ops::apply_op_result(app, tenant, id, kind, label, reload_versions, result),
        Event::VersionsListed { tenant, id, result } => {
            ops::apply_versions_listed(app, tenant, id, result)
        }
    }
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

pub fn start_create(app: &mut App) {
    app.secret.create = Some(CreateForm::new());
    app.input_mode = InputMode::Secrets(Mode::Create);
}

pub fn row_count(app: &App) -> usize {
    crate::secrets::state::rows(app, app.active_tenant().map(|t| t.name.as_str())).len()
}

pub fn current_selection(app: &App) -> usize {
    app.secret.list.selected
}

pub fn set_selection(app: &mut App, idx: usize) {
    app.secret.list.selected = idx;
}

pub fn filter_active(app: &App) -> bool {
    !app.secret.list.query.is_empty()
}

pub fn clear_filter(app: &mut App) {
    app.secret.reset_view();
}

pub fn primary(app: &mut App) {
    open_versions(app);
}

pub fn delete(app: &mut App) {
    request_delete(app);
}

pub fn new_item(app: &mut App) {
    start_create(app);
}

fn handle_create_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let Some(form) = app.secret.create.as_mut() else {
        return Ok(());
    };
    let focused = form.focused;
    match key.code {
        KeyCode::Esc => {
            app.secret.create = None;
            app.input_mode = InputMode::Normal;
            return Ok(());
        }
        KeyCode::Tab => {
            form.focused = focused.next();
            return Ok(());
        }
        KeyCode::BackTab => {
            form.focused = focused.prev();
            return Ok(());
        }
        KeyCode::Enter => {
            match focused {
                CreateField::Value | CreateField::Save => ops::commit_create(app),
                _ => form.focused = focused.next(),
            }
            return Ok(());
        }
        KeyCode::Left | KeyCode::Right => {
            let left = key.code == KeyCode::Left;
            match focused {
                CreateField::Encoding => {
                    form.encoding = if left {
                        form.encoding.prev()
                    } else {
                        form.encoding.next()
                    };
                    return Ok(());
                }
                CreateField::Placeholders => {
                    form.use_in_placeholders = !form.use_in_placeholders;
                    return Ok(());
                }
                CreateField::Json => {
                    form.as_json = !form.as_json;
                    return Ok(());
                }
                _ => {}
            }
        }
        _ => {}
    }

    form.error = None;
    match focused {
        CreateField::Id => {
            form.id.handle_key(&key);
        }
        CreateField::Description => {
            form.description.handle_key(&key);
        }
        CreateField::Value => {
            form.value.handle_key(&key);
        }
        CreateField::Placeholders if key.code == KeyCode::Char(' ') => {
            form.use_in_placeholders = !form.use_in_placeholders;
        }
        CreateField::Json if key.code == KeyCode::Char(' ') => {
            form.as_json = !form.as_json;
        }
        _ => {}
    }
    Ok(())
}

fn open_add_version(app: &mut App) {
    let Some((tenant, id)) = app.secret.version_target.clone() else {
        return;
    };
    let encoding = secret_in_cache(app, &tenant, &id)
        .as_ref()
        .map(|s| match encoding_of(s) {
            "pem" => Encoding::Pem,
            "base64hmac" => Encoding::Base64Hmac,
            "base64aes" => Encoding::Base64Aes,
            _ => Encoding::Generic,
        })
        .unwrap_or(Encoding::Generic);
    app.secret.add_version = Some(AddVersionForm {
        tenant,
        id,
        encoding,
        value: TextField::masked("New version value"),
        error: None,
    });
    app.input_mode = InputMode::Secrets(Mode::AddVersion);
}

fn handle_add_version_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let Some(form) = app.secret.add_version.as_mut() else {
        return Ok(());
    };
    match key.code {
        KeyCode::Esc => {
            app.secret.add_version = None;
            app.input_mode = InputMode::Secrets(Mode::Versions);
        }
        KeyCode::Enter => ops::commit_add_version(app),
        _ => {
            form.error = None;
            form.value.handle_key(&key);
        }
    }
    Ok(())
}

pub fn versions_panel_open(app: &App) -> bool {
    matches!(
        app.input_mode,
        InputMode::Secrets(Mode::Versions | Mode::AddVersion | Mode::VersionDestroyConfirm)
    )
}

pub fn open_versions(app: &mut App) {
    let Some(secret) = selected_secret(app) else {
        return;
    };
    let Some(tenant) = app.active_tenant().map(|t| t.name.clone()) else {
        return;
    };
    let id = id_of(&secret).to_string();
    app.secret.version_selected = 0;
    app.secret.version_target = Some((tenant.clone(), id.clone()));
    app.secret.detail_focus = DetailFocus::Versions;
    app.secret.description =
        TextField::single_line("Description").with_initial(description_of(&secret));
    app.secret
        .versions
        .insert((tenant.clone(), id.clone()), LoadState::Loading);
    app.input_mode = InputMode::Secrets(Mode::Versions);
    ops::reload_versions(app, tenant, id);
}

fn handle_versions_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let (tenant, id, versions) = match versions_view(app) {
        Some(VersionsView::Loaded {
            tenant,
            id,
            versions,
        }) => (tenant, id, versions),
        Some(_) => {
            if key.code == KeyCode::Esc {
                app.input_mode = InputMode::Normal;
            }
            return Ok(());
        }
        None => {
            app.input_mode = InputMode::Normal;
            return Ok(());
        }
    };
    match key.code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
            return Ok(());
        }
        KeyCode::Tab | KeyCode::BackTab => {
            app.secret.detail_focus = match app.secret.detail_focus {
                DetailFocus::Versions => DetailFocus::Description,
                DetailFocus::Description => DetailFocus::Versions,
            };
            return Ok(());
        }
        _ => {}
    }

    if app.secret.detail_focus == DetailFocus::Description {
        if key.code == KeyCode::Enter {
            ops::commit_description(app);
        } else {
            app.secret.description.handle_key(&key);
        }
        return Ok(());
    }

    let n = versions.len();
    match key.code {
        KeyCode::Char('j') | KeyCode::Down if n > 0 && app.secret.version_selected + 1 < n => {
            app.secret.version_selected += 1;
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.secret.version_selected = app.secret.version_selected.saturating_sub(1);
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            open_add_version(app);
        }
        KeyCode::Char('e') | KeyCode::Char('d') => {
            if let Some(v) = versions.get(app.secret.version_selected) {
                toggle_version_status(app, &tenant, &id, v);
            }
        }
        KeyCode::Char('x') | KeyCode::Delete => {
            if let Some(v) = versions.get(app.secret.version_selected) {
                destroy_version(app, &tenant, &id, v);
            }
        }
        _ => {}
    }
    Ok(())
}

fn toggle_version_status(app: &mut App, tenant: &str, id: &str, v: &serde_json::Value) {
    let Some(version) = version_num(v) else {
        return;
    };
    let status = version_status(v);
    let new_status = match status {
        "ENABLED" => "DISABLED",
        "DISABLED" => "ENABLED",
        other => {
            app.push_toast(
                ToastKind::Info,
                format!("Version {version} is {other}; status can't change"),
            );
            return;
        }
    };
    let tenant = tenant.to_string();
    let id = id.to_string();
    let is_prod = app
        .active_tenant()
        .is_some_and(|t| t.theme == TenantTheme::Production);
    if is_prod {
        app.prod_confirm.pending = Some(PendingProdAction::SecretVersionStatus {
            tenant,
            id,
            version,
            status: new_status.to_string(),
        });
        app.input_mode = InputMode::ProdConfirm;
    } else {
        ops::execute_version_status(app, tenant, id, version, new_status.to_string(), false);
    }
}

fn destroy_version(app: &mut App, tenant: &str, id: &str, v: &serde_json::Value) {
    let Some(version) = version_num(v) else {
        return;
    };
    if version_status(v) == "DESTROYED" {
        app.push_toast(
            ToastKind::Info,
            format!("Version {version} already destroyed"),
        );
        return;
    }
    app.secret.pending_version_destroy = Some((tenant.to_string(), id.to_string(), version));
    app.input_mode = InputMode::Secrets(Mode::VersionDestroyConfirm);
}

fn handle_version_destroy_confirm_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let Some((tenant, id, version)) = app.secret.pending_version_destroy.take() else {
                app.input_mode = InputMode::Secrets(Mode::Versions);
                return Ok(());
            };
            let is_prod = app
                .active_tenant()
                .is_some_and(|t| t.theme == TenantTheme::Production);
            if is_prod {
                app.prod_confirm.pending = Some(PendingProdAction::SecretVersionDestroy {
                    tenant,
                    id,
                    version,
                });
                app.input_mode = InputMode::ProdConfirm;
            } else {
                ops::execute_version_destroy(app, tenant, id, version, false);
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.secret.pending_version_destroy = None;
            app.input_mode = InputMode::Secrets(Mode::Versions);
        }
        _ => {}
    }
    Ok(())
}

pub fn request_delete(app: &mut App) {
    let Some(secret) = selected_secret(app) else {
        return;
    };
    let Some(tenant) = app.active_tenant().map(|t| t.name.clone()) else {
        return;
    };
    app.secret.pending_delete = Some(DeletePlan {
        tenant,
        id: id_of(&secret).to_string(),
    });
    app.input_mode = InputMode::Secrets(Mode::DeleteConfirm);
}

fn handle_delete_confirm_key(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let Some(plan) = app.secret.pending_delete.take() else {
                app.input_mode = InputMode::Normal;
                return Ok(());
            };
            let is_prod = app
                .active_tenant()
                .is_some_and(|t| t.theme == TenantTheme::Production);
            if is_prod {
                app.prod_confirm.pending = Some(PendingProdAction::SecretDelete(plan));
                app.input_mode = InputMode::ProdConfirm;
            } else {
                ops::execute_delete(app, plan, false);
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.secret.pending_delete = None;
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
    Ok(())
}
