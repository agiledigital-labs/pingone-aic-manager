//! Shared onboarding glue used by more than one flow: the ordered menu slice
//! that drives selection + numbering, tenant persistence, and the small
//! form/confirm helpers each flow's key handler reaches for.
//!
//! HTTP/bootstrap orchestration lives in [`super::bootstrap`]; per-flow key
//! handling, bootstrap tasks, and completion handlers live in the flow files.

use crate::app::event::{AppEvent, ToastKind};
use crate::app::prod_confirm::PendingProdAction;
use crate::app::{App, InputMode};
use crate::config::ProjectConfig;
use crate::config::tenant::{CredentialSource, Provenance, Tenant, TenantTheme};
use crate::logs::LogKeyPair;

use super::OnboardPath;
use super::screen::{Mode, PendingConfirm, ProdAction};

/// One entry in the add-tenant menu. The menu is a single ordered slice so
/// adding a flow is appending one entry — no tuple-index math. `label`
/// carries a `{n}` placeholder for the displayed choice number, which is the
/// 1-based position of the entry within the *visible* list.
pub struct MenuEntry {
    pub path: OnboardPath,
    /// Label template; `{n}` is replaced with the visible choice number.
    pub label: &'static str,
    /// Whether the entry is offered (Envrc only appears with env creds).
    pub available: fn(has_env_creds: bool) -> bool,
}

/// Menu order and labels — byte-identical to the historical hard-coded list.
pub const MENU: &[MenuEntry] = &[
    MenuEntry {
        path: OnboardPath::Cookie,
        label: "  {n}  Paste browser session cookie  (full SSO/MFA/passkey)",
        available: |_| true,
    },
    MenuEntry {
        path: OnboardPath::UserPass,
        label: "  {n}  Username + password           (TOTP supported)",
        available: |_| true,
    },
    MenuEntry {
        path: OnboardPath::Paste,
        label: "  {n}  Paste service-account JWK     (already have one)",
        available: |_| true,
    },
    MenuEntry {
        path: OnboardPath::Envrc,
        label: "  {n}  Import sandbox from environment",
        available: |has_env_creds| has_env_creds,
    },
    MenuEntry {
        path: OnboardPath::LogOnly,
        label: "  {n}  Log-only environment          (logs API key, no service account)",
        available: |_| true,
    },
];

/// The visible menu entries in order, honouring `has_env_creds`.
pub fn visible_entries(has_env_creds: bool) -> impl Iterator<Item = &'static MenuEntry> {
    MENU.iter().filter(move |e| (e.available)(has_env_creds))
}

/// Number of options currently offered.
pub(crate) fn menu_option_count(has_env_creds: bool) -> usize {
    visible_entries(has_env_creds).count()
}

/// Rendered rows for the menu: `(display_number, label)` in order.
pub fn menu_rows(has_env_creds: bool) -> Vec<(usize, String)> {
    visible_entries(has_env_creds)
        .enumerate()
        .map(|(i, entry)| {
            let n = i + 1;
            (n, entry.label.replace("{n}", &n.to_string()))
        })
        .collect()
}

/// The `OnboardPath` at a given visible menu index (0-based).
pub fn path_for_index(idx: usize, has_env_creds: bool) -> Option<OnboardPath> {
    visible_entries(has_env_creds).nth(idx).map(|e| e.path)
}

pub(crate) fn tenant_name_exists(tenants: &[Tenant], name: &str) -> bool {
    tenants.iter().any(|tenant| tenant.name == name)
}

pub(crate) fn queue_overwrite_confirm(app: &mut App, pending: PendingConfirm) {
    app.onboard.pending_confirm = Some(pending);
    app.input_mode = InputMode::Onboard(Mode::OverwriteConfirm);
}

pub(crate) fn clear_onboard_forms(app: &mut App) {
    app.onboard.cookie_form = None;
    app.onboard.up_form = None;
    app.onboard.paste_form = None;
    app.onboard.log_only_form = None;
    app.onboard.pending_id = None;
    app.onboard.pending_callback_body = None;
}

pub(crate) fn send_onboard_error(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    onboard_id: uuid::Uuid,
    message: impl Into<String>,
) {
    let _ = tx.send(AppEvent::Onboard(super::screen::Event::Error {
        onboard_id,
        message: message.into(),
    }));
}

/// Service-account completion handler shared by the cookie and userpass flows,
/// which both emit [`super::screen::Event::ServiceAccountReady`]. Drops stale
/// completions, then persists (routing production themes through the guard).
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_sa_created(
    app: &mut App,
    onboard_id: uuid::Uuid,
    tenant_name: String,
    base_url: String,
    theme: TenantTheme,
    sa_id: String,
    jwk: serde_json::Value,
    log_key: Option<LogKeyPair>,
) -> crate::Result<()> {
    // Drop the event if it doesn't match the bootstrap we're waiting on.
    if app.onboard.pending_id != Some(onboard_id) {
        tracing::debug!(
            event_id = %onboard_id,
            pending = ?app.onboard.pending_id,
            "dropping stale service-account completion"
        );
        return Ok(());
    }
    app.onboard.pending_id = None;

    let scopes: Vec<String> = super::bootstrap::SA_SCOPES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let tenant = Tenant {
        name: tenant_name,
        base_url,
        theme,
        sa_id: Some(sa_id),
        scopes,
        provenance: Provenance {
            service_account: Some(CredentialSource::Created),
            // Only if one was actually minted. This path completes with
            // `log_key: None` when minting was skipped or failed, and claiming
            // `Created` there would let a log key the user later pastes in
            // inherit a provenance that defaults it to purge on offboarding.
            log_key: log_key.as_ref().map(|_| CredentialSource::Created),
        },
    };

    // Clear in-flight forms.
    app.onboard.cookie_form = None;
    app.onboard.up_form = None;
    app.onboard.pending_callback_body = None;

    if tenant.theme == TenantTheme::Production {
        app.prod_confirm.pending = Some(PendingProdAction::Onboard(ProdAction::SaveTenant {
            tenant,
            jwk: Some(jwk),
            log_key,
        }));
        app.input_mode = InputMode::ProdConfirm;
        return Ok(());
    }

    match persist_tenant_overwriting(app, tenant, Some(jwk), log_key) {
        Ok(()) => {
            app.push_toast(ToastKind::Success, "Tenant saved");
            app.input_mode = InputMode::Normal;
        }
        Err(e) => {
            app.push_toast(ToastKind::Error, format!("Save failed: {e}"));
            app.input_mode = InputMode::Normal;
        }
    }
    Ok(())
}

// ---- Persist helpers (jwks + tenants config) ----

/// Save a tenant outright — replacing any existing entry with the same
/// name. Caller is responsible for confirming the overwrite before calling.
pub(crate) fn persist_tenant_overwriting(
    app: &mut App,
    tenant: Tenant,
    jwk: Option<serde_json::Value>,
    log_key: Option<LogKeyPair>,
) -> crate::Result<()> {
    let tenant_name = tenant.name.clone();
    if let Some(jwk) = jwk {
        app.save_jwk(&tenant.name, jwk)?;
    }

    // Recorded against the file under its lock, not from `app.tenants`: that
    // list is this session's copy, and saving it whole would drop a tenant
    // another `aic` onboarded since it was loaded.
    let config = record_tenant(crate::config::project_paths(), &tenant, || {
        std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "aic-project".into())
    })?;
    app.tenants = config.tenants.clone();
    if let Some(idx) = app.tenants.iter().position(|t| t.name == tenant.name) {
        app.set_active_tenant(idx);
    }
    app.config = Some(config);

    if let Some(log_key) = log_key {
        app.save_log_key(&tenant_name, log_key)?;
    }

    // Kick the ESVs tab to fetch for the just-added tenant. Without this
    // the user lands on the dashboard after a fresh import and has to
    // wait up to 30s for the next poll tick — feels broken even though
    // it isn't.
    crate::esv::ops::refresh(app, false);
    Ok(())
}

/// Replace the entry named like `tenant`, or append it, in `config.toml` as
/// one locked transaction, creating the file on a first onboarding. The
/// default tenant stays the first entry, as it always has. Returns the
/// document written.
pub(crate) fn record_tenant(
    paths: &crate::config::ProjectPaths,
    tenant: &Tenant,
    project_name: impl FnOnce() -> String,
) -> crate::Result<ProjectConfig> {
    paths.update_or_create_config(
        || {
            Ok(ProjectConfig {
                project: project_name(),
                default_tenant: String::new(),
                tenants: Vec::new(),
            })
        },
        |config| {
            match config.tenants.iter().position(|t| t.name == tenant.name) {
                Some(idx) => config.tenants[idx] = tenant.clone(),
                None => config.tenants.push(tenant.clone()),
            }
            config.default_tenant = config
                .tenants
                .first()
                .map(|t| t.name.clone())
                .unwrap_or_default();
            Ok(config.clone())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tenant::TenantTheme;

    #[test]
    fn log_only_is_last_with_or_without_env_import() {
        assert_eq!(path_for_index(3, false), Some(OnboardPath::LogOnly));
        assert_eq!(path_for_index(4, false), None);

        assert_eq!(path_for_index(3, true), Some(OnboardPath::Envrc));
        assert_eq!(path_for_index(4, true), Some(OnboardPath::LogOnly));
        assert_eq!(path_for_index(5, true), None);
    }

    #[test]
    fn tenant_name_exists_matches_exact_existing_name() {
        let tenants = vec![Tenant {
            name: "sandbox".into(),
            base_url: "https://example.forgeblocks.com".into(),
            theme: TenantTheme::Sandbox,
            sa_id: Some("service-account-id".into()),
            scopes: vec!["fr:am:*".into()],
            provenance: Provenance::default(),
        }];

        assert!(tenant_name_exists(&tenants, "sandbox"));
        assert!(!tenant_name_exists(&tenants, "Sandbox"));
        assert!(!tenant_name_exists(&tenants, "development"));
    }

    fn entry(name: &str, base_url: &str) -> Tenant {
        Tenant {
            name: name.into(),
            base_url: base_url.into(),
            theme: TenantTheme::Sandbox,
            sa_id: None,
            scopes: Vec::new(),
            provenance: Provenance::default(),
        }
    }

    /// The first onboarding creates the file; a later one edits the file as
    /// it is now — a tenant only another process knew about survives, and a
    /// same-name entry is replaced, not duplicated.
    #[test]
    fn record_tenant_creates_then_merges_into_the_file_on_disk() {
        let dir = crate::config::TestDir::new();
        let paths = crate::config::ProjectPaths::new(dir.path("p")).unwrap();
        let first = record_tenant(&paths, &entry("a", "https://a.example.com"), || {
            "proj".into()
        })
        .unwrap();
        assert_eq!(first.project, "proj");
        assert_eq!(first.default_tenant, "a");

        // Another process onboarded `b` since this session loaded its copy.
        paths
            .update_config(|c| {
                c.tenants.push(entry("b", "https://b.example.com"));
                Ok(())
            })
            .unwrap();
        record_tenant(
            &paths,
            &entry("a", "https://a2.example.com"),
            || unreachable!(),
        )
        .unwrap();
        record_tenant(
            &paths,
            &entry("c", "https://c.example.com"),
            || unreachable!(),
        )
        .unwrap();

        let on_disk = paths.load_config().unwrap().unwrap();
        let got: Vec<_> = on_disk
            .tenants
            .iter()
            .map(|t| (t.name.as_str(), t.base_url.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("a", "https://a2.example.com"),
                ("b", "https://b.example.com"),
                ("c", "https://c.example.com")
            ]
        );
        assert_eq!(on_disk.project, "proj");
        assert_eq!(on_disk.default_tenant, "a");
    }
}
