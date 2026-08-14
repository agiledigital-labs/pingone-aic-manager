//! Probe disk/vault for the planner, and execute a resolved purge.
//!
//! [`spec::plan`] decides what is safe. This module looks, then does — it
//! must not invent a second notion of safety. Callers feed user choices
//! through [`crate::offboard::spec::DeletePlan::resolve_purge`] before they
//! reach [`execute`].

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::config::{
    ProjectConfig, Tenant, VaultArtifact, tenant_file_name, write_current_context,
};
use crate::offboard::spec::{self, DeletePlan, Inventory, ResolvedPurge, Survivor, TargetKind};
use crate::undo::{DiskLog, UndoLog};
use crate::{Error, Result};

/// Per-tenant vault contents the sharing guard needs, keyed by tenant name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VaultView {
    pub jwks: HashSet<String>,
    pub log_keys: HashMap<String, String>,
    pub issuer_kids: HashMap<String, String>,
}

/// On-disk presence for the path-shaped targets of one departing tenant.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathPresence {
    pub logs_database: bool,
    pub idm_store: bool,
    pub workspace: bool,
    pub sync_state: bool,
    pub undo_entries: bool,
}

/// Roots used to address local artifacts. Production uses
/// [`Layout::live`]; tests point this at a temp directory so nothing
/// touches the real `.aic/`.
#[derive(Debug, Clone)]
pub struct Layout {
    pub aic_dir: PathBuf,
    pub workspace_dir: PathBuf,
}

impl Layout {
    pub fn live() -> Self {
        Self {
            aic_dir: ProjectConfig::dir(),
            workspace_dir: ProjectConfig::workspace_dir(),
        }
    }

    pub fn logs_db(&self, tenant: &str) -> PathBuf {
        self.aic_dir
            .join("logs")
            .join(format!("{}.duckdb", tenant_file_name(tenant)))
    }

    pub fn idm_store(&self, tenant: &str) -> PathBuf {
        self.aic_dir
            .join("idmstore")
            .join(format!("{}.sqlite", tenant_file_name(tenant)))
    }

    pub fn workspace(&self, tenant: &str) -> PathBuf {
        self.workspace_dir.join(tenant)
    }

    pub fn sync_state(&self, tenant: &str) -> PathBuf {
        self.workspace(tenant).join(".aic-sync")
    }

    pub fn undo_log(&self) -> PathBuf {
        self.aic_dir.join("undo.log")
    }

    pub fn backup_path(&self, tenant: &str, now: DateTime<Utc>) -> PathBuf {
        self.aic_dir.join("backups").join(format!(
            "tenant-{}-{}.json",
            tenant_file_name(tenant),
            now.format("%Y%m%dT%H%M%SZ")
        ))
    }
}

/// Reconstructible record of a removed `[[tenant]]` entry.
///
/// This is **not** an undo. The vault may be encrypted; dumping a private
/// JWK (or a log-key secret) beside this file would be a credentials
/// regression. The file holds identifiers so the entry can be re-onboarded
/// with the same values, and nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantBackup {
    pub tenant: Tenant,
    pub sa_id: Option<String>,
    pub api_key_id: Option<String>,
    pub kid: Option<String>,
}

impl TenantBackup {
    pub fn from_inventory(tenant: &Tenant, inventory: &Inventory) -> Self {
        Self {
            tenant: tenant.clone(),
            sa_id: nonempty_owned(tenant.sa_id.as_deref()),
            api_key_id: nonempty_owned(inventory.log_api_key_id.as_deref()),
            kid: nonempty_owned(inventory.issuer_kid.as_deref()),
        }
    }
}

/// One step of [`execute`]. Failures are recorded; they do not abort the
/// rest of the run, except that a failed vault or filesystem step skips
/// removing the config entry so the tenant stays addressable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutcome {
    pub step: Step,
    pub status: StepStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Backup,
    RemoteIssuer,
    Vault(TargetKind),
    Path(TargetKind),
    ConfigEntry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepStatus {
    Ok,
    Failed(String),
    Skipped(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecuteReport {
    pub backup_path: Option<PathBuf>,
    pub steps: Vec<StepOutcome>,
    pub config_removed: bool,
    pub next_context: Option<String>,
    pub default_tenant: String,
    pub remote_error: Option<String>,
}

impl ExecuteReport {
    pub fn failed(&self) -> bool {
        self.steps
            .iter()
            .any(|step| matches!(step.status, StepStatus::Failed(_)))
    }
}

/// I/O seam for [`execute`]. The live CLI/TUI use [`LiveIo`]; tests inject
/// failures without touching `.aic/`.
pub trait ExecuteIo {
    fn write_backup(&mut self, path: &Path, bytes: &[u8]) -> Result<()>;
    fn remove_remote_issuer(
        &mut self,
        tenant: &str,
        kid: &str,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
    fn remove_vault(
        &mut self,
        artifact: VaultArtifact,
        tenant: &str,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
    fn remove_path(&mut self, path: &Path, directory: bool) -> Result<()>;
    fn forget_undo(&mut self, path: &Path, tenant: &str) -> Result<usize>;
    fn save_config(&mut self, config: &ProjectConfig) -> Result<()>;
    fn write_context(&mut self, name: Option<&str>) -> Result<()>;
}

/// Production I/O: agent vault verbs, local files, and `ProjectConfig::save`.
///
/// `confirmed_prod` defaults to `false` so a bare [`LiveIo::default`] cannot
/// silently authorise a production issuer write.
pub struct LiveIo {
    pub realm: String,
    pub confirmed_prod: bool,
}

impl Default for LiveIo {
    fn default() -> Self {
        Self {
            realm: "alpha".into(),
            confirmed_prod: false,
        }
    }
}

impl LiveIo {
    pub fn new(confirmed_prod: bool) -> Self {
        Self {
            realm: "alpha".into(),
            confirmed_prod,
        }
    }
}

impl ExecuteIo for LiveIo {
    fn write_backup(&mut self, path: &Path, bytes: &[u8]) -> Result<()> {
        write_backup_file(path, bytes)
    }

    async fn remove_remote_issuer(&mut self, tenant: &str, kid: &str) -> Result<()> {
        let issuer = crate::jwtbearer::api::read_issuer(
            tenant,
            &self.realm,
            crate::jwtbearer::ops::DEFAULT_ISSUER_ID,
        )
        .await?;
        crate::jwtbearer::ops::remove_key_from_issuer(
            tenant,
            &self.realm,
            kid,
            issuer,
            self.confirmed_prod,
        )
        .await?;
        Ok(())
    }

    async fn remove_vault(&mut self, artifact: VaultArtifact, tenant: &str) -> Result<()> {
        let agent = crate::agent::AgentClient::connect_or_spawn().await?;
        agent.remove_secret(artifact.kind(), tenant).await
    }

    fn remove_path(&mut self, path: &Path, directory: bool) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }
        if directory {
            std::fs::remove_dir_all(path)?;
        } else {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    fn forget_undo(&mut self, path: &Path, tenant: &str) -> Result<usize> {
        let mut log = DiskLog::load(path.to_path_buf())?;
        log.forget_tenant(tenant)
    }

    fn save_config(&mut self, config: &ProjectConfig) -> Result<()> {
        config.save()
    }

    fn write_context(&mut self, name: Option<&str>) -> Result<()> {
        match name {
            Some(name) => write_current_context(name),
            None => {
                let path = crate::config::current_context_path();
                if path.exists() {
                    std::fs::remove_file(path)?;
                }
                Ok(())
            }
        }
    }
}

/// Write `bytes` at mode 0600 without clobbering an existing backup.
pub fn write_backup_file(path: &Path, bytes: &[u8]) -> Result<()> {
    crate::access::ops::write_private_file(path, bytes, true)
}

pub fn inventory_from(tenant: &str, vault: &VaultView, paths: &PathPresence) -> Inventory {
    Inventory {
        service_account_jwk: vault.jwks.contains(tenant),
        log_api_key_id: vault.log_keys.get(tenant).cloned(),
        issuer_kid: vault.issuer_kids.get(tenant).cloned(),
        logs_database: paths.logs_database,
        idm_store: paths.idm_store,
        workspace: paths.workspace,
        sync_state: paths.sync_state,
        undo_entries: paths.undo_entries,
    }
}

pub fn survivors_from(departing: &str, tenants: &[Tenant], vault: &VaultView) -> Vec<Survivor> {
    tenants
        .iter()
        .filter(|tenant| tenant.name != departing)
        .map(|tenant| {
            Survivor::from_tenant(
                tenant,
                vault.log_keys.get(&tenant.name).cloned(),
                vault.issuer_kids.get(&tenant.name).cloned(),
            )
        })
        .collect()
}

pub fn probe_paths(tenant: &str, layout: &Layout) -> PathPresence {
    PathPresence {
        logs_database: layout.logs_db(tenant).exists(),
        idm_store: layout.idm_store(tenant).exists(),
        workspace: layout.workspace(tenant).is_dir(),
        sync_state: layout.sync_state(tenant).exists(),
        undo_entries: undo_mentions(tenant, &layout.undo_log()),
    }
}

fn undo_mentions(tenant: &str, path: &Path) -> bool {
    match DiskLog::load(path.to_path_buf()) {
        Ok(log) => log
            .list(usize::MAX)
            .iter()
            .any(|entry| entry.tenant == tenant),
        Err(_) => true,
    }
}

/// Load per-tenant vault maps via the agent. An empty vault (no artifact
/// files) is probed without connecting; a locked agent is an error, not an
/// empty map — treating locked as absent would skip offered credentials and
/// then delete the config entry, stranding them.
pub async fn probe_vault(names: &[String]) -> Result<VaultView> {
    if !vault_files_exist() {
        return Ok(VaultView::default());
    }
    let mut view = VaultView::default();
    for name in names {
        if optional_secret(VaultArtifact::Jwks, name).await?.is_some() {
            view.jwks.insert(name.clone());
        }
        if let Some(value) = optional_secret(VaultArtifact::LogKeys, name).await? {
            let pair: crate::logs::LogKeyPair = serde_json::from_value(value)?;
            if let Some(id) = nonempty_owned(Some(pair.api_key_id.as_str())) {
                view.log_keys.insert(name.clone(), id);
            }
        }
        if let Some(value) = optional_secret(VaultArtifact::JwtBearerKeys, name).await? {
            let record: crate::jwtbearer::KeyRecord = serde_json::from_value(value)?;
            if let Some(kid) = nonempty_owned(Some(record.kid.as_str())) {
                view.issuer_kids.insert(name.clone(), kid);
            }
        }
    }
    Ok(view)
}

fn vault_files_exist() -> bool {
    let dir = ProjectConfig::dir();
    VaultArtifact::ALL.iter().any(|artifact| {
        let stem = artifact.file_stem();
        dir.join(format!("{stem}.enc")).exists() || dir.join(format!("{stem}.plain")).exists()
    })
}

async fn optional_secret(
    artifact: VaultArtifact,
    tenant: &str,
) -> Result<Option<serde_json::Value>> {
    let agent = crate::agent::AgentClient::connect_or_spawn().await?;
    match agent.get_secret(artifact.kind(), tenant).await {
        Ok(value) => Ok(Some(value)),
        Err(Error::SecretMissing { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Drop `name` from `config` and retarget `default_tenant` if it named it.
pub fn drop_tenant(config: &mut ProjectConfig, name: &str) {
    config.tenants.retain(|tenant| tenant.name != name);
    if config.default_tenant == name {
        config.default_tenant = config
            .tenants
            .first()
            .map(|tenant| tenant.name.clone())
            .unwrap_or_default();
    }
}

/// Where `current-context` should point after `name` is gone. `config` is
/// the document *after* [`drop_tenant`].
pub fn next_context(
    current: Option<&str>,
    removed: &str,
    config: &ProjectConfig,
) -> Option<String> {
    match current {
        Some(current) if current == removed => {
            if config.default_tenant.is_empty() {
                None
            } else {
                Some(config.default_tenant.clone())
            }
        }
        Some(current) => Some(current.to_string()),
        None => None,
    }
}

/// Apply a resolved `purge` set.
///
/// Order is load-bearing and must not be inverted by a later refactor:
///
/// 1. The remote issuer edit needs a bearer for the tenant being removed,
///    so it runs while that tenant's credentials still exist.
/// 2. Vault entries next.
/// 3. Data stores and directories.
/// 4. The `[[tenant]]` row in `.aic/config.toml` **last**. Every `aic`
///    command resolves a tenant by name from that file; removing the row
///    first would strand a failed vault purge as artifacts no command can
///    name. A failure in (2) or (3) therefore skips (4) so the run stays
///    retryable. A failed remote step does not skip the local purge — the
///    leftover kid is reported for console cleanup.
///
/// Tenant deletion is not recorded in the undo log; the backup is the
/// recovery story.
pub async fn execute<I: ExecuteIo>(
    tenant: &Tenant,
    config: &ProjectConfig,
    current_context: Option<&str>,
    inventory: &Inventory,
    purge: &ResolvedPurge,
    layout: &Layout,
    io: &mut I,
) -> ExecuteReport {
    let mut steps = Vec::new();
    let mut blocking_failure = false;
    let mut remote_error = None;

    let Some(backup_path) = write_backup_step(tenant, inventory, layout, io, &mut steps) else {
        return aborted(steps, current_context, config);
    };

    if purge.contains(&TargetKind::IssuerSigningKey) {
        match inventory.issuer_kid.as_deref() {
            Some(kid) => match io.remove_remote_issuer(&tenant.name, kid).await {
                Ok(()) => steps.push(outcome(Step::RemoteIssuer, StepStatus::Ok)),
                Err(error) => {
                    remote_error = Some(error.to_string());
                    steps.push(outcome(
                        Step::RemoteIssuer,
                        StepStatus::Failed(error.to_string()),
                    ));
                }
            },
            None => steps.push(outcome(
                Step::RemoteIssuer,
                StepStatus::Skipped("no kid in inventory"),
            )),
        }
    }

    for (kind, artifact) in [
        (TargetKind::ServiceAccountJwk, VaultArtifact::Jwks),
        (TargetKind::LogApiKey, VaultArtifact::LogKeys),
        (TargetKind::IssuerSigningKey, VaultArtifact::JwtBearerKeys),
    ] {
        if !purge.contains(&kind) {
            continue;
        }
        match io.remove_vault(artifact, &tenant.name).await {
            Ok(()) => steps.push(outcome(Step::Vault(kind), StepStatus::Ok)),
            Err(error) => {
                blocking_failure = true;
                steps.push(outcome(
                    Step::Vault(kind),
                    StepStatus::Failed(error.to_string()),
                ));
            }
        }
    }

    if purge.contains(&TargetKind::LogsDatabase) {
        push_path(
            &mut steps,
            &mut blocking_failure,
            TargetKind::LogsDatabase,
            io.remove_path(&layout.logs_db(&tenant.name), false),
        );
    }
    if purge.contains(&TargetKind::IdmStore) {
        push_path(
            &mut steps,
            &mut blocking_failure,
            TargetKind::IdmStore,
            io.remove_path(&layout.idm_store(&tenant.name), false),
        );
    }

    let workspace_purged = purge.contains(&TargetKind::Workspace);
    if workspace_purged {
        match io.remove_path(&layout.workspace(&tenant.name), true) {
            Ok(()) => {
                steps.push(outcome(Step::Path(TargetKind::Workspace), StepStatus::Ok));
                if purge.contains(&TargetKind::SyncState) {
                    steps.push(outcome(Step::Path(TargetKind::SyncState), StepStatus::Ok));
                }
            }
            Err(error) => {
                blocking_failure = true;
                steps.push(outcome(
                    Step::Path(TargetKind::Workspace),
                    StepStatus::Failed(error.to_string()),
                ));
                if purge.contains(&TargetKind::SyncState) {
                    push_path(
                        &mut steps,
                        &mut blocking_failure,
                        TargetKind::SyncState,
                        io.remove_path(&layout.sync_state(&tenant.name), true),
                    );
                }
            }
        }
    } else if purge.contains(&TargetKind::SyncState) {
        push_path(
            &mut steps,
            &mut blocking_failure,
            TargetKind::SyncState,
            io.remove_path(&layout.sync_state(&tenant.name), true),
        );
    }

    if purge.contains(&TargetKind::UndoLog) {
        match io.forget_undo(&layout.undo_log(), &tenant.name) {
            Ok(_) => steps.push(outcome(Step::Path(TargetKind::UndoLog), StepStatus::Ok)),
            Err(error) => {
                blocking_failure = true;
                steps.push(outcome(
                    Step::Path(TargetKind::UndoLog),
                    StepStatus::Failed(error.to_string()),
                ));
            }
        }
    }

    let mut next = config.clone();
    drop_tenant(&mut next, &tenant.name);
    let next_ctx = next_context(current_context, &tenant.name, &next);
    let default_tenant = next.default_tenant.clone();

    if blocking_failure {
        steps.push(outcome(
            Step::ConfigEntry,
            StepStatus::Skipped("earlier local step failed; tenant left addressable"),
        ));
        return ExecuteReport {
            backup_path: Some(backup_path),
            steps,
            config_removed: false,
            next_context: current_context.map(str::to_string),
            default_tenant: config.default_tenant.clone(),
            remote_error,
        };
    }

    match io.save_config(&next) {
        Ok(()) => {
            if let Err(error) = io.write_context(next_ctx.as_deref()) {
                steps.push(outcome(
                    Step::ConfigEntry,
                    StepStatus::Failed(format!("config saved, context not updated: {error}")),
                ));
            } else {
                steps.push(outcome(Step::ConfigEntry, StepStatus::Ok));
            }
            ExecuteReport {
                backup_path: Some(backup_path),
                steps,
                config_removed: true,
                next_context: next_ctx,
                default_tenant,
                remote_error,
            }
        }
        Err(error) => {
            steps.push(outcome(
                Step::ConfigEntry,
                StepStatus::Failed(error.to_string()),
            ));
            ExecuteReport {
                backup_path: Some(backup_path),
                steps,
                config_removed: false,
                next_context: current_context.map(str::to_string),
                default_tenant: config.default_tenant.clone(),
                remote_error,
            }
        }
    }
}

fn write_backup_step<I: ExecuteIo>(
    tenant: &Tenant,
    inventory: &Inventory,
    layout: &Layout,
    io: &mut I,
    steps: &mut Vec<StepOutcome>,
) -> Option<PathBuf> {
    let backup = TenantBackup::from_inventory(tenant, inventory);
    let bytes = match serde_json::to_vec_pretty(&backup) {
        Ok(bytes) => bytes,
        Err(error) => {
            steps.push(outcome(Step::Backup, StepStatus::Failed(error.to_string())));
            return None;
        }
    };
    let path = layout.backup_path(&tenant.name, Utc::now());
    match io.write_backup(&path, &bytes) {
        Ok(()) => {
            steps.push(outcome(Step::Backup, StepStatus::Ok));
            Some(path)
        }
        Err(error) => {
            steps.push(outcome(Step::Backup, StepStatus::Failed(error.to_string())));
            None
        }
    }
}

fn aborted(
    steps: Vec<StepOutcome>,
    current_context: Option<&str>,
    config: &ProjectConfig,
) -> ExecuteReport {
    ExecuteReport {
        backup_path: None,
        steps,
        config_removed: false,
        next_context: current_context.map(str::to_string),
        default_tenant: config.default_tenant.clone(),
        remote_error: None,
    }
}

fn push_path(
    steps: &mut Vec<StepOutcome>,
    blocking_failure: &mut bool,
    kind: TargetKind,
    result: Result<()>,
) {
    match result {
        Ok(()) => steps.push(outcome(Step::Path(kind), StepStatus::Ok)),
        Err(error) => {
            *blocking_failure = true;
            steps.push(outcome(
                Step::Path(kind),
                StepStatus::Failed(error.to_string()),
            ));
        }
    }
}

fn outcome(step: Step, status: StepStatus) -> StepOutcome {
    StepOutcome { step, status }
}

fn nonempty_owned(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Build the plan the CLI/TUI both start from. `purge` is not decided here.
pub fn plan_for(
    tenant: &Tenant,
    tenants: &[Tenant],
    vault: &VaultView,
    paths: &PathPresence,
) -> (Inventory, Vec<Survivor>, DeletePlan) {
    let inventory = inventory_from(&tenant.name, vault, paths);
    let survivors = survivors_from(&tenant.name, tenants, vault);
    let plan = spec::plan(tenant, &inventory, &survivors);
    (inventory, survivors, plan)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::config::{CredentialSource, Provenance, TenantTheme};

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

    fn config(tenants: Vec<Tenant>, default: &str) -> ProjectConfig {
        ProjectConfig {
            project: "test".into(),
            default_tenant: default.into(),
            tenants,
        }
    }

    fn full_inventory() -> Inventory {
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

    struct RecordingIo {
        events: Vec<String>,
        fail_vault: Option<VaultArtifact>,
        vault_removed: Vec<(VaultArtifact, String)>,
        paths_removed: Vec<PathBuf>,
        saved_config: Option<ProjectConfig>,
        context: Option<Option<String>>,
        backup: Option<Vec<u8>>,
        fail_remote: bool,
    }

    impl RecordingIo {
        fn new() -> Self {
            Self {
                events: Vec::new(),
                fail_vault: None,
                vault_removed: Vec::new(),
                paths_removed: Vec::new(),
                saved_config: None,
                context: None,
                backup: None,
                fail_remote: false,
            }
        }
    }

    impl ExecuteIo for RecordingIo {
        fn write_backup(&mut self, _path: &Path, bytes: &[u8]) -> Result<()> {
            self.events.push("backup".into());
            self.backup = Some(bytes.to_vec());
            Ok(())
        }

        async fn remove_remote_issuer(&mut self, tenant: &str, kid: &str) -> Result<()> {
            self.events.push(format!("remote:{tenant}:{kid}"));
            if self.fail_remote {
                return Err(Error::Config("injected remote failure".into()));
            }
            Ok(())
        }

        async fn remove_vault(&mut self, artifact: VaultArtifact, tenant: &str) -> Result<()> {
            self.events.push(format!("vault:{}", artifact.kind()));
            if self.fail_vault == Some(artifact) {
                return Err(Error::Config("injected vault failure".into()));
            }
            self.vault_removed.push((artifact, tenant.to_string()));
            Ok(())
        }

        fn remove_path(&mut self, path: &Path, _directory: bool) -> Result<()> {
            self.events.push(format!("path:{}", path.display()));
            self.paths_removed.push(path.to_path_buf());
            Ok(())
        }

        fn forget_undo(&mut self, _path: &Path, tenant: &str) -> Result<usize> {
            self.events.push(format!("undo:{tenant}"));
            Ok(1)
        }

        fn save_config(&mut self, config: &ProjectConfig) -> Result<()> {
            self.events.push("config".into());
            self.saved_config = Some(config.clone());
            Ok(())
        }

        fn write_context(&mut self, name: Option<&str>) -> Result<()> {
            self.context = Some(name.map(str::to_string));
            Ok(())
        }
    }

    fn temp_layout() -> (Layout, PathBuf) {
        let root = std::env::temp_dir().join(format!("aic-offboard-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        (
            Layout {
                aic_dir: root.join(".aic"),
                workspace_dir: root.join("workspace"),
            },
            root,
        )
    }

    #[tokio::test]
    async fn delete_keys_does_not_touch_a_refused_target() {
        let departing = tenant("UAT", Some(DUP_SA));
        let keep = tenant("uat", Some(UAT_SA));
        let mut vault = VaultView::default();
        vault.jwks.insert("UAT".into());
        vault.jwks.insert("uat".into());
        vault.log_keys.insert("UAT".into(), SHARED_LOG_KEY.into());
        vault.log_keys.insert("uat".into(), SHARED_LOG_KEY.into());
        let paths = PathPresence {
            logs_database: true,
            ..PathPresence::default()
        };
        let (inventory, _, plan) = plan_for(&departing, &[departing.clone(), keep], &vault, &paths);
        let requested: Vec<_> = plan
            .targets
            .iter()
            .filter(|target| matches!(target.decision, spec::TargetDecision::Offered { .. }))
            .map(|target| target.kind)
            .collect();
        let purge = plan.resolve_purge(requested);
        assert!(!purge.contains(&TargetKind::LogApiKey));

        let (layout, root) = temp_layout();
        let cfg = config(vec![departing.clone(), tenant("uat", Some(UAT_SA))], "uat");
        let mut io = RecordingIo::new();
        let report = execute(
            &departing,
            &cfg,
            Some("UAT"),
            &inventory,
            &purge,
            &layout,
            &mut io,
        )
        .await;

        assert!(report.config_removed);
        assert!(
            !io.vault_removed
                .iter()
                .any(|(artifact, _)| *artifact == VaultArtifact::LogKeys),
            "refused log key must stay in the vault even under --delete-keys"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn vault_failure_leaves_the_config_entry() {
        // The config row is the name everything else is addressed by. A
        // vault failure after removing it would strand the leftover secret.
        let departing = tenant("UAT", Some(DUP_SA));
        let inventory = full_inventory();
        let plan = spec::plan(&departing, &inventory, &[]);
        let purge = plan.resolve_purge(TargetKind::ALL);
        let (layout, root) = temp_layout();
        let cfg = config(vec![departing.clone(), tenant("uat", Some(UAT_SA))], "uat");
        let mut io = RecordingIo::new();
        io.fail_vault = Some(VaultArtifact::Jwks);

        let report = execute(
            &departing,
            &cfg,
            Some("UAT"),
            &inventory,
            &purge,
            &layout,
            &mut io,
        )
        .await;

        assert!(!report.config_removed);
        assert!(io.saved_config.is_none());
        assert!(
            io.events.iter().any(|event| event.starts_with("vault:")),
            "vault step must run before the config decision: {:?}",
            io.events
        );
        assert!(
            !io.events.iter().any(|event| event == "config"),
            "config must not be saved after a vault failure: {:?}",
            io.events
        );
        let backup_at = io.events.iter().position(|event| event == "backup");
        let vault_at = io
            .events
            .iter()
            .position(|event| event.starts_with("vault:"));
        assert!(backup_at < vault_at);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn remote_failure_does_not_block_the_local_purge() {
        let departing = tenant("UAT", Some(DUP_SA));
        let inventory = full_inventory();
        let plan = spec::plan(&departing, &inventory, &[]);
        let purge = plan.resolve_purge(TargetKind::ALL);
        let (layout, root) = temp_layout();
        let cfg = config(vec![departing.clone()], "UAT");
        let mut io = RecordingIo::new();
        io.fail_remote = true;

        let report = execute(
            &departing,
            &cfg,
            Some("UAT"),
            &inventory,
            &purge,
            &layout,
            &mut io,
        )
        .await;

        assert!(report.config_removed);
        assert!(report.remote_error.is_some());
        assert!(io.saved_config.is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn drop_tenant_retargets_default_and_clears_the_last_entry() {
        let mut cfg = config(
            vec![tenant("uat", Some(UAT_SA)), tenant("UAT", Some(DUP_SA))],
            "UAT",
        );
        drop_tenant(&mut cfg, "UAT");
        assert_eq!(cfg.tenants.len(), 1);
        assert_eq!(cfg.default_tenant, "uat");
        assert_eq!(
            next_context(Some("UAT"), "UAT", &cfg).as_deref(),
            Some("uat")
        );

        drop_tenant(&mut cfg, "uat");
        assert!(cfg.tenants.is_empty());
        assert_eq!(cfg.default_tenant, "");
        assert_eq!(next_context(Some("uat"), "uat", &cfg), None);
    }

    #[test]
    fn next_context_leaves_an_unrelated_pointer_alone() {
        let cfg = config(vec![tenant("uat", Some(UAT_SA))], "uat");
        assert_eq!(
            next_context(Some("uat"), "UAT", &cfg).as_deref(),
            Some("uat")
        );
    }

    #[tokio::test]
    async fn execute_last_tenant_clears_context_and_default() {
        let departing = tenant("UAT", Some(DUP_SA));
        let inventory = Inventory {
            service_account_jwk: true,
            ..Inventory::default()
        };
        let plan = spec::plan(&departing, &inventory, &[]);
        let purge = plan.resolve_purge([TargetKind::ServiceAccountJwk]);
        let (layout, root) = temp_layout();
        let cfg = config(vec![departing.clone()], "UAT");
        let mut io = RecordingIo::new();

        let report = execute(
            &departing,
            &cfg,
            Some("UAT"),
            &inventory,
            &purge,
            &layout,
            &mut io,
        )
        .await;

        assert!(report.config_removed);
        assert_eq!(report.next_context, None);
        assert_eq!(report.default_tenant, "");
        let saved = io.saved_config.expect("config saved");
        assert!(saved.tenants.is_empty());
        assert_eq!(saved.default_tenant, "");
        assert_eq!(io.context, Some(None));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn backup_holds_identifiers_and_no_secret_material() {
        let departing = Tenant {
            name: "UAT".into(),
            base_url: UAT_URL.into(),
            theme: TenantTheme::Sandbox,
            sa_id: Some(DUP_SA.into()),
            scopes: vec!["fr:am:*".into()],
            provenance: Provenance {
                service_account: Some(CredentialSource::Created),
                log_key: Some(CredentialSource::External),
            },
        };
        let inventory = Inventory {
            service_account_jwk: true,
            log_api_key_id: Some(SHARED_LOG_KEY.into()),
            issuer_kid: Some("kid-uat".into()),
            ..Inventory::default()
        };
        let backup = TenantBackup::from_inventory(&departing, &inventory);
        let bytes = serde_json::to_vec_pretty(&backup).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let keys: HashSet<_> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            HashSet::from(["tenant", "sa_id", "api_key_id", "kid"])
        );
        assert_eq!(value["sa_id"], DUP_SA);
        assert_eq!(value["api_key_id"], SHARED_LOG_KEY);
        assert_eq!(value["kid"], "kid-uat");

        let text = String::from_utf8(bytes.clone()).unwrap();
        for secret in ["api_key_secret", "private_jwk", "\"d\"", "\"p\"", "\"q\""] {
            assert!(
                !text.contains(secret),
                "backup must not contain {secret}: {text}"
            );
        }

        let dir = std::env::temp_dir().join(format!("aic-backup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tenant-UAT-test.json");
        write_backup_file(&path, &bytes).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "backup must be mode 0600, got {mode:o}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn workspace_purge_reports_sync_removed_not_kept() {
        let departing = tenant("UAT", Some(DUP_SA));
        let inventory = Inventory {
            workspace: true,
            sync_state: true,
            ..Inventory::default()
        };
        let plan = spec::plan(&departing, &inventory, &[]);
        let purge = plan.resolve_purge([TargetKind::Workspace]);
        assert!(purge.contains(&TargetKind::SyncState));

        let (layout, root) = temp_layout();
        let cfg = config(vec![departing.clone()], "UAT");
        let mut io = RecordingIo::new();
        let report = execute(&departing, &cfg, None, &inventory, &purge, &layout, &mut io).await;

        let sync = report
            .steps
            .iter()
            .find(|step| step.step == Step::Path(TargetKind::SyncState))
            .expect("sync outcome");
        assert_eq!(sync.status, StepStatus::Ok);
        assert!(
            !io.paths_removed
                .iter()
                .any(|path| path.ends_with(".aic-sync")),
            "sync dir is inside the workspace; do not delete it separately: {:?}",
            io.paths_removed
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
