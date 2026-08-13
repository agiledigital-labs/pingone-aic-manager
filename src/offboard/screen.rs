//! Env-picker delete modal: keys, nested [`Mode`]/[`Event`], and the
//! background probe/execute that feed them.
//!
//! The planner in [`crate::offboard::spec`] decides what is safe. This
//! module only collects ticks and sends them through
//! [`DeletePlan::resolve_purge`] — it must not invent a second request.

use std::collections::HashSet;

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::event::{AppEvent, ToastKind};
use crate::app::prod_confirm::PendingProdAction;
use crate::app::{App, InputMode};
use crate::config::Tenant;
use crate::offboard::ops::{self, ExecuteReport, PathPresence, VaultView};
use crate::offboard::spec::{self, DeletePlan, Inventory, PromptAction, ResolvedPurge, TargetKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Probing,
    Confirm,
    Working,
}

#[derive(Debug)]
pub enum Event {
    Probed {
        generation: u64,
        tenant: String,
        result: Result<(Inventory, DeletePlan), String>,
    },
    Executed {
        generation: u64,
        tenant: String,
        /// Captured at dispatch, not read back from the form on arrival.
        ///
        /// It is the only way to name the signing key left trusted by the
        /// tenant when the remote step fails, and the form it would otherwise
        /// come from is mutable state this event does not own. `Mode::Working`
        /// swallows every key today, so the form does survive — but the first
        /// change that lets the operator leave a running delete would drop the
        /// warning silently, which is the worst way to lose it.
        kid: Option<String>,
        report: ExecuteReport,
    },
}

#[derive(Debug)]
pub enum ProdAction {
    Execute,
}

#[derive(Debug, Default)]
pub struct State {
    pub form: Option<Form>,
    generation: u64,
    pub pending_name: Option<String>,
    #[cfg(test)]
    pub last_purge: Option<ResolvedPurge>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.form = None;
        self.pending_name = None;
        #[cfg(test)]
        {
            self.last_purge = None;
        }
    }
}

#[derive(Debug)]
pub struct Form {
    pub tenant: Tenant,
    pub inventory: Inventory,
    pub plan: DeletePlan,
    pub accepted: HashSet<TargetKind>,
    pub cursor: usize,
}

impl Form {
    fn new(tenant: Tenant, inventory: Inventory, plan: DeletePlan) -> Self {
        let accepted = default_accepted(&plan);
        Self {
            tenant,
            inventory,
            plan,
            accepted,
            cursor: 0,
        }
    }

    pub fn visible(&self) -> Vec<TargetKind> {
        TargetKind::ALL
            .into_iter()
            .filter(|kind| {
                !matches!(
                    self.plan.prompt_for(*kind, &self.accepted),
                    PromptAction::Absent
                )
            })
            .collect()
    }

    pub fn selected_kind(&self) -> Option<TargetKind> {
        self.visible().get(self.cursor).copied()
    }

    fn clamp_cursor(&mut self) {
        let n = self.visible().len();
        if n == 0 {
            self.cursor = 0;
        } else if self.cursor >= n {
            self.cursor = n - 1;
        }
    }

    fn toggle_selected(&mut self) {
        let Some(kind) = self.selected_kind() else {
            return;
        };
        if !matches!(
            self.plan.prompt_for(kind, &self.accepted),
            PromptAction::Ask { .. }
        ) {
            return;
        }
        if !self.accepted.remove(&kind) {
            self.accepted.insert(kind);
        }
        self.clamp_cursor();
    }

    fn purge(&self) -> ResolvedPurge {
        self.plan.resolve_purge(self.accepted.iter().copied())
    }
}

fn default_accepted(plan: &DeletePlan) -> HashSet<TargetKind> {
    let mut accepted = HashSet::new();
    for kind in TargetKind::ALL {
        if let PromptAction::Ask { default_on: true } = plan.prompt_for(kind, &accepted) {
            accepted.insert(kind);
        }
    }
    accepted
}

pub fn help_lines(mode: Mode) -> Option<Vec<(&'static str, &'static str)>> {
    match mode {
        Mode::Probing => Some(vec![("Esc", "cancel"), ("F1", "show keybinds")]),
        Mode::Confirm => Some(vec![
            ("↑/↓", "move between artifacts"),
            ("Space", "toggle offered artifact"),
            ("Enter", "delete tenant"),
            ("Esc", "cancel"),
            ("F1", "show keybinds"),
        ]),
        Mode::Working => Some(vec![("F1", "show keybinds")]),
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent, mode: Mode) {
    match mode {
        Mode::Probing => {
            if key.code == KeyCode::Esc {
                cancel(app);
            }
        }
        Mode::Working => {}
        Mode::Confirm => match key.code {
            KeyCode::Esc => cancel(app),
            KeyCode::Up | KeyCode::Char('k') => move_cursor(app, -1),
            KeyCode::Down | KeyCode::Char('j') => move_cursor(app, 1),
            KeyCode::Char(' ') => {
                if let Some(form) = app.offboard.form.as_mut() {
                    form.toggle_selected();
                }
            }
            KeyCode::Enter => submit(app),
            _ => {}
        },
    }
}

fn move_cursor(app: &mut App, delta: isize) {
    let Some(form) = app.offboard.form.as_mut() else {
        return;
    };
    let n = form.visible().len();
    if n == 0 {
        return;
    }
    let next = form.cursor as isize + delta;
    form.cursor = next.clamp(0, n as isize - 1) as usize;
}

fn cancel(app: &mut App) {
    app.offboard.clear();
    app.input_mode = InputMode::EnvPicker;
}

/// Open the delete modal for the env-picker highlight.
///
/// No tenant → no-op (including an empty list). Does not touch disk when
/// there is no Tokio runtime, so `App::for_test` can drive the picker
/// keys without reading `.aic/` or talking to the agent.
pub fn open_from_picker(app: &mut App) {
    let Some(tenant) = app.tenants.get(app.env_picker_idx).cloned() else {
        return;
    };
    app.offboard.clear();
    app.offboard.generation = app.offboard.generation.wrapping_add(1);
    let generation = app.offboard.generation;
    app.offboard.pending_name = Some(tenant.name.clone());

    if tokio::runtime::Handle::try_current().is_ok() {
        app.input_mode = InputMode::Offboard(Mode::Probing);
        spawn_probe(app, tenant, generation);
        return;
    }

    // Unit tests (and any caller without a runtime) get a plan from
    // in-memory maps only — empty paths, so nothing under `.aic/` is read.
    let vault = vault_from_memory(app);
    let (inventory, _, plan) =
        ops::plan_for(&tenant, &app.tenants, &vault, &PathPresence::default());
    app.offboard.form = Some(Form::new(tenant, inventory, plan));
    app.input_mode = InputMode::Offboard(Mode::Confirm);
}

fn vault_from_memory(app: &App) -> VaultView {
    let mut view = VaultView::default();
    for name in app.jwks().keys() {
        view.jwks.insert(name.clone());
    }
    view
}

fn spawn_probe(app: &App, tenant: Tenant, generation: u64) {
    let names: Vec<String> = app
        .tenants
        .iter()
        .map(|tenant| tenant.name.clone())
        .collect();
    let tenants = app.tenants.clone();
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = match ops::probe_vault(&names).await {
            Ok(vault) => {
                let paths = ops::probe_paths(&tenant.name, &ops::Layout::live());
                let (inventory, _, plan) = ops::plan_for(&tenant, &tenants, &vault, &paths);
                Ok((inventory, plan))
            }
            Err(error) => Err(error.to_string()),
        };
        let _ = tx.send(AppEvent::Offboard(Event::Probed {
            generation,
            tenant: tenant.name,
            result,
        }));
    });
}

fn submit(app: &mut App) {
    let Some(form) = app.offboard.form.as_ref() else {
        return;
    };
    if form.tenant.is_prod() {
        app.prod_confirm.pending = Some(PendingProdAction::Offboard(ProdAction::Execute));
        app.input_mode = InputMode::ProdConfirm;
        return;
    }
    start_execute(app);
}

pub fn execute_prod_action(app: &mut App, action: ProdAction) {
    match action {
        ProdAction::Execute => start_execute(app),
    }
}

pub fn resume_mode(_app: &App, _action: &ProdAction) -> InputMode {
    InputMode::Offboard(Mode::Confirm)
}

pub fn describe_prod_action(app: &App, _action: &ProdAction) -> Option<String> {
    app.offboard
        .form
        .as_ref()
        .map(|form| format!("Delete tenant {}", form.tenant.name))
}

fn start_execute(app: &mut App) {
    let Some(form) = app.offboard.form.as_ref() else {
        return;
    };
    let purge = form.purge();
    #[cfg(test)]
    {
        // Unit tests must not run LiveIo against the real `.aic/` tree.
        app.offboard.last_purge = Some(purge);
    }
    #[cfg(not(test))]
    {
        spawn_live_execute(app, purge);
    }
}

#[cfg(not(test))]
fn spawn_live_execute(app: &mut App, purge: ResolvedPurge) {
    let Some(form) = app.offboard.form.as_ref() else {
        return;
    };
    let Some(cfg) = app.config.clone() else {
        app.push_toast(ToastKind::Error, "no .aic/config.toml here");
        return;
    };
    let tenant = form.tenant.clone();
    let inventory = form.inventory.clone();
    let kid = inventory.issuer_kid.clone();
    let generation = app.offboard.generation;
    let current = crate::config::read_current_context().ok().flatten();
    app.input_mode = InputMode::Offboard(Mode::Working);
    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let mut io = ops::LiveIo::default();
        let report = ops::execute(
            &tenant,
            &cfg,
            current.as_deref(),
            &inventory,
            &purge,
            &ops::Layout::live(),
            &mut io,
        )
        .await;
        let _ = tx.send(AppEvent::Offboard(Event::Executed {
            generation,
            tenant: tenant.name,
            kid,
            report,
        }));
    });
}

pub fn apply_event(app: &mut App, event: Event) {
    match event {
        Event::Probed {
            generation,
            tenant,
            result,
        } => apply_probed(app, generation, tenant, result),
        Event::Executed {
            generation,
            tenant,
            kid,
            report,
        } => apply_executed(app, generation, tenant, kid, report),
    }
}

fn apply_probed(
    app: &mut App,
    generation: u64,
    tenant: String,
    result: Result<(Inventory, DeletePlan), String>,
) {
    if !probe_is_current(app, generation, &tenant) {
        return;
    }
    match result {
        Ok((inventory, plan)) => {
            let Some(departing) = app.tenants.iter().find(|row| row.name == tenant).cloned() else {
                cancel(app);
                return;
            };
            app.offboard.form = Some(Form::new(departing, inventory, plan));
            app.offboard.pending_name = None;
            app.input_mode = InputMode::Offboard(Mode::Confirm);
        }
        Err(error) => {
            app.push_toast(
                ToastKind::Error,
                format!("Could not read local artifacts: {error}"),
            );
            cancel(app);
        }
    }
}

fn apply_executed(
    app: &mut App,
    generation: u64,
    tenant: String,
    kid: Option<String>,
    report: ExecuteReport,
) {
    // The world may have changed even if the operator left the modal.
    // Only the input-mode restore is gated — a stale Offboard form must
    // not replace whatever they are looking at (see access::ops::
    // write_mode_is_current).
    let still_here = execute_is_current(app, generation, &tenant);

    if report.config_removed {
        apply_removal(app, &tenant, &report);
        app.offboard.clear();
        if still_here {
            app.input_mode = if app.tenants.is_empty() {
                InputMode::Normal
            } else {
                InputMode::EnvPicker
            };
        }
        app.push_toast(ToastKind::Success, format!("Removed tenant {tenant}"));
        if let Some(next) = &report.next_context {
            app.push_toast(ToastKind::Info, format!("context is now {next}"));
        } else if app.tenants.is_empty() {
            app.push_toast(ToastKind::Info, "context cleared (no tenants remain)");
        }
        if let (Some(error), Some(kid)) = (report.remote_error.as_deref(), kid.as_deref()) {
            app.push_toast(
                ToastKind::Warning,
                spec::console_cleanup_issuer_line(kid, error),
            );
        }
    } else if still_here {
        app.input_mode = InputMode::Offboard(Mode::Confirm);
        app.push_toast(
            ToastKind::Error,
            "Tenant entry was left in place so the removal can be retried",
        );
        if let (Some(error), Some(kid)) = (report.remote_error.as_deref(), kid.as_deref()) {
            app.push_toast(
                ToastKind::Warning,
                spec::console_cleanup_issuer_line(kid, error),
            );
        }
    } else {
        app.offboard.clear();
        app.push_toast(
            ToastKind::Error,
            "Tenant entry was left in place so the removal can be retried",
        );
    }
}

fn probe_is_current(app: &App, generation: u64, tenant: &str) -> bool {
    app.offboard.generation == generation
        && app.input_mode == InputMode::Offboard(Mode::Probing)
        && app.offboard.pending_name.as_deref() == Some(tenant)
}

fn execute_is_current(app: &App, generation: u64, tenant: &str) -> bool {
    app.offboard.generation == generation
        && matches!(
            app.input_mode,
            InputMode::Offboard(Mode::Working) | InputMode::Offboard(Mode::Confirm)
        )
        && app
            .offboard
            .form
            .as_ref()
            .is_some_and(|form| form.tenant.name == tenant)
}

/// Apply a successful config removal to in-memory tenant state.
///
/// Does **not** call [`App::set_active_tenant`]: that writes
/// `current-context`, and [`ops::execute`] already did. Tests must not
/// touch the real `.aic/` directory.
fn apply_removal(app: &mut App, removed: &str, report: &ExecuteReport) {
    let previous_active = app.active_tenant().map(|tenant| tenant.name.clone());
    app.tenants.retain(|tenant| tenant.name != removed);
    if let Some(cfg) = app.config.as_mut() {
        ops::drop_tenant(cfg, removed);
        cfg.default_tenant = report.default_tenant.clone();
    }

    if app.tenants.is_empty() {
        app.active_tenant_idx = 0;
        app.env_picker_idx = 0;
        reset_view_state(app);
        return;
    }

    if let Some(name) = previous_active {
        if let Some(idx) = app.tenants.iter().position(|tenant| tenant.name == name) {
            app.active_tenant_idx = idx;
            if app.env_picker_idx >= app.tenants.len() {
                app.env_picker_idx = app.tenants.len() - 1;
            }
            return;
        }
    }

    let idx = report
        .next_context
        .as_deref()
        .and_then(|name| app.tenants.iter().position(|tenant| tenant.name == name))
        .unwrap_or(0);
    app.active_tenant_idx = idx;
    app.env_picker_idx = idx;
    reset_view_state(app);
    crate::app::refresh_view(app, app.active_view, false);
}

fn reset_view_state(app: &mut App) {
    app.esv.reset_view();
    app.secret.reset_view();
    app.scripts.reset_view();
    app.managed.reset_view();
    app.mappings.reset_view();
    app.access.reset_view();
    app.idmstore.reset_view();
    app.oauth.reset_view();
    app.secretmap.reset_view();
    let mappings_allowed = app
        .tenants
        .get(app.active_tenant_idx)
        .is_some_and(|tenant| tenant.allows_secret_mappings());
    app.esv.view = app.esv.view.clamp(mappings_allowed);
}

#[cfg(test)]
pub fn open_confirm_for_test(app: &mut App, inventory: Inventory, plan: DeletePlan) {
    let tenant = app
        .tenants
        .get(app.env_picker_idx)
        .cloned()
        .expect("open_confirm_for_test requires a highlighted tenant");
    app.offboard.generation = app.offboard.generation.wrapping_add(1);
    app.offboard.form = Some(Form::new(tenant, inventory, plan));
    app.offboard.pending_name = None;
    app.input_mode = InputMode::Offboard(Mode::Confirm);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, View};
    use crate::config::{CredentialSource, ProjectConfig, Provenance, TenantTheme};
    use crate::offboard::ops::{ExecuteReport, Step, StepOutcome, StepStatus};
    use crate::offboard::spec::{self, Survivor, TargetDecision};

    const UAT_URL: &str = "https://tenant.example.com";
    const UAT_SA: &str = "2f1882d0-df7b-4067-8b58-03fda365acf8";
    const DUP_SA: &str = "b55e7f59-3fc1-4512-9843-8925cff63e90";
    const SHARED_LOG_KEY: &str = "shared-log-key";

    fn tenant(name: &str, sa_id: Option<&str>) -> Tenant {
        Tenant {
            name: name.into(),
            base_url: UAT_URL.into(),
            theme: TenantTheme::Sandbox,
            sa_id: sa_id.map(str::to_string),
            scopes: Vec::new(),
            provenance: Provenance::default(),
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    fn present() -> Inventory {
        Inventory {
            service_account_jwk: true,
            log_api_key_id: Some(SHARED_LOG_KEY.into()),
            issuer_kid: Some("kid-uat".into()),
            logs_database: true,
            idm_store: true,
            workspace: true,
            sync_state: true,
            undo_entries: true,
        }
    }

    fn two_uat_app() -> App {
        let mut app = App::for_test(
            vec![tenant("uat", Some(UAT_SA)), tenant("UAT", Some(DUP_SA))],
            View::Esvs,
        );
        app.active_tenant_idx = 1;
        app.env_picker_idx = 1;
        app.config = Some(ProjectConfig {
            project: "test".into(),
            default_tenant: "UAT".into(),
            tenants: app.tenants.clone(),
        });
        app
    }

    fn open_shared_log_key(app: &mut App) {
        let departing = app.tenants[app.env_picker_idx].clone();
        let keep = Survivor::from_tenant(&app.tenants[0], Some(SHARED_LOG_KEY.into()), None);
        let plan = spec::plan(&departing, &present(), &[keep]);
        open_confirm_for_test(app, present(), plan);
    }

    #[test]
    fn esc_returns_to_picker_and_leaves_tenants_untouched() {
        let mut app = two_uat_app();
        open_shared_log_key(&mut app);
        let before_tenants = app.tenants.clone();
        let before_active = app.active_tenant_idx;
        let before_picker = app.env_picker_idx;

        handle_key(&mut app, key(KeyCode::Esc), Mode::Confirm);

        assert_eq!(app.input_mode, InputMode::EnvPicker);
        assert!(app.offboard.form.is_none());
        assert_eq!(app.tenants, before_tenants);
        assert_eq!(app.active_tenant_idx, before_active);
        assert_eq!(app.env_picker_idx, before_picker);
    }

    #[test]
    fn space_cannot_toggle_a_refused_row() {
        let mut app = two_uat_app();
        open_shared_log_key(&mut app);
        {
            let form = app.offboard.form.as_ref().unwrap();
            assert!(matches!(
                form.plan.decision(TargetKind::LogApiKey),
                TargetDecision::Refused { .. }
            ));
            assert!(!form.accepted.contains(&TargetKind::LogApiKey));
            let cursor = form
                .visible()
                .iter()
                .position(|kind| *kind == TargetKind::LogApiKey)
                .expect("refused log key is a visible row");
            app.offboard.form.as_mut().unwrap().cursor = cursor;
        }

        handle_key(&mut app, key(KeyCode::Char(' ')), Mode::Confirm);

        let form = app.offboard.form.as_ref().unwrap();
        assert!(
            !form.accepted.contains(&TargetKind::LogApiKey),
            "Space must not accept a refused target"
        );
    }

    #[test]
    fn enter_does_not_put_a_refused_row_in_the_purge() {
        let mut app = two_uat_app();
        open_shared_log_key(&mut app);
        // Leave the refused log-key row highlighted so a buggy Enter that
        // used the cursor instead of resolve_purge would include it.
        let cursor = app
            .offboard
            .form
            .as_ref()
            .unwrap()
            .visible()
            .iter()
            .position(|kind| *kind == TargetKind::LogApiKey)
            .unwrap();
        app.offboard.form.as_mut().unwrap().cursor = cursor;

        handle_key(&mut app, key(KeyCode::Enter), Mode::Confirm);

        let purge = app
            .offboard
            .last_purge
            .as_ref()
            .expect("Enter records purge");
        assert!(
            !purge.contains(&TargetKind::LogApiKey),
            "refused target must stay out of the executed plan"
        );
        assert!(purge.contains(&TargetKind::ServiceAccountJwk));
    }

    #[test]
    fn deleting_the_active_tenant_retargets_index_and_list() {
        let mut app = two_uat_app();
        open_shared_log_key(&mut app);
        assert_eq!(app.active_tenant().map(|t| t.name.as_str()), Some("UAT"));

        let generation = app.offboard.generation;
        apply_event(
            &mut app,
            Event::Executed {
                kid: Some("kid-uat".into()),
                generation,
                tenant: "UAT".into(),
                report: ExecuteReport {
                    backup_path: None,
                    steps: vec![StepOutcome {
                        step: Step::ConfigEntry,
                        status: StepStatus::Ok,
                    }],
                    config_removed: true,
                    next_context: Some("uat".into()),
                    default_tenant: "uat".into(),
                    remote_error: None,
                },
            },
        );

        assert_eq!(app.tenants.len(), 1);
        assert_eq!(app.tenants[0].name, "uat");
        assert_eq!(app.active_tenant_idx, 0);
        assert_eq!(app.env_picker_idx, 0);
        assert_eq!(app.active_tenant().map(|t| t.name.as_str()), Some("uat"));
        assert_eq!(
            app.config.as_ref().map(|cfg| cfg.default_tenant.as_str()),
            Some("uat")
        );
        assert_eq!(app.input_mode, InputMode::EnvPicker);
        assert!(app.offboard.form.is_none());
    }

    #[test]
    fn failed_config_removal_is_an_error_and_keeps_the_tenant() {
        let mut app = two_uat_app();
        open_shared_log_key(&mut app);
        let generation = app.offboard.generation;

        apply_event(
            &mut app,
            Event::Executed {
                kid: Some("kid-uat".into()),
                generation,
                tenant: "UAT".into(),
                report: ExecuteReport {
                    backup_path: None,
                    steps: vec![StepOutcome {
                        step: Step::ConfigEntry,
                        status: StepStatus::Skipped(
                            "earlier local step failed; tenant left addressable",
                        ),
                    }],
                    config_removed: false,
                    next_context: Some("UAT".into()),
                    default_tenant: "UAT".into(),
                    remote_error: None,
                },
            },
        );

        assert_eq!(app.tenants.len(), 2);
        assert_eq!(app.active_tenant().map(|t| t.name.as_str()), Some("UAT"));
        assert_eq!(app.input_mode, InputMode::Offboard(Mode::Confirm));
        assert!(
            app.toasts.iter().any(|toast| {
                matches!(toast.kind, ToastKind::Error) && toast.message.contains("left in place")
            }),
            "failure must not look like a successful delete: {:?}",
            app.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn remote_error_toast_names_the_kid() {
        let mut app = two_uat_app();
        open_shared_log_key(&mut app);
        let generation = app.offboard.generation;

        apply_event(
            &mut app,
            Event::Executed {
                kid: Some("kid-uat".into()),
                generation,
                tenant: "UAT".into(),
                report: ExecuteReport {
                    backup_path: None,
                    steps: vec![],
                    config_removed: true,
                    next_context: Some("uat".into()),
                    default_tenant: "uat".into(),
                    remote_error: Some("injected remote failure".into()),
                },
            },
        );

        assert!(
            app.toasts.iter().any(|toast| {
                matches!(toast.kind, ToastKind::Warning)
                    && toast.message.contains("kid-uat")
                    && toast.message.contains("unpublish failed")
            }),
            "remote_error must surface the kid: {:?}",
            app.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn leaving_the_modal_drops_form_so_a_later_draw_cannot_show_it() {
        // draw.rs routes on input_mode alone and returns. A stale form
        // that a later Offboard mode would pick up replaces whatever the
        // operator is looking at — the same class as access write_mode_is_current.
        let mut app = two_uat_app();
        open_shared_log_key(&mut app);
        handle_key(&mut app, key(KeyCode::Esc), Mode::Confirm);
        assert_eq!(app.input_mode, InputMode::EnvPicker);
        assert!(
            app.offboard.form.is_none(),
            "cancel must drop the form, not just the mode"
        );

        let generation = app.offboard.generation;
        let departing = app.tenants[1].clone();
        apply_event(
            &mut app,
            Event::Probed {
                generation,
                tenant: "UAT".into(),
                result: Ok((present(), spec::plan(&departing, &present(), &[]))),
            },
        );
        assert!(
            app.offboard.form.is_none(),
            "a late probe must not reinstall a form after cancel"
        );
        assert_eq!(app.input_mode, InputMode::EnvPicker);
    }

    #[test]
    fn execute_result_does_not_restore_offboard_after_switch_away() {
        let mut app = two_uat_app();
        open_shared_log_key(&mut app);
        let generation = app.offboard.generation;
        app.input_mode = InputMode::Normal;

        apply_event(
            &mut app,
            Event::Executed {
                kid: Some("kid-uat".into()),
                generation,
                tenant: "UAT".into(),
                report: ExecuteReport {
                    backup_path: None,
                    steps: vec![],
                    config_removed: true,
                    next_context: Some("uat".into()),
                    default_tenant: "uat".into(),
                    remote_error: None,
                },
            },
        );

        assert_eq!(
            app.input_mode,
            InputMode::Normal,
            "must not steal the screen back to Offboard/EnvPicker"
        );
        assert!(app.offboard.form.is_none());
        assert_eq!(app.tenants.len(), 1);
    }

    #[test]
    fn space_toggles_an_external_default_off_row() {
        let mut departing = tenant("UAT", Some(DUP_SA));
        departing.provenance.service_account = Some(CredentialSource::External);
        let mut app = App::for_test(vec![departing.clone()], View::Esvs);
        app.env_picker_idx = 0;
        let plan = spec::plan(&departing, &present(), &[]);
        open_confirm_for_test(&mut app, present(), plan);

        {
            let form = app.offboard.form.as_ref().unwrap();
            assert!(!form.accepted.contains(&TargetKind::ServiceAccountJwk));
            let cursor = form
                .visible()
                .iter()
                .position(|kind| *kind == TargetKind::ServiceAccountJwk)
                .unwrap();
            app.offboard.form.as_mut().unwrap().cursor = cursor;
        }

        handle_key(&mut app, key(KeyCode::Char(' ')), Mode::Confirm);
        assert!(
            app.offboard
                .form
                .as_ref()
                .unwrap()
                .accepted
                .contains(&TargetKind::ServiceAccountJwk)
        );
    }

    #[test]
    fn prod_enter_routes_through_the_prod_guard() {
        let mut departing = tenant("prod", Some(DUP_SA));
        departing.theme = TenantTheme::Production;
        let mut app = App::for_test(vec![departing.clone()], View::Esvs);
        let plan = spec::plan(&departing, &present(), &[]);
        open_confirm_for_test(&mut app, present(), plan);

        handle_key(&mut app, key(KeyCode::Enter), Mode::Confirm);

        assert_eq!(app.input_mode, InputMode::ProdConfirm);
        assert!(matches!(
            app.prod_confirm.pending,
            Some(PendingProdAction::Offboard(ProdAction::Execute))
        ));
        assert!(app.offboard.form.is_some());
    }
}
