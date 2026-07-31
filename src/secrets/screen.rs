//! ESV-secret input modes, background events, and key handling.
//!
//! The state struct lives on `App` as `app.secret`; handlers remain free
//! functions so global dispatch keeps one arm for the whole feature.

use crossterm::event::KeyEvent;

use crate::app::event::ToastKind;
use crate::app::prod_confirm::PendingProdAction;
use crate::app::{App, InputMode};
#[derive(Debug)]
pub enum ProdAction {
    Create(crate::secrets::state::CreatePlan),
    AddVersion(crate::secrets::state::VersionAddPlan),
    Delete(crate::secrets::state::DeletePlan),
    SetDescription(crate::secrets::state::SetDescriptionPlan),
    VersionStatus {
        tenant: String,
        id: String,
        version: String,
        status: String,
    },
    VersionDestroy {
        tenant: String,
        id: String,
        version: String,
    },
}
pub fn execute_prod_action(app: &mut App, action: ProdAction) {
    match action {
        ProdAction::Create(plan) => crate::secrets::ops::execute_create(app, plan, true),
        ProdAction::AddVersion(plan) => crate::secrets::ops::execute_add_version(app, plan, true),
        ProdAction::Delete(plan) => crate::secrets::ops::execute_delete(app, plan, true),
        ProdAction::SetDescription(plan) => {
            crate::secrets::ops::execute_set_description(app, plan, true)
        }
        ProdAction::VersionStatus {
            tenant,
            id,
            version,
            status,
        } => crate::secrets::ops::execute_version_status(app, tenant, id, version, status, true),
        ProdAction::VersionDestroy {
            tenant,
            id,
            version,
        } => crate::secrets::ops::execute_version_destroy(app, tenant, id, version, true),
    }
}

pub fn resume_mode(_app: &App, _action: &ProdAction) -> InputMode {
    InputMode::Normal
}

pub fn describe_prod_action(_action: &ProdAction) -> Option<String> {
    None
}

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
    crate::secrets::keys::handle_key(app, key, mode)
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

pub fn help_lines(mode: Mode, app: &App) -> Option<Vec<(&'static str, &'static str)>> {
    crate::secrets::keys::help_lines(mode, app)
}

pub fn footer_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    crate::secrets::keys::footer_hints(app)
}

pub fn create_field_active(app: &App) -> bool {
    app.secret.create.is_some()
}

pub fn create_focus(app: &App) -> Option<CreateField> {
    app.secret.create.as_ref().map(|form| form.focused)
}

pub(crate) fn open_add_version(app: &mut App) {
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

pub(crate) fn toggle_selected_version(app: &mut App) {
    match versions_view(app) {
        Some(VersionsView::Loaded {
            tenant,
            id,
            versions,
        }) => {
            if let Some(version) = versions.get(app.secret.version_selected) {
                toggle_version_status(app, &tenant, &id, version);
            }
        }
        Some(_) => {}
        None => {
            app.input_mode = InputMode::Normal;
        }
    }
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
        app.prod_confirm.pending = Some(PendingProdAction::Secrets(ProdAction::VersionStatus {
            tenant,
            id,
            version,
            status: new_status.to_string(),
        }));
        app.input_mode = InputMode::ProdConfirm;
    } else {
        ops::execute_version_status(app, tenant, id, version, new_status.to_string(), false);
    }
}

pub(crate) fn destroy_selected_version(app: &mut App) {
    match versions_view(app) {
        Some(VersionsView::Loaded {
            tenant,
            id,
            versions,
        }) => {
            if let Some(version) = versions.get(app.secret.version_selected) {
                destroy_version(app, &tenant, &id, version);
            }
        }
        Some(_) => {}
        None => {
            app.input_mode = InputMode::Normal;
        }
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

pub(crate) fn confirm_version_destroy(app: &mut App) {
    let Some((tenant, id, version)) = app.secret.pending_version_destroy.take() else {
        app.input_mode = InputMode::Secrets(Mode::Versions);
        return;
    };
    let is_prod = app
        .active_tenant()
        .is_some_and(|t| t.theme == TenantTheme::Production);
    if is_prod {
        app.prod_confirm.pending = Some(PendingProdAction::Secrets(ProdAction::VersionDestroy {
            tenant,
            id,
            version,
        }));
        app.input_mode = InputMode::ProdConfirm;
    } else {
        ops::execute_version_destroy(app, tenant, id, version, false);
    }
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

pub(crate) fn confirm_delete(app: &mut App) {
    let Some(plan) = app.secret.pending_delete.take() else {
        app.input_mode = InputMode::Normal;
        return;
    };
    let is_prod = app
        .active_tenant()
        .is_some_and(|t| t.theme == TenantTheme::Production);
    if is_prod {
        app.prod_confirm.pending = Some(PendingProdAction::Secrets(ProdAction::Delete(plan)));
        app.input_mode = InputMode::ProdConfirm;
    } else {
        ops::execute_delete(app, plan, false);
    }
}
