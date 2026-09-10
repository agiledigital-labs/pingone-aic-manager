//! Kind-agnostic sync engine: snapshot store, pull, push, status, diff.
//!
//! This module never matches on [`Kind`] — every per-kind concern (including
//! whether it's realm-scoped) is reached through the `Kind` methods. Conflict
//! detection is **content-based** (scripts and IDM endpoints both lack `_rev`):
//! we keep the last-synced raw config as a snapshot and compare *decoded source
//! bytes* (CLAUDE.md §5, `docs/api/04-scripts.md`).
//!
//! The workspace is **per-tenant**. AM scripts are realm-scoped (stored under
//! `am/<realm>/…`); IDM endpoints are tenant-global. The snapshot/manifest key
//! each entry on `realm: Option<String>` (Some for AM, None for IDM) so a
//! same-named script in alpha and bravo never collide.

pub use super::gate::SyntaxGate;
use super::gate::{Gated, write_checked};
use super::syntax::Refusal;
use super::{Kind, RemoteRef, RemoteScript};
use crate::cli::force::OperationAndSyntaxCheckForce;
use crate::config::ProjectConfig;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Which scripts an operation targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    All,
    Name(String),
    Prefix(String),
}

impl Selector {
    fn matches(&self, r: &RemoteRef) -> bool {
        match self {
            Selector::All => true,
            Selector::Name(n) => r.name == *n,
            Selector::Prefix(prefix) => r.name.starts_with(prefix),
        }
    }
}

/// Per-script outcome of a pull.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullStatus {
    Created,
    Updated,
    Unchanged,
    /// Existing local source differed from the fetched remote; we backed it up
    /// to the given path before overwriting, independent of snapshot state.
    LocalBackedUp(PathBuf),
}

#[derive(Debug, Clone)]
pub struct PullOutcome {
    pub name: String,
    pub kind: Kind,
    pub realm: Option<String>,
    pub status: PullStatus,
}

/// One namespace/selector pair included in a protected pull preflight.
#[derive(Debug, Clone)]
pub struct PullTarget {
    pub realm: String,
    pub kind: Kind,
    pub selector: Selector,
}

#[derive(Debug)]
struct PreparedPull {
    realm: String,
    script: RemoteScript,
    remote_source: Vec<u8>,
    local: Option<Vec<u8>>,
    protected: bool,
}

/// Fetched remote content and local decisions for one atomic authorization
/// boundary. No workspace file or snapshot changes until [`Self::install`].
#[derive(Debug)]
pub struct PullPlan {
    store: SnapshotStore,
    workspace_tree: PathBuf,
    entries: Vec<PreparedPull>,
}

impl PullPlan {
    #[cfg(test)]
    pub(crate) fn protected_for_test(names: &[&str]) -> Self {
        Self {
            store: SnapshotStore {
                dir: PathBuf::new(),
            },
            workspace_tree: PathBuf::new(),
            entries: names
                .iter()
                .map(|name| PreparedPull {
                    realm: String::new(),
                    script: RemoteScript {
                        reference: RemoteRef {
                            kind: Kind::IdmSyncMapping,
                            id: format!("sync/{name}"),
                            name: (*name).to_string(),
                            context: None,
                            is_default: false,
                            evaluator_version: None,
                        },
                        raw_config: Value::Null,
                    },
                    remote_source: b"remote".to_vec(),
                    local: Some(b"local".to_vec()),
                    protected: true,
                })
                .collect(),
        }
    }

    pub fn protected_refs(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| entry.protected)
            .map(|entry| {
                super::full_name(
                    entry.script.reference.kind,
                    entry
                        .script
                        .reference
                        .kind
                        .realm_scoped()
                        .then_some(entry.realm.as_str()),
                    &entry.script.reference.name,
                )
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Install the already-fetched content after the surface has authorized
    /// every protected entry. Recheck all local bytes first so an edit made
    /// while a confirmation modal was open cannot be overwritten unseen.
    pub fn install(self, skip_backup: bool) -> Result<Vec<PullOutcome>> {
        for entry in &self.entries {
            let path =
                workspace_file_in(&self.workspace_tree, &entry.realm, &entry.script.reference);
            if read_local(&path)? != entry.local {
                return Err(Error::Config(format!(
                    "local source changed during pull preflight: {}; nothing was installed",
                    super::full_name(
                        entry.script.reference.kind,
                        entry
                            .script
                            .reference
                            .kind
                            .realm_scoped()
                            .then_some(entry.realm.as_str()),
                        &entry.script.reference.name,
                    )
                )));
            }
        }

        self.entries
            .iter()
            .map(|entry| {
                let status = install_remote(
                    &self.store,
                    &self.workspace_tree,
                    &entry.realm,
                    &entry.script,
                    &entry.remote_source,
                    skip_backup,
                )?;
                Ok(PullOutcome {
                    name: entry.script.reference.name.clone(),
                    kind: entry.script.reference.kind,
                    realm: entry
                        .script
                        .reference
                        .kind
                        .realm_scoped()
                        .then(|| entry.realm.clone()),
                    status,
                })
            })
            .collect()
    }
}

/// Identifies the exact source bytes an operation acted on, so a surface
/// holding a stale result can tell that the file has moved on. A digest rather
/// than the bytes: this is only ever compared for equality, and a refusal may
/// outlive the push that produced it by minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceId(u64);

/// The [`SourceId`] of some source bytes.
pub fn source_id(bytes: &[u8]) -> SourceId {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    SourceId(hasher.finish())
}

/// The [`SourceId`] of what is in the workspace for `c` right now, or `None`
/// when there is no local file. Deliberately *not* the snapshot fallback that
/// [`preview_source`] uses: this answers "has the operator edited it since",
/// and the snapshot is not the operator's copy.
pub fn local_source_id(tenant: &str, c: &Candidate) -> Option<SourceId> {
    let realm = c.realm.as_deref().unwrap_or_default();
    let r = ref_of(c);
    read_local(&workspace_file(tenant, realm, &r))
        .ok()
        .flatten()
        .map(|bytes| source_id(&bytes))
}

/// Per-script outcome of a push.
#[must_use = "a discarded push outcome reports a refusal as a successful push"]
#[derive(Debug, Clone)]
pub enum PushOutcome {
    Pushed,
    /// The tenant accepted the write, but a fresh read could not prove that it
    /// holds the submitted source. The snapshot remains unchanged.
    NotConfirmed(ConfirmationFailure),
    /// Local matches the last-synced snapshot — nothing to push.
    Unchanged,
    /// Remote already equals local — snapshot refreshed, no write needed.
    AlreadyInSync,
    /// Remote drifted from the snapshot and doesn't match local. Blocked
    /// unless `--force`. Carries the 3-way texts for display.
    Conflict(ThreeWay),
    /// The gate refused the write, so nothing was written. Distinct from
    /// `Conflict`: `--force` must not override this, because the script would
    /// be stored broken (AM) or become un-routable (IDM).
    ///
    /// `source` identifies the bytes that were checked — **not** whatever is
    /// on disk when this is read. A surface that holds the refusal has to know
    /// which source it was about: the operator can save a fix while the push
    /// is still in flight, and a refusal re-identified by re-reading the file
    /// afterwards would be pinned to the corrected source it says nothing
    /// about.
    Refused {
        refusal: Refusal,
        source: SourceId,
    },
}

/// Why a tenant-accepted write could not be confirmed by an immediate read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmationFailure {
    /// The fresh remote source differed from the exact bytes submitted.
    Mismatch,
    /// The fresh resource could not be fetched or its source could not be
    /// decoded.
    Failed(String),
}

impl ConfirmationFailure {
    /// Conservative wording shared by every surface that displays a failed
    /// confirmation. A mismatch does not prove whether the write never landed
    /// or was replaced immediately afterwards.
    pub fn message(&self) -> String {
        match self {
            Self::Mismatch => "write accepted, but read-back did not match the submitted source — snapshot unchanged; tenant state uncertain".into(),
            Self::Failed(error) => format!(
                "write accepted, but confirmation failed: {error} — snapshot unchanged; tenant state uncertain"
            ),
        }
    }
}

/// The three sides of a content conflict, as decoded UTF-8 (lossy) text.
#[derive(Debug, Clone)]
pub struct ThreeWay {
    pub last_synced: String,
    pub remote: String,
    pub local: String,
}

/// State of one synced script relative to its snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptState {
    InSync,
    LocallyModified,
    RemotelyModified,
    BothModified,
    LocalMissing,
}

#[derive(Debug, Clone)]
pub struct StatusEntry {
    pub name: String,
    pub kind: Kind,
    pub realm: Option<String>,
    pub state: ScriptState,
}

// ---------------------------------------------------------------------------
// Snapshot store: .aic-sync/{manifest.json, configs/<kind>/<realm?>/<file>, backups/}
// ---------------------------------------------------------------------------

/// A synced script's identity plus the realm it belongs to (Some for AM, None
/// for tenant-global IDM). Serialized into the per-tenant manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncedScript {
    #[serde(flatten)]
    pub reference: RemoteRef,
    #[serde(default)]
    pub realm: Option<String>,
}

/// On-disk record of the last-synced state for one tenant (both realms + IDM).
#[derive(Debug)]
pub struct SnapshotStore {
    dir: PathBuf,
}

impl SnapshotStore {
    pub fn open(tenant: &str) -> Self {
        SnapshotStore {
            dir: ProjectConfig::aic_sync_dir(tenant),
        }
    }

    fn manifest_path(&self) -> PathBuf {
        self.dir.join("manifest.json")
    }

    fn config_path(&self, r: &RemoteRef, realm: &str) -> PathBuf {
        // `config_subpath` already namespaces by kind (and realm for AM).
        self.dir
            .join("configs")
            .join(r.kind.config_subpath(r, realm))
    }

    fn pull_snapshot_source(&self, r: &RemoteRef, realm: &str) -> Result<Option<Vec<u8>>> {
        let bytes = match std::fs::read(self.config_path(r, realm)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let Some(raw) = serde_json::from_slice::<Value>(&bytes).ok() else {
            return Ok(None);
        };
        Ok(r.kind.decode_source(&raw).ok())
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.dir.join("backups")
    }

    /// The manifest of every script we've synced for this tenant.
    pub fn load_manifest(&self) -> Result<Vec<SyncedScript>> {
        let path = self.manifest_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = std::fs::read(&path)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn save_manifest(&self, entries: &[SyncedScript]) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.manifest_path(), serde_json::to_vec_pretty(entries)?)?;
        Ok(())
    }

    /// The realm key under which a (kind, realm) entry is stored: Some for
    /// realm-scoped kinds, None otherwise.
    fn realm_key(kind: Kind, realm: &str) -> Option<String> {
        kind.realm_scoped().then(|| realm.to_string())
    }

    /// Insert or replace a manifest entry, keyed by (kind, name, realm).
    fn upsert(&self, entry: &SyncedScript) -> Result<()> {
        let mut entries = self.load_manifest()?;
        if let Some(slot) = entries.iter_mut().find(|e| {
            e.reference.kind == entry.reference.kind
                && e.reference.name == entry.reference.name
                && e.realm == entry.realm
        }) {
            *slot = entry.clone();
        } else {
            entries.push(entry.clone());
        }
        self.save_manifest(&entries)
    }

    fn remove(&self, kind: Kind, name: &str, realm: &str) -> Result<()> {
        let key = Self::realm_key(kind, realm);
        let mut entries = self.load_manifest()?;
        let removed = entries
            .iter()
            .find(|e| e.reference.kind == kind && e.reference.name == name && e.realm == key)
            .cloned();
        entries
            .retain(|e| !(e.reference.kind == kind && e.reference.name == name && e.realm == key));
        self.save_manifest(&entries)?;
        if let Some(entry) = removed {
            let path = self.config_path(&entry.reference, realm);
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    /// Last-synced raw config (the snapshot we forked from), if present.
    pub fn load_config(&self, r: &RemoteRef, realm: &str) -> Result<Option<Value>> {
        let path = self.config_path(r, realm);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path)?;
        Ok(Some(serde_json::from_slice(&bytes)?))
    }

    fn save_config(&self, r: &RemoteRef, realm: &str, raw: &Value) -> Result<()> {
        let path = self.config_path(r, realm);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(raw)?)?;
        Ok(())
    }

    /// Record a script as synced: store its raw config + manifest entry.
    fn record(&self, script: &RemoteScript, realm: &str) -> Result<()> {
        self.save_config(&script.reference, realm, &script.raw_config)?;
        self.upsert(&SyncedScript {
            reference: script.reference.clone(),
            realm: Self::realm_key(script.reference.kind, realm),
        })
    }

    fn lookup(&self, kind: Kind, name: &str, realm: &str) -> Result<Option<SyncedScript>> {
        let key = Self::realm_key(kind, realm);
        Ok(self
            .load_manifest()?
            .into_iter()
            .find(|e| e.reference.kind == kind && e.reference.name == name && e.realm == key))
    }
}

fn workspace_file(tenant: &str, realm: &str, r: &RemoteRef) -> PathBuf {
    workspace_file_in(&ProjectConfig::workspace_tree(tenant), realm, r)
}

fn workspace_file_in(workspace_tree: &Path, realm: &str, r: &RemoteRef) -> PathBuf {
    workspace_tree.join(r.kind.workspace_subpath(r, realm))
}

/// Read a local workspace file without collapsing permission / transient I/O
/// errors into "missing". Only a genuine `NotFound` means there is no local
/// copy.
fn read_local(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The only remote operations push/reconcile need. Keeping this seam here
/// lets tests drive the production decision logic without teaching [`Kind`]
/// about fake variants or bypassing the syntax/write gate in production.
trait SyncIo {
    async fn list(&self, kind: Kind, tenant: &str, realm: &str) -> Result<Vec<RemoteRef>>;

    async fn fetch(&self, kind: Kind, tenant: &str, realm: &str, id: &str) -> Result<RemoteScript>;

    async fn write_checked(
        &self,
        kind: Kind,
        tenant: &str,
        realm: &str,
        script: &RemoteScript,
        confirmed_prod: bool,
        gate: SyntaxGate,
    ) -> Result<Gated>;
}

struct LiveSyncIo;

impl SyncIo for LiveSyncIo {
    async fn list(&self, kind: Kind, tenant: &str, realm: &str) -> Result<Vec<RemoteRef>> {
        kind.list(tenant, realm).await
    }

    async fn fetch(&self, kind: Kind, tenant: &str, realm: &str, id: &str) -> Result<RemoteScript> {
        kind.fetch(tenant, realm, id).await
    }

    async fn write_checked(
        &self,
        kind: Kind,
        tenant: &str,
        realm: &str,
        script: &RemoteScript,
        confirmed_prod: bool,
        gate: SyntaxGate,
    ) -> Result<Gated> {
        write_checked(kind, tenant, realm, script, confirmed_prod, gate).await
    }
}

async fn confirm_write(
    io: &impl SyncIo,
    kind: Kind,
    tenant: &str,
    realm: &str,
    id: &str,
    submitted_source: &[u8],
) -> std::result::Result<RemoteScript, ConfirmationFailure> {
    let fetched = io
        .fetch(kind, tenant, realm, id)
        .await
        .map_err(|error| ConfirmationFailure::Failed(error.to_string()))?;
    let fetched_source = kind
        .decode_source(&fetched.raw_config)
        .map_err(|error| ConfirmationFailure::Failed(error.to_string()))?;
    if fetched_source != submitted_source {
        return Err(ConfirmationFailure::Mismatch);
    }
    Ok(fetched)
}

// ---------------------------------------------------------------------------
// Picker candidates
// ---------------------------------------------------------------------------

/// The local file's state relative to the last-synced snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalState {
    /// No local file (not pulled yet, or deleted). Shown as `-`.
    Missing,
    /// Local file matches the snapshot.
    Clean,
    /// Local file differs from the snapshot — un-synced changes on disk. `!`.
    Modified,
}

/// A script offered in the interactive pull/push picker.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub kind: Kind,
    pub realm: Option<String>,
    pub name: String,
    pub local: LocalState,
    /// Product-shipped default (AM `default:true`) — `push all` skips these.
    pub is_default: bool,
    /// AM script `context` (routes the workspace folder); `None` for IDM.
    /// Carried so callers can reconstruct the local file path.
    pub context: Option<String>,
    /// AM engine version — needed alongside `context` to resolve the
    /// decision-node folder (legacy vs next-gen). `None` for IDM.
    pub evaluator_version: Option<String>,
}

/// The local state of one (already-known) reference — snapshot vs the file on
/// disk. Cheap (no network).
fn local_state_for(
    store: &SnapshotStore,
    tenant: &str,
    r: &RemoteRef,
    realm: &str,
) -> Result<LocalState> {
    let Some(cfg) = store.load_config(r, realm)? else {
        return Ok(LocalState::Missing);
    };
    let snapshot = r.kind.decode_source(&cfg)?;
    let dest = workspace_file(tenant, realm, r);
    Ok(match read_local(&dest)? {
        Some(local) if local == snapshot => LocalState::Clean,
        Some(_) => LocalState::Modified,
        None => LocalState::Missing,
    })
}

/// Local state of a script by (kind, realm, name) — `Missing` if never synced.
/// Cheap (no network); used to decide whether a pull would clobber local edits.
pub fn local_state(tenant: &str, kind: Kind, realm: &str, name: &str) -> Result<LocalState> {
    let store = SnapshotStore::open(tenant);
    match store.lookup(kind, name, realm)? {
        Some(e) => local_state_for(&store, tenant, &e.reference, realm),
        None => Ok(LocalState::Missing),
    }
}

/// Candidates for `pull`: every remote script across all namespaces, tagged
/// with its local state. Lists the tenant (a few HTTP calls) + a local file
/// check per synced script; no per-script body fetch.
pub async fn pull_candidates(tenant: &str) -> Result<Vec<Candidate>> {
    use std::collections::HashMap;
    let store = SnapshotStore::open(tenant);
    let manifest = store.load_manifest()?;
    let by_key: HashMap<(Kind, Option<String>, String), &SyncedScript> = manifest
        .iter()
        .map(|e| {
            (
                (e.reference.kind, e.realm.clone(), e.reference.name.clone()),
                e,
            )
        })
        .collect();
    let mut out = Vec::new();
    for ns in super::Namespace::all() {
        for r in ns.kind.list(tenant, ns.realm_arg()).await? {
            let key = (ns.kind, ns.realm.clone(), r.name.clone());
            let local = match by_key.get(&key) {
                Some(e) => local_state_for(&store, tenant, &e.reference, ns.realm_arg())?,
                None => LocalState::Missing,
            };
            out.push(Candidate {
                kind: ns.kind,
                realm: ns.realm.clone(),
                is_default: r.is_default,
                context: r.context,
                evaluator_version: r.evaluator_version,
                name: r.name,
                local,
            });
        }
    }
    Ok(out)
}

/// Candidates for `push`: every synced script, tagged with its local state.
/// Purely local — no network.
pub fn push_candidates(tenant: &str) -> Result<Vec<Candidate>> {
    let store = SnapshotStore::open(tenant);
    let mut out = Vec::new();
    for e in store.load_manifest()? {
        let realm = e.realm.as_deref().unwrap_or_default();
        let local = local_state_for(&store, tenant, &e.reference, realm)?;
        out.push(Candidate {
            kind: e.reference.kind,
            realm: e.realm,
            is_default: e.reference.is_default,
            context: e.reference.context,
            evaluator_version: e.reference.evaluator_version,
            name: e.reference.name,
            local,
        });
    }
    Ok(out)
}

/// Candidates a batch push must inspect. A forced batch includes every tracked
/// entry because local-vs-snapshot cleanliness cannot prove the live tenant is
/// equal to either one.
pub fn push_batch_candidates(candidates: Vec<Candidate>, force: bool) -> Vec<Candidate> {
    candidates
        .into_iter()
        .filter(|candidate| force || candidate.local == LocalState::Modified)
        .collect()
}

/// The [`RemoteRef`] a candidate addresses locally. `id` is empty because
/// nothing local keys on it — the workspace path and the snapshot path are
/// both derived from kind/name/context.
fn ref_of(c: &Candidate) -> RemoteRef {
    RemoteRef {
        kind: c.kind,
        id: String::new(),
        name: c.name.clone(),
        context: c.context.clone(),
        is_default: c.is_default,
        evaluator_version: c.evaluator_version.clone(),
    }
}

/// A candidate's source for preview: the local workspace file if present
/// (what a push would send), else the last-synced snapshot, else `None`
/// (never pulled — nothing local to show). Cheap; no network.
pub fn preview_source(tenant: &str, c: &Candidate) -> Option<String> {
    let realm = c.realm.as_deref().unwrap_or_default();
    let r = ref_of(c);
    if let Ok(Some(bytes)) = read_local(&workspace_file(tenant, realm, &r)) {
        return Some(lossy(&bytes));
    }
    let store = SnapshotStore::open(tenant);
    if let Ok(Some(cfg)) = store.load_config(&r, realm) {
        if let Ok(bytes) = c.kind.decode_source(&cfg) {
            return Some(lossy(&bytes));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Pull
// ---------------------------------------------------------------------------

/// Fetch and classify every selected script before any workspace mutation.
pub async fn prepare_pull(tenant: &str, targets: Vec<PullTarget>) -> Result<PullPlan> {
    let store = SnapshotStore::open(tenant);
    let workspace_tree = ProjectConfig::workspace_tree(tenant);
    prepare_pull_with(&store, &workspace_tree, &LiveSyncIo, tenant, targets).await
}

async fn prepare_pull_with(
    store: &SnapshotStore,
    workspace_tree: &Path,
    io: &impl SyncIo,
    tenant: &str,
    targets: Vec<PullTarget>,
) -> Result<PullPlan> {
    let mut entries = Vec::new();
    for target in targets {
        let refs: Vec<_> = io
            .list(target.kind, tenant, &target.realm)
            .await?
            .into_iter()
            .filter(|reference| target.selector.matches(reference))
            .collect();
        if let Selector::Name(name) = &target.selector {
            if refs.is_empty() {
                return Err(Error::Config(format!(
                    "no {} script named {name:?}",
                    target.kind.as_str()
                )));
            }
        }
        for reference in refs {
            let script = io
                .fetch(target.kind, tenant, &target.realm, &reference.id)
                .await?;
            let remote_source = target.kind.decode_source(&script.raw_config)?;
            let local = read_local(&workspace_file_in(
                workspace_tree,
                &target.realm,
                &script.reference,
            ))?;
            let snapshot = store.pull_snapshot_source(&script.reference, &target.realm)?;
            let protected = local
                .as_ref()
                .is_some_and(|local| local != &remote_source && snapshot.as_ref() != Some(local));
            entries.push(PreparedPull {
                realm: target.realm.clone(),
                script,
                remote_source,
                local,
                protected,
            });
        }
    }
    Ok(PullPlan {
        store: SnapshotStore {
            dir: store.dir.clone(),
        },
        workspace_tree: workspace_tree.to_path_buf(),
        entries,
    })
}

/// Pull scripts of `kind` matching `selector` into the workspace, updating the
/// snapshot store. Backs up every differing local source unless `skip_backup`.
/// `realm` selects the AM realm (ignored for IDM).
pub async fn pull(
    tenant: &str,
    realm: &str,
    kind: Kind,
    selector: &Selector,
    skip_backup: bool,
) -> Result<Vec<PullOutcome>> {
    let store = SnapshotStore::open(tenant);
    let workspace_tree = ProjectConfig::workspace_tree(tenant);
    pull_with(
        &store,
        &workspace_tree,
        &LiveSyncIo,
        tenant,
        realm,
        kind,
        selector,
        skip_backup,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn pull_with(
    store: &SnapshotStore,
    workspace_tree: &Path,
    io: &impl SyncIo,
    tenant: &str,
    realm: &str,
    kind: Kind,
    selector: &Selector,
    skip_backup: bool,
) -> Result<Vec<PullOutcome>> {
    let refs: Vec<RemoteRef> = io
        .list(kind, tenant, realm)
        .await?
        .into_iter()
        .filter(|r| selector.matches(r))
        .collect();

    if let Selector::Name(n) = selector {
        if refs.is_empty() {
            return Err(Error::Config(format!(
                "no {} script named {n:?}",
                kind.as_str()
            )));
        }
    }

    let mut outcomes = Vec::new();
    for r in &refs {
        let script = io.fetch(kind, tenant, realm, &r.id).await?;
        let remote_src = kind.decode_source(&script.raw_config)?;
        let status = install_remote(
            store,
            workspace_tree,
            realm,
            &script,
            &remote_src,
            skip_backup,
        )?;

        outcomes.push(PullOutcome {
            name: script.reference.name.clone(),
            kind,
            realm: kind.realm_scoped().then(|| realm.to_string()),
            status,
        });
    }
    Ok(outcomes)
}

/// What [`create`] found when it went to make the script.
#[must_use = "a discarded create outcome reports a refusal as a successful create"]
#[derive(Debug, Clone)]
pub enum CreateOutcome {
    Created(RemoteScript),
    /// The tenant already had a script by that name. Carries its reference so
    /// the caller can [`adopt`] it rather than only report the refusal.
    NameTaken(RemoteRef),
    /// The gate refused the write; nothing was created.
    Refused(Refusal),
}

/// The outcome of a create that will not take an existing name — what
/// `aic script new` and `aic script copy` want. A taken name is an `Err`
/// there, so only two cases are left.
#[must_use = "a discarded create outcome reports a refusal as a successful create"]
#[derive(Debug)]
pub enum Creation {
    Created(RemoteScript),
    /// The gate refused the write; nothing was created.
    Refused(Refusal),
}

/// What to tell someone whose chosen name is already on the tenant. Not "use
/// `aic script push`": push needs a snapshot, so on an untracked name it fails
/// with `not synced yet` and leaves the caller exactly where they started.
///
/// The remedy names the **full ref**, not the bare name: a bare name resolves
/// its namespace from the current directory, so `aic script pull foo` run from
/// anywhere but a workspace subdir fails with `ambiguous "foo"` — advice that
/// works only where the caller already was.
pub fn name_taken_message(kind: Kind, realm: &str, name: &str) -> String {
    let reference = super::full_name(kind, kind.realm_scoped().then_some(realm), name);
    format!(
        "{name} already exists on the tenant — `aic script pull {reference}` to bring it under sync first"
    )
}

/// Create a standalone script, then pull the server's canonical representation
/// into the workspace and snapshot store. Returns [`CreateOutcome::NameTaken`]
/// rather than writing over a script that is already there.
pub async fn create(
    tenant: &str,
    realm: &str,
    script: &RemoteScript,
    confirmed_prod: bool,
    gate: SyntaxGate,
) -> Result<CreateOutcome> {
    let kind = script.reference.kind;
    if let Some(existing) = kind
        .list(tenant, realm)
        .await?
        .into_iter()
        .find(|existing| existing.name == script.reference.name)
    {
        return Ok(CreateOutcome::NameTaken(existing));
    }
    match write_checked(kind, tenant, realm, script, confirmed_prod, gate).await? {
        Gated::Written => {}
        Gated::Refused(refusal) => return Ok(CreateOutcome::Refused(refusal)),
    }
    // Pull it straight back so the workspace file, generated extras, and the
    // snapshot are exactly what a plain `pull` would have produced — the server
    // normalises fields we sent (AM rewrites `context`), so its copy is the
    // canonical one.
    pull(
        tenant,
        realm,
        kind,
        &Selector::Name(script.reference.name.clone()),
        false,
    )
    .await?;
    // Every kind honours the id we wrote to (AM: the URL uuid; IDM: the
    // name-derived config id — both verified), so re-read it directly rather
    // than listing the namespace again to rediscover it.
    Ok(CreateOutcome::Created(
        kind.fetch(tenant, realm, &script.reference.id).await?,
    ))
}

/// [`create`], refusing a name the tenant already has. What `aic script new` and
/// `aic script copy` want — both are asking for a script that does not exist.
/// A refusal is returned, **not** flattened into an `Error`: the wording of a
/// refusal — what the tenant said, that nothing was written, and the
/// `--force=syntax-check` remedy — belongs to the surface and is worded once
/// there. Converting it here is what left `create` and `copy` reporting a bare
/// `Error::Config` while every other verb explained itself.
pub async fn create_new(
    tenant: &str,
    realm: &str,
    script: &RemoteScript,
    confirmed_prod: bool,
    gate: SyntaxGate,
) -> Result<Creation> {
    match create(tenant, realm, script, confirmed_prod, gate).await? {
        CreateOutcome::Created(created) => Ok(Creation::Created(created)),
        CreateOutcome::Refused(refusal) => Ok(Creation::Refused(refusal)),
        CreateOutcome::NameTaken(existing) => Err(Error::Config(name_taken_message(
            existing.kind,
            realm,
            &existing.name,
        ))),
    }
}

/// Bring a script that is already on the tenant under sync **without writing to
/// the tenant and without touching the local file**: fetch it and snapshot it
/// as the baseline.
///
/// This is what an untracked name needs. `create` refuses it (rightly — it will
/// not silently overwrite), and `push` refuses it too (no snapshot), so a caller
/// that only has those two verbs is stuck repeating one refusal forever. After
/// adopting, the local file is an ordinary local edit against a known baseline
/// and the next push takes the conflict-aware path like any other script.
///
/// The tenant's own source is backed up when it differs from what is on disk,
/// because adopting makes the next push overwrite it.
/// Snapshot **this** copy, which must be the one the caller inspected. Taking a
/// fresh fetch here instead would reopen the window it just closed: the copy
/// that was compared and the copy that becomes the baseline have to be the same
/// bytes, or a write landing between them is adopted unseen and the next push
/// overwrites it without a conflict.
pub fn adopt(tenant: &str, realm: &str, remote: &RemoteScript) -> Result<PathBuf> {
    let store = SnapshotStore::open(tenant);
    let r = &remote.reference;
    let backup = back_up(&store, r, realm, &r.kind.decode_source(&remote.raw_config)?)?;
    store.record(remote, realm)?;
    Ok(backup)
}

/// The tenant's copy, and whether it already matches what is on disk.
///
/// The two cases are not the same decision. Matching content raises no ownership
/// question — adopting writes nothing to the tenant, the push that follows is
/// `Unchanged`, and an overwrite needs a later local edit like any tracked
/// script. **Differing** content means the local file never forked from that
/// snapshot, so the ordinary conflict check cannot help: manufacture a snapshot
/// from the remote and the next push has drift-free permission to overwrite work
/// this workspace has never seen. That one needs the operator.
pub async fn fetch_for_adoption(
    tenant: &str,
    realm: &str,
    r: &RemoteRef,
) -> Result<(RemoteScript, bool)> {
    let remote = r.kind.fetch(tenant, realm, &r.id).await?;
    let remote_src = r.kind.decode_source(&remote.raw_config)?;
    let matches =
        read_local(&workspace_file(tenant, realm, r))?.as_deref() == Some(remote_src.as_slice());
    Ok((remote, matches))
}

/// Rewrite only the identity fields required to copy a raw config verbatim.
///
/// `_id` **must** be rewritten, not just `name`: AM rejects a body whose `_id`
/// disagrees with the URL id (`400 "Script resource id and script JSON body id
/// do not match"`). Every other field rides along untouched, which is the point
/// of `copy` — `context`, `evaluatorVersion`, a schedule's cron and `globals`,
/// and any field this tool doesn't model all survive. Server-owned fields
/// (`_rev`, `createdBy`, `creationDate`, `lastModified*`) are left in place
/// because AIC ignores them on a write and stamps its own (verified 2026-07-30).
///
/// The one exception is `default`: a copy of a product-shipped script is not
/// itself a product default, so we send `false`. AM ignores the field on write
/// and computes it itself (verified 2026-07-31 — a client-sent `true` reads back
/// as `false` on both create routes and on update), so this is belt-and-braces
/// against a future AM that honours it: a script AM considered default would be
/// undeletable (403).
pub fn copy_body(raw: &Value, id: &str, name: &str) -> Result<Value> {
    let mut copied = raw.clone();
    let object = copied
        .as_object_mut()
        .ok_or_else(|| Error::Config("script config is not an object".into()))?;
    object.insert("_id".into(), Value::String(id.into()));
    object.insert("name".into(), Value::String(name.into()));
    if object.contains_key("default") {
        object.insert("default".into(), Value::Bool(false));
    }
    Ok(copied)
}

/// Copy a fetched script to a new identity, retaining every other raw field.
pub async fn copy(
    tenant: &str,
    realm: &str,
    source: &RemoteScript,
    destination_name: &str,
    confirmed_prod: bool,
    gate: SyntaxGate,
) -> Result<Creation> {
    let kind = source.reference.kind;
    let raw_config = copy_body(
        &source.raw_config,
        &kind.id_for_new(destination_name),
        destination_name,
    )?;
    let script = RemoteScript {
        reference: RemoteRef {
            kind,
            id: raw_config
                .get("_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            name: destination_name.to_string(),
            context: source.reference.context.clone(),
            is_default: false,
            evaluator_version: source.reference.evaluator_version.clone(),
        },
        raw_config,
    };
    create_new(tenant, realm, &script, confirmed_prod, gate).await
}

/// Delete a remote standalone script and remove only its local sync metadata.
pub async fn delete(
    tenant: &str,
    realm: &str,
    kind: Kind,
    reference: &RemoteRef,
    confirmed_prod: bool,
) -> Result<()> {
    kind.delete(tenant, realm, &reference.id, confirmed_prod)
        .await?;
    forget(tenant, realm, kind, &reference.name)
}

/// Replace the tracked source with a fetched remote copy. Any existing,
/// differing source is backed up first unless backup bypass was requested.
/// The snapshot advances only after the protected workspace operation.
fn install_remote(
    store: &SnapshotStore,
    workspace_tree: &Path,
    realm: &str,
    script: &RemoteScript,
    remote_source: &[u8],
    skip_backup: bool,
) -> Result<PullStatus> {
    let dest = workspace_file_in(workspace_tree, realm, &script.reference);
    let local = read_local(&dest)?;
    let differs = local.as_deref() != Some(remote_source);
    let status = match &local {
        Some(bytes) if differs && !skip_backup => {
            PullStatus::LocalBackedUp(back_up(store, &script.reference, realm, bytes)?)
        }
        None => PullStatus::Created,
        Some(_) if !differs => PullStatus::Unchanged,
        Some(_) => PullStatus::Updated,
    };

    if differs {
        write_workspace_source_in(workspace_tree, realm, script, remote_source)?;
    }
    // Supporting files are managed output, not user source. Every pull repairs
    // them even when the source itself is already current.
    write_workspace_supporting_files_in(workspace_tree, realm, script)?;
    store.record(script, realm)?;
    Ok(status)
}

fn write_workspace_source_in(
    workspace_tree: &Path,
    realm: &str,
    script: &RemoteScript,
    source: &[u8],
) -> Result<()> {
    let r = &script.reference;
    let dest = workspace_file_in(workspace_tree, realm, r);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, source)?;
    Ok(())
}

fn write_workspace_supporting_files_in(
    workspace_tree: &Path,
    realm: &str,
    script: &RemoteScript,
) -> Result<()> {
    let r = &script.reference;
    for (rel, contents) in r.kind.extra_files(r, realm) {
        let p = workspace_tree.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(p, contents)?;
    }
    Ok(())
}

fn back_up(store: &SnapshotStore, r: &RemoteRef, realm: &str, local: &[u8]) -> Result<PathBuf> {
    crate::backup::create_in(
        &store.backups_dir(),
        r.kind.as_str(),
        if r.kind.realm_scoped() {
            realm
        } else {
            "global"
        },
        &r.name,
        "cjs",
        local,
    )
}

#[cfg(test)]
fn back_up_at(
    store: &SnapshotStore,
    r: &RemoteRef,
    realm: &str,
    local: &[u8],
    stamp: &str,
) -> Result<PathBuf> {
    crate::backup::create_at(
        &store.backups_dir(),
        r.kind.as_str(),
        if r.kind.realm_scoped() {
            realm
        } else {
            "global"
        },
        &r.name,
        "cjs",
        local,
        stamp,
    )
}

// ---------------------------------------------------------------------------
// Push
// ---------------------------------------------------------------------------

/// Push a local edit back to the tenant. Requires a prior pull (snapshot must
/// exist). Content-based conflict check unless `force`. `realm` selects the AM
/// realm (ignored for IDM).
pub async fn push(
    tenant: &str,
    realm: &str,
    kind: Kind,
    name: &str,
    force: bool,
    confirmed_prod: bool,
    gate: SyntaxGate,
) -> Result<PushOutcome> {
    let store = SnapshotStore::open(tenant);
    let workspace_tree = ProjectConfig::workspace_tree(tenant);
    push_with(
        &store,
        &workspace_tree,
        &LiveSyncIo,
        tenant,
        realm,
        kind,
        name,
        force,
        confirmed_prod,
        gate,
    )
    .await
}

/// CLI push bridge: keep the parsed operation permission intact until the
/// call that selects the engine's forced-convergence path.
pub async fn push_authorized(
    tenant: &str,
    realm: &str,
    kind: Kind,
    name: &str,
    force: OperationAndSyntaxCheckForce,
    confirmed_prod: bool,
    gate: SyntaxGate,
) -> Result<PushOutcome> {
    let store = SnapshotStore::open(tenant);
    let workspace_tree = ProjectConfig::workspace_tree(tenant);
    push_authorized_with(
        &store,
        &workspace_tree,
        &LiveSyncIo,
        tenant,
        realm,
        kind,
        name,
        force,
        confirmed_prod,
        gate,
    )
    .await
}

/// Push a selected batch through the same per-entry engine as a single push.
/// Results stay per-entry so the CLI can continue past non-fatal failures.
pub async fn push_batch(
    tenant: &str,
    candidates: Vec<Candidate>,
    force: bool,
    confirmed_prod: bool,
    gate: SyntaxGate,
) -> Vec<(Candidate, Result<PushOutcome>)> {
    let store = SnapshotStore::open(tenant);
    let workspace_tree = ProjectConfig::workspace_tree(tenant);
    push_batch_with(
        &store,
        &workspace_tree,
        &LiveSyncIo,
        tenant,
        candidates,
        force,
        confirmed_prod,
        gate,
    )
    .await
}

/// Batch counterpart to [`push_authorized`].
pub async fn push_batch_authorized(
    tenant: &str,
    candidates: Vec<Candidate>,
    force: OperationAndSyntaxCheckForce,
    confirmed_prod: bool,
    gate: SyntaxGate,
) -> Vec<(Candidate, Result<PushOutcome>)> {
    let store = SnapshotStore::open(tenant);
    let workspace_tree = ProjectConfig::workspace_tree(tenant);
    push_batch_authorized_with(
        &store,
        &workspace_tree,
        &LiveSyncIo,
        tenant,
        candidates,
        force,
        confirmed_prod,
        gate,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn push_authorized_with(
    store: &SnapshotStore,
    workspace_tree: &Path,
    io: &impl SyncIo,
    tenant: &str,
    realm: &str,
    kind: Kind,
    name: &str,
    force: OperationAndSyntaxCheckForce,
    confirmed_prod: bool,
    gate: SyntaxGate,
) -> Result<PushOutcome> {
    push_with(
        store,
        workspace_tree,
        io,
        tenant,
        realm,
        kind,
        name,
        force.operation(),
        confirmed_prod,
        gate,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn push_batch_authorized_with(
    store: &SnapshotStore,
    workspace_tree: &Path,
    io: &impl SyncIo,
    tenant: &str,
    candidates: Vec<Candidate>,
    force: OperationAndSyntaxCheckForce,
    confirmed_prod: bool,
    gate: SyntaxGate,
) -> Vec<(Candidate, Result<PushOutcome>)> {
    push_batch_with(
        store,
        workspace_tree,
        io,
        tenant,
        candidates,
        force.operation(),
        confirmed_prod,
        gate,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn push_batch_with(
    store: &SnapshotStore,
    workspace_tree: &Path,
    io: &impl SyncIo,
    tenant: &str,
    candidates: Vec<Candidate>,
    force: bool,
    confirmed_prod: bool,
    gate: SyntaxGate,
) -> Vec<(Candidate, Result<PushOutcome>)> {
    let mut outcomes = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let realm = candidate.realm.as_deref().unwrap_or_default();
        let result = push_with(
            store,
            workspace_tree,
            io,
            tenant,
            realm,
            candidate.kind,
            &candidate.name,
            force,
            confirmed_prod,
            gate,
        )
        .await;
        outcomes.push((candidate, result));
    }
    outcomes
}

#[allow(clippy::too_many_arguments)]
async fn push_with(
    store: &SnapshotStore,
    workspace_tree: &Path,
    io: &impl SyncIo,
    tenant: &str,
    realm: &str,
    kind: Kind,
    name: &str,
    force: bool,
    confirmed_prod: bool,
    gate: SyntaxGate,
) -> Result<PushOutcome> {
    let entry = store.lookup(kind, name, realm)?.ok_or_else(|| {
        Error::Config(format!(
            "{name:?} not synced yet — `aic script pull {name}` first"
        ))
    })?;
    let r = &entry.reference;

    let snapshot_cfg = store
        .load_config(r, realm)?
        .ok_or_else(|| Error::Config(format!("snapshot for {name:?} missing — pull again")))?;
    let snapshot_src = kind.decode_source(&snapshot_cfg)?;

    let dest = workspace_file_in(workspace_tree, realm, r);
    let local_src = read_local(&dest)?.ok_or_else(|| {
        Error::Config(format!(
            "local file {} not found — pull first",
            dest.display()
        ))
    })?;

    // A normal push needs a local change. A forced push instead means "make
    // the live tenant match this file", including when a poisoned snapshot
    // happens to equal the local bytes.
    if local_src == snapshot_src && !force {
        return Ok(PushOutcome::Unchanged);
    }

    // Product-shipped defaults are editable too — no special guard. The only
    // thing that blocks a push is remote drift (handled below).

    // Conflict check: refetch remote, compare decoded bytes to the snapshot.
    let remote = io.fetch(kind, tenant, realm, &r.id).await?;
    let remote_src = kind.decode_source(&remote.raw_config)?;

    if remote_src == local_src {
        // Someone already pushed identical content; just refresh the snapshot.
        store.record(&remote, realm)?;
        return Ok(PushOutcome::AlreadyInSync);
    }

    let remote_drifted = remote_src != snapshot_src;
    if remote_drifted && !force {
        return Ok(PushOutcome::Conflict(ThreeWay {
            last_synced: lossy(&snapshot_src),
            remote: lossy(&remote_src),
            local: lossy(&local_src),
        }));
    }

    // Safe to push (remote matches snapshot) or forced. Start from the *current
    // remote* config and merge only our edited source, so concurrent metadata
    // changes (description/context/language/exports) aren't reverted by a
    // source-only push.
    let mut raw = remote.raw_config.clone();
    kind.encode_source(&mut raw, &local_src)?;
    let to_push = RemoteScript {
        reference: r.clone(),
        raw_config: raw,
    };
    match io
        .write_checked(kind, tenant, realm, &to_push, confirmed_prod, gate)
        .await?
    {
        Gated::Written => {}
        // Nothing was written and the snapshot is untouched, so the local edit
        // survives for the operator to fix and push again. The refusal is
        // stamped with the source it was about, taken from the bytes read at
        // the top of this function rather than re-read afterwards.
        Gated::Refused(refusal) => {
            return Ok(PushOutcome::Refused {
                refusal,
                source: source_id(&local_src),
            });
        }
    }

    let confirmed = match confirm_write(io, kind, tenant, realm, &r.id, &local_src).await {
        Ok(confirmed) => confirmed,
        Err(reason) => return Ok(PushOutcome::NotConfirmed(reason)),
    };
    // The fresh tenant copy is authoritative for server-normalised metadata.
    store.record(&confirmed, realm)?;
    Ok(PushOutcome::Pushed)
}

// ---------------------------------------------------------------------------
// Status / diff
// ---------------------------------------------------------------------------

/// Compute the state of every synced script for the tenant (optionally just one
/// kind). Realm comes from each manifest entry.
pub async fn status(tenant: &str, only: Option<Kind>) -> Result<Vec<StatusEntry>> {
    let store = SnapshotStore::open(tenant);
    let mut out = Vec::new();
    for entry in store.load_manifest()? {
        let r = &entry.reference;
        if let Some(k) = only {
            if r.kind != k {
                continue;
            }
        }
        let realm = entry.realm.as_deref().unwrap_or_default();
        let snapshot_cfg = match store.load_config(r, realm)? {
            Some(c) => c,
            None => continue,
        };
        let snapshot_src = r.kind.decode_source(&snapshot_cfg)?;

        let dest = workspace_file(tenant, realm, r);
        let Some(local_src) = read_local(&dest)? else {
            out.push(StatusEntry {
                name: r.name.clone(),
                kind: r.kind,
                realm: entry.realm.clone(),
                state: ScriptState::LocalMissing,
            });
            continue;
        };
        let local_modified = local_src != snapshot_src;

        let remote = r.kind.fetch(tenant, realm, &r.id).await?;
        let remote_src = r.kind.decode_source(&remote.raw_config)?;
        let remote_modified = remote_src != snapshot_src;

        let state = match (local_modified, remote_modified) {
            (false, false) => ScriptState::InSync,
            (true, false) => ScriptState::LocallyModified,
            (false, true) => ScriptState::RemotelyModified,
            (true, true) => ScriptState::BothModified,
        };
        out.push(StatusEntry {
            name: r.name.clone(),
            kind: r.kind,
            realm: entry.realm.clone(),
            state,
        });
    }
    Ok(out)
}

/// Which two script versions to load for `aic script diff`.
#[derive(Debug, Clone, Copy)]
pub enum DiffMode {
    /// Local edits only. Does not require a tenant request.
    LocalVsSnapshot,
    /// Tenant drift only. Does not read the local workspace file.
    SnapshotVsRemote,
    /// Current tenant content against the local workspace file.
    RemoteVsLocal,
}

#[derive(Debug, Clone)]
pub struct DiffPair {
    pub left: String,
    pub right: String,
}

/// Load exactly the two sides requested by the CLI. A missing local file is
/// represented as empty content so the rendered diff shows its deletion.
pub async fn diff(
    tenant: &str,
    realm: &str,
    kind: Kind,
    name: &str,
    mode: DiffMode,
) -> Result<DiffPair> {
    let store = SnapshotStore::open(tenant);
    let entry = store
        .lookup(kind, name, realm)?
        .ok_or_else(|| Error::Config(format!("{name:?} not synced yet")))?;
    let r = &entry.reference;
    let snapshot_cfg = store
        .load_config(r, realm)?
        .ok_or_else(|| Error::Config(format!("snapshot for {name:?} missing")))?;
    let snapshot_src = kind.decode_source(&snapshot_cfg)?;

    let load_local = || -> Result<Vec<u8>> {
        Ok(read_local(&workspace_file(tenant, realm, r))?.unwrap_or_default())
    };
    let load_remote = async {
        let remote = kind.fetch(tenant, realm, &r.id).await?;
        kind.decode_source(&remote.raw_config)
    };

    let (left, right) = match mode {
        DiffMode::LocalVsSnapshot => (snapshot_src, load_local()?),
        DiffMode::SnapshotVsRemote => (snapshot_src, load_remote.await?),
        DiffMode::RemoteVsLocal => (load_remote.await?, load_local()?),
    };
    Ok(DiffPair {
        left: lossy(&left),
        right: lossy(&right),
    })
}

// ---------------------------------------------------------------------------
// Sync (bidirectional reconcile)
// ---------------------------------------------------------------------------

/// Outcome of reconciling one synced script.
#[derive(Debug, Clone)]
#[must_use = "a discarded reconcile outcome reports a refusal as a successful push"]
pub enum ReconcileOutcome {
    InSync,
    Pushed,
    /// A write was accepted, but its read-back could not be confirmed. The
    /// local file and snapshot remain unchanged.
    NotConfirmed(ConfirmationFailure),
    Pulled(PullStatus),
    /// Both sides changed to the same content; snapshot refreshed.
    Converged,
    /// Both sides changed differently — the caller resolves.
    Conflict(ThreeWay),
    /// Only local changed, and the gate refused the write. Nothing was written
    /// and the snapshot is unchanged, so the next reconcile retries.
    Refused(Refusal),
}

/// Reconcile one synced script (one remote fetch): push if only local changed,
/// pull if only remote changed (or the local file is missing), refresh if both
/// converged to the same content, else return `Conflict` for the caller to
/// resolve. Pushing obeys the prod-write guard via `confirmed_prod`.
pub async fn reconcile(
    tenant: &str,
    realm: &str,
    kind: Kind,
    name: &str,
    confirmed_prod: bool,
    gate: SyntaxGate,
) -> Result<ReconcileOutcome> {
    let store = SnapshotStore::open(tenant);
    let workspace_tree = ProjectConfig::workspace_tree(tenant);
    reconcile_with(
        &store,
        &workspace_tree,
        &LiveSyncIo,
        tenant,
        realm,
        kind,
        name,
        confirmed_prod,
        gate,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn reconcile_with(
    store: &SnapshotStore,
    workspace_tree: &Path,
    io: &impl SyncIo,
    tenant: &str,
    realm: &str,
    kind: Kind,
    name: &str,
    confirmed_prod: bool,
    gate: SyntaxGate,
) -> Result<ReconcileOutcome> {
    let entry = store
        .lookup(kind, name, realm)?
        .ok_or_else(|| Error::Config(format!("{name:?} not synced")))?;
    let r = &entry.reference;
    let snap_cfg = store
        .load_config(r, realm)?
        .ok_or_else(|| Error::Config(format!("snapshot for {name:?} missing — pull again")))?;
    let snapshot = kind.decode_source(&snap_cfg)?;

    let remote_script = io.fetch(kind, tenant, realm, &r.id).await?;
    let remote = kind.decode_source(&remote_script.raw_config)?;
    let remote_changed = remote != snapshot;

    let dest = workspace_file_in(workspace_tree, realm, r);
    let local = match read_local(&dest)? {
        Some(bytes) => bytes,
        None => {
            // Local file gone — restore it from the remote.
            let status =
                install_remote(store, workspace_tree, realm, &remote_script, &remote, false)?;
            return Ok(ReconcileOutcome::Pulled(status));
        }
    };
    let local_changed = local != snapshot;

    match (local_changed, remote_changed) {
        (false, false) => Ok(ReconcileOutcome::InSync),
        (false, true) => {
            let status =
                install_remote(store, workspace_tree, realm, &remote_script, &remote, false)?;
            Ok(ReconcileOutcome::Pulled(status))
        }
        (true, false) => {
            // Remote == snapshot, so pushing the local edit is safe. Start from
            // the current remote config and merge only the edited source.
            let mut raw = remote_script.raw_config.clone();
            kind.encode_source(&mut raw, &local)?;
            let to_push = RemoteScript {
                reference: r.clone(),
                raw_config: raw,
            };
            match io
                .write_checked(kind, tenant, realm, &to_push, confirmed_prod, gate)
                .await?
            {
                Gated::Written => {}
                Gated::Refused(refusal) => return Ok(ReconcileOutcome::Refused(refusal)),
            }
            let confirmed = match confirm_write(io, kind, tenant, realm, &r.id, &local).await {
                Ok(confirmed) => confirmed,
                Err(reason) => return Ok(ReconcileOutcome::NotConfirmed(reason)),
            };
            store.record(&confirmed, realm)?;
            Ok(ReconcileOutcome::Pushed)
        }
        (true, true) if local == remote => {
            store.record(&remote_script, realm)?;
            Ok(ReconcileOutcome::Converged)
        }
        (true, true) => Ok(ReconcileOutcome::Conflict(ThreeWay {
            last_synced: lossy(&snapshot),
            remote: lossy(&remote),
            local: lossy(&local),
        })),
    }
}

/// An explicit `sync --resolve` direction. Unlike three-way reconcile, this
/// applies to every selected entry before any mutating operation is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Local,
    Remote,
}

/// Resolve one tracked entry in an explicit direction. Local means a confirmed
/// forced push; remote means a protected pull. A missing local source therefore
/// fails in local mode instead of being silently restored from the tenant.
pub async fn reconcile_resolved(
    tenant: &str,
    realm: &str,
    kind: Kind,
    name: &str,
    resolution: Resolution,
    confirmed_prod: bool,
    gate: SyntaxGate,
) -> Result<ReconcileOutcome> {
    let store = SnapshotStore::open(tenant);
    let workspace_tree = ProjectConfig::workspace_tree(tenant);
    reconcile_resolved_with(
        &store,
        &workspace_tree,
        &LiveSyncIo,
        tenant,
        realm,
        kind,
        name,
        resolution,
        confirmed_prod,
        gate,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn reconcile_resolved_with(
    store: &SnapshotStore,
    workspace_tree: &Path,
    io: &impl SyncIo,
    tenant: &str,
    realm: &str,
    kind: Kind,
    name: &str,
    resolution: Resolution,
    confirmed_prod: bool,
    gate: SyntaxGate,
) -> Result<ReconcileOutcome> {
    match resolution {
        Resolution::Local => {
            match push_with(
                store,
                workspace_tree,
                io,
                tenant,
                realm,
                kind,
                name,
                true,
                confirmed_prod,
                gate,
            )
            .await?
            {
                PushOutcome::Pushed => Ok(ReconcileOutcome::Pushed),
                PushOutcome::NotConfirmed(reason) => Ok(ReconcileOutcome::NotConfirmed(reason)),
                PushOutcome::Unchanged | PushOutcome::AlreadyInSync => Ok(ReconcileOutcome::InSync),
                PushOutcome::Conflict(conflict) => Ok(ReconcileOutcome::Conflict(conflict)),
                PushOutcome::Refused { refusal, .. } => Ok(ReconcileOutcome::Refused(refusal)),
            }
        }
        Resolution::Remote => {
            let entry = store
                .lookup(kind, name, realm)?
                .ok_or_else(|| Error::Config(format!("{name:?} not synced")))?;
            let remote_script = io.fetch(kind, tenant, realm, &entry.reference.id).await?;
            let remote = kind.decode_source(&remote_script.raw_config)?;
            let status =
                install_remote(store, workspace_tree, realm, &remote_script, &remote, false)?;
            if status == PullStatus::Unchanged {
                Ok(ReconcileOutcome::InSync)
            } else {
                Ok(ReconcileOutcome::Pulled(status))
            }
        }
    }
}

/// Remove a script's snapshot + manifest entry after a remote delete. Does not
/// touch the user's local `.cjs` (they may still want it).
pub fn forget(tenant: &str, realm: &str, kind: Kind, name: &str) -> Result<()> {
    let store = SnapshotStore::open(tenant);
    store.remove(kind, name, realm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    #[test]
    fn local_reads_only_treat_not_found_as_missing() {
        let dir = std::env::temp_dir().join(format!("aic-sync-read-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let file = dir.join("script.cjs");
        std::fs::write(&file, b"contents").unwrap();

        assert_eq!(read_local(&file).unwrap(), Some(b"contents".to_vec()));
        assert_eq!(read_local(&dir.join("missing.cjs")).unwrap(), None);
        assert!(read_local(&dir).is_err());

        std::fs::remove_dir_all(dir).unwrap();
    }

    // ----- snapshot-store manifest keying ---------------------------------
    // These exercise the engine's identity rule: every entry is keyed on
    // (kind, name, realm), where realm is Some only for realm-scoped kinds.
    // The store is built with an explicit temp `dir` so the tests don't depend
    // on the cwd-relative workspace root.

    use serde_json::json;

    #[test]
    fn copy_body_rewrites_only_identity_fields() {
        let raw = json!({
            "_id": "old-id", "name": "Old", "context": "LIBRARY",
            "evaluatorVersion": "2.0", "description": "keep", "unknown": {"x": 1}
        });
        let copied = copy_body(&raw, "new-id", "New").unwrap();
        assert_eq!(copied["_id"], "new-id");
        assert_eq!(copied["name"], "New");
        assert_eq!(copied["context"], "LIBRARY");
        assert_eq!(copied["evaluatorVersion"], "2.0");
        assert_eq!(copied["description"], "keep");
        assert_eq!(copied["unknown"], json!({"x": 1}));
    }

    #[test]
    fn copying_a_default_script_does_not_produce_another_default() {
        // An AM script AM considers default is undeletable (403), so a copy must
        // never inherit the flag.
        let copied = copy_body(&json!({"_id": "a", "name": "N", "default": true}), "b", "M");
        assert_eq!(copied.unwrap()["default"], false);
        // Kinds with no `default` field (IDM configs) gain nothing.
        let idm = copy_body(
            &json!({"_id": "endpoint/a", "source": "x"}),
            "endpoint/b",
            "b",
        );
        assert!(idm.unwrap().get("default").is_none());
    }

    fn store_at(dir: &Path) -> SnapshotStore {
        SnapshotStore {
            dir: dir.to_path_buf(),
        }
    }

    fn tmp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aic-sync-store-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn parsed_push_force(flags: &[&str]) -> OperationAndSyntaxCheckForce {
        let cli = crate::cli::Cli::try_parse_from(
            ["aic", "script", "push", "endpoint/confirm-me"]
                .into_iter()
                .chain(flags.iter().copied()),
        )
        .unwrap();
        let Some(crate::cli::Command::Script {
            command: crate::scripts::cli::ScriptCommand::Push { force, .. },
        }) = cli.command
        else {
            panic!("expected script push")
        };
        force
    }

    fn am_ref(name: &str) -> RemoteRef {
        RemoteRef {
            kind: Kind::Am,
            id: format!("uuid-{name}"),
            name: name.into(),
            context: None,
            is_default: false,
            evaluator_version: None,
        }
    }

    fn endpoint_ref(name: &str) -> RemoteRef {
        RemoteRef {
            kind: Kind::IdmEndpoint,
            id: format!("endpoint/{name}"),
            name: name.into(),
            context: None,
            is_default: false,
            evaluator_version: None,
        }
    }

    fn mapping_ref(name: &str) -> RemoteRef {
        RemoteRef {
            kind: Kind::IdmSyncMapping,
            id: format!("sync/{name}"),
            name: name.into(),
            context: None,
            is_default: false,
            evaluator_version: None,
        }
    }

    fn script(reference: RemoteRef, raw: Value) -> RemoteScript {
        RemoteScript {
            reference,
            raw_config: raw,
        }
    }

    struct FakeSyncIo {
        lists: Mutex<VecDeque<Result<Vec<RemoteRef>>>>,
        fetches: Mutex<VecDeque<Result<RemoteScript>>>,
        writes: Mutex<Vec<RemoteScript>>,
        refusal: Mutex<Option<Refusal>>,
    }

    impl FakeSyncIo {
        fn new(fetches: Vec<Result<RemoteScript>>) -> Self {
            Self {
                lists: Mutex::new(VecDeque::new()),
                fetches: Mutex::new(fetches.into()),
                writes: Mutex::new(Vec::new()),
                refusal: Mutex::new(None),
            }
        }

        fn refusing(fetches: Vec<Result<RemoteScript>>, refusal: Refusal) -> Self {
            Self {
                lists: Mutex::new(VecDeque::new()),
                fetches: Mutex::new(fetches.into()),
                writes: Mutex::new(Vec::new()),
                refusal: Mutex::new(Some(refusal)),
            }
        }

        fn for_pull(reference: RemoteRef, fetched: Result<RemoteScript>) -> Self {
            Self {
                lists: Mutex::new(vec![Ok(vec![reference])].into()),
                fetches: Mutex::new(vec![fetched].into()),
                writes: Mutex::new(Vec::new()),
                refusal: Mutex::new(None),
            }
        }

        fn write_count(&self) -> usize {
            self.writes.lock().unwrap().len()
        }
    }

    impl SyncIo for FakeSyncIo {
        async fn list(&self, _kind: Kind, _tenant: &str, _realm: &str) -> Result<Vec<RemoteRef>> {
            self.lists
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected list")
        }

        async fn fetch(
            &self,
            _kind: Kind,
            _tenant: &str,
            _realm: &str,
            _id: &str,
        ) -> Result<RemoteScript> {
            self.fetches
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected fetch")
        }

        async fn write_checked(
            &self,
            _kind: Kind,
            _tenant: &str,
            _realm: &str,
            script: &RemoteScript,
            _confirmed_prod: bool,
            _gate: SyntaxGate,
        ) -> Result<Gated> {
            if let Some(refusal) = self.refusal.lock().unwrap().take() {
                return Ok(Gated::Refused(refusal));
            }
            self.writes.lock().unwrap().push(script.clone());
            Ok(Gated::Written)
        }
    }

    fn endpoint_script(reference: &RemoteRef, source: &str, marker: &str) -> RemoteScript {
        script(
            reference.clone(),
            json!({
                "_id": reference.id,
                "type": "text/javascript",
                "source": source,
                "serverMarker": marker,
            }),
        )
    }

    fn mapping_script(reference: &RemoteRef, source: &str) -> RemoteScript {
        script(
            reference.clone(),
            json!({"_id": reference.id, "type": "text/javascript", "source": source}),
        )
    }

    fn push_fixture(snapshot: &str, local: &str) -> (PathBuf, SnapshotStore, PathBuf, RemoteRef) {
        let dir = tmp();
        let store = store_at(&dir.join(".aic-sync"));
        let workspace = dir.join("workspace");
        let reference = endpoint_ref("confirm-me");
        store
            .record(&endpoint_script(&reference, snapshot, "snapshot"), "alpha")
            .unwrap();
        let local_path = workspace_file_in(&workspace, "alpha", &reference);
        std::fs::create_dir_all(local_path.parent().unwrap()).unwrap();
        std::fs::write(&local_path, local).unwrap();
        (dir, store, workspace, reference)
    }

    fn snapshot_bytes(store: &SnapshotStore, reference: &RemoteRef) -> (Vec<u8>, Vec<u8>) {
        (
            std::fs::read(store.manifest_path()).unwrap(),
            std::fs::read(store.config_path(reference, "alpha")).unwrap(),
        )
    }

    async fn prepared_fixture(
        snapshot: Option<&str>,
        local: Option<&str>,
        remote: &str,
        malformed_snapshot: bool,
    ) -> (PathBuf, PullPlan) {
        let dir = tmp();
        let store = store_at(&dir.join(".aic-sync"));
        let workspace = dir.join("workspace");
        let reference = endpoint_ref("protected");
        if let Some(snapshot) = snapshot {
            store
                .record(&endpoint_script(&reference, snapshot, "snapshot"), "alpha")
                .unwrap();
        }
        if malformed_snapshot {
            let path = store.config_path(&reference, "alpha");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"not json").unwrap();
        }
        if let Some(local) = local {
            let path = workspace_file_in(&workspace, "alpha", &reference);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, local).unwrap();
        }
        let io = FakeSyncIo::for_pull(
            reference.clone(),
            Ok(endpoint_script(&reference, remote, "remote")),
        );
        let plan = prepare_pull_with(
            &store,
            &workspace,
            &io,
            "tenant",
            vec![PullTarget {
                realm: "alpha".into(),
                kind: Kind::IdmEndpoint,
                selector: Selector::Name(reference.name),
            }],
        )
        .await
        .unwrap();
        (dir, plan)
    }

    #[tokio::test]
    async fn protected_pull_preflight_follows_the_content_matrix() {
        for (snapshot, local, remote, malformed, protected) in [
            (None, None, "remote", false, false),
            (None, Some("remote"), "remote", false, false),
            (Some("old"), Some("old"), "remote", false, false),
            (Some("old"), Some("edited"), "remote", false, true),
            (None, Some("edited"), "remote", false, true),
            (None, Some("edited"), "remote", true, true),
        ] {
            let (dir, plan) = prepared_fixture(snapshot, local, remote, malformed).await;
            assert_eq!(!plan.protected_refs().is_empty(), protected);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn protected_pull_plan_backs_up_and_rechecks_before_any_install() {
        let (dir, plan) = prepared_fixture(Some("old"), Some("edited"), "remote", false).await;
        let outcomes = plan.install(false).unwrap();
        let PullStatus::LocalBackedUp(path) = &outcomes[0].status else {
            panic!("expected backup")
        };
        assert_eq!(std::fs::read(path).unwrap(), b"edited");
        std::fs::remove_dir_all(dir).unwrap();

        let (dir, plan) = prepared_fixture(Some("old"), Some("edited"), "remote", false).await;
        let entry = &plan.entries[0];
        let local_path =
            workspace_file_in(&plan.workspace_tree, &entry.realm, &entry.script.reference);
        std::fs::write(&local_path, b"newer edit").unwrap();
        assert!(plan.install(false).is_err());
        assert_eq!(std::fs::read(local_path).unwrap(), b"newer edit");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn bulk_pull_preflight_names_every_protected_script() {
        let dir = tmp();
        let store = store_at(&dir.join(".aic-sync"));
        let workspace = dir.join("workspace");
        let one = endpoint_ref("One");
        let two = endpoint_ref("Two");
        for reference in [&one, &two] {
            let path = workspace_file_in(&workspace, "alpha", reference);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"local edit").unwrap();
        }
        let io = FakeSyncIo {
            lists: Mutex::new(vec![Ok(vec![one.clone(), two.clone()])].into()),
            fetches: Mutex::new(
                vec![
                    Ok(endpoint_script(&one, "remote one", "remote")),
                    Ok(endpoint_script(&two, "remote two", "remote")),
                ]
                .into(),
            ),
            writes: Mutex::new(Vec::new()),
            refusal: Mutex::new(None),
        };
        let plan = prepare_pull_with(
            &store,
            &workspace,
            &io,
            "tenant",
            vec![PullTarget {
                realm: "alpha".into(),
                kind: Kind::IdmEndpoint,
                selector: Selector::All,
            }],
        )
        .await
        .unwrap();

        assert_eq!(plan.protected_refs(), ["endpoint/One", "endpoint/Two"]);
        assert_eq!(
            std::fs::read(workspace_file_in(&workspace, "alpha", &one)).unwrap(),
            b"local edit"
        );
        assert_eq!(
            std::fs::read(workspace_file_in(&workspace, "alpha", &two)).unwrap(),
            b"local edit"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn mapping_prefix_pull_preflights_the_whole_mapping_without_installing() {
        let dir = tmp();
        let store = store_at(&dir.join(".aic-sync"));
        let workspace = dir.join("workspace");
        let one = mapping_ref("map.onCreate");
        let two = mapping_ref("map.transform.name");
        let other = mapping_ref("other.onCreate");
        for reference in [&one, &two] {
            let path = workspace_file_in(&workspace, "", reference);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"local edit").unwrap();
        }
        let io = FakeSyncIo {
            lists: Mutex::new(vec![Ok(vec![one.clone(), other, two.clone()])].into()),
            fetches: Mutex::new(
                vec![
                    Ok(mapping_script(&one, "remote one")),
                    Ok(mapping_script(&two, "remote two")),
                ]
                .into(),
            ),
            writes: Mutex::new(Vec::new()),
            refusal: Mutex::new(None),
        };

        let plan = prepare_pull_with(
            &store,
            &workspace,
            &io,
            "tenant",
            vec![PullTarget {
                realm: String::new(),
                kind: Kind::IdmSyncMapping,
                selector: Selector::Prefix("map.".into()),
            }],
        )
        .await
        .unwrap();

        assert_eq!(
            plan.protected_refs(),
            ["sync/map.onCreate", "sync/map.transform.name"]
        );
        assert_eq!(
            std::fs::read(workspace_file_in(&workspace, "", &one)).unwrap(),
            b"local edit"
        );
        assert_eq!(
            std::fs::read(workspace_file_in(&workspace, "", &two)).unwrap(),
            b"local edit"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn push_does_not_advance_snapshot_when_accepted_write_reads_back_different() {
        let (dir, store, workspace, reference) = push_fixture("old", "local edit");
        let before = snapshot_bytes(&store, &reference);
        let io = FakeSyncIo::new(vec![
            Ok(endpoint_script(&reference, "old", "before-write")),
            Ok(endpoint_script(&reference, "still old", "after-write")),
        ]);

        let outcome = push_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            false,
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap();

        assert!(matches!(
            outcome,
            PushOutcome::NotConfirmed(ConfirmationFailure::Mismatch)
        ));
        assert_eq!(io.write_count(), 1);
        assert_eq!(snapshot_bytes(&store, &reference), before);
        assert_eq!(
            std::fs::read(workspace_file_in(&workspace, "alpha", &reference)).unwrap(),
            b"local edit"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn reconcile_does_not_advance_snapshot_when_accepted_write_reads_back_different() {
        let (dir, store, workspace, reference) = push_fixture("old", "local edit");
        let before = snapshot_bytes(&store, &reference);
        let io = FakeSyncIo::new(vec![
            Ok(endpoint_script(&reference, "old", "before-write")),
            Ok(endpoint_script(&reference, "still old", "after-write")),
        ]);

        let outcome = reconcile_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap();

        assert!(matches!(
            outcome,
            ReconcileOutcome::NotConfirmed(ConfirmationFailure::Mismatch)
        ));
        assert_eq!(io.write_count(), 1);
        assert_eq!(snapshot_bytes(&store, &reference), before);
        assert_eq!(
            std::fs::read(workspace_file_in(&workspace, "alpha", &reference)).unwrap(),
            b"local edit"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn confirmed_push_records_the_fetched_config_not_the_submitted_config() {
        let (dir, store, workspace, reference) = push_fixture("old", "local edit");
        let confirmed = endpoint_script(&reference, "local edit", "canonical-from-server");
        let io = FakeSyncIo::new(vec![
            Ok(endpoint_script(&reference, "old", "before-write")),
            Ok(confirmed.clone()),
        ]);

        let outcome = push_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            false,
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap();

        assert!(matches!(outcome, PushOutcome::Pushed));
        assert_eq!(
            store.load_config(&reference, "alpha").unwrap(),
            Some(confirmed.raw_config)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn confirmed_reconcile_records_the_fetched_config_not_the_submitted_config() {
        let (dir, store, workspace, reference) = push_fixture("old", "local edit");
        let confirmed = endpoint_script(&reference, "local edit", "canonical-from-server");
        let io = FakeSyncIo::new(vec![
            Ok(endpoint_script(&reference, "old", "before-write")),
            Ok(confirmed.clone()),
        ]);

        let outcome = reconcile_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap();

        assert!(matches!(outcome, ReconcileOutcome::Pushed));
        assert_eq!(
            store.load_config(&reference, "alpha").unwrap(),
            Some(confirmed.raw_config)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn push_confirmation_fetch_failure_leaves_snapshot_and_local_untouched() {
        let (dir, store, workspace, reference) = push_fixture("old", "local edit");
        let before = snapshot_bytes(&store, &reference);
        let io = FakeSyncIo::new(vec![
            Ok(endpoint_script(&reference, "old", "before-write")),
            Err(Error::Api {
                status: 503,
                body: "confirmation unavailable".into(),
            }),
        ]);

        let outcome = push_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            false,
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap();

        let PushOutcome::NotConfirmed(reason) = outcome else {
            panic!("expected failed confirmation")
        };
        assert!(matches!(reason, ConfirmationFailure::Failed(_)));
        assert!(reason.message().contains("confirmation failed"));
        assert_eq!(snapshot_bytes(&store, &reference), before);
        assert_eq!(
            std::fs::read(workspace_file_in(&workspace, "alpha", &reference)).unwrap(),
            b"local edit"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn reconcile_confirmation_decode_failure_leaves_snapshot_and_local_untouched() {
        let (dir, store, workspace, reference) = push_fixture("old", "local edit");
        let before = snapshot_bytes(&store, &reference);
        let io = FakeSyncIo::new(vec![
            Ok(endpoint_script(&reference, "old", "before-write")),
            Ok(script(reference.clone(), json!({"_id": reference.id}))),
        ]);

        let outcome = reconcile_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap();

        let ReconcileOutcome::NotConfirmed(reason) = outcome else {
            panic!("expected failed confirmation")
        };
        assert!(matches!(reason, ConfirmationFailure::Failed(_)));
        assert!(reason.message().contains("confirmation failed"));
        assert_eq!(snapshot_bytes(&store, &reference), before);
        assert_eq!(
            std::fs::read(workspace_file_in(&workspace, "alpha", &reference)).unwrap(),
            b"local edit"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn content_conflict_still_blocks_without_writing() {
        let (dir, store, workspace, reference) = push_fixture("old", "local edit");
        let before = snapshot_bytes(&store, &reference);
        let io = FakeSyncIo::new(vec![Ok(endpoint_script(
            &reference,
            "remote edit",
            "metadata",
        ))]);

        let outcome = push_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            false,
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap();

        assert!(matches!(outcome, PushOutcome::Conflict(_)));
        assert_eq!(io.write_count(), 0);
        assert_eq!(snapshot_bytes(&store, &reference), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn metadata_only_remote_drift_does_not_create_a_content_conflict() {
        let (dir, store, workspace, reference) = push_fixture("old", "local edit");
        let confirmed = endpoint_script(&reference, "local edit", "server-normalised");
        let io = FakeSyncIo::new(vec![
            Ok(endpoint_script(&reference, "old", "metadata-drifted")),
            Ok(confirmed),
        ]);

        let outcome = push_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            false,
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap();

        assert!(matches!(outcome, PushOutcome::Pushed));
        assert_eq!(io.write_count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn force_pushes_when_local_equals_a_poisoned_snapshot() {
        let (dir, store, workspace, reference) = push_fixture("local", "local");
        let io = FakeSyncIo::new(vec![
            Ok(endpoint_script(&reference, "remote", "before-write")),
            Ok(endpoint_script(&reference, "local", "confirmed")),
        ]);

        let outcome = push_authorized_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            parsed_push_force(&["--force"]),
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap();

        assert!(matches!(outcome, PushOutcome::Pushed));
        assert_eq!(io.write_count(), 1);
        let written = io.writes.lock().unwrap();
        assert_eq!(
            Kind::IdmEndpoint
                .decode_source(&written[0].raw_config)
                .unwrap(),
            b"local"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn forced_batch_pushes_and_confirms_a_clean_poisoned_snapshot_entry() {
        let (dir, store, workspace, reference) = push_fixture("local", "local");
        let io = FakeSyncIo::new(vec![
            Ok(endpoint_script(&reference, "remote", "before-write")),
            Ok(endpoint_script(&reference, "local", "confirmed")),
        ]);
        let force = parsed_push_force(&["--force"]);
        let selected = push_batch_candidates(
            vec![Candidate {
                kind: reference.kind,
                realm: None,
                name: reference.name.clone(),
                local: LocalState::Clean,
                is_default: false,
                context: None,
                evaluator_version: None,
            }],
            force.operation(),
        );

        let outcomes = push_batch_authorized_with(
            &store,
            &workspace,
            &io,
            "tenant",
            selected,
            force,
            false,
            SyntaxGate::Check,
        )
        .await;

        assert_eq!(outcomes.len(), 1);
        assert!(matches!(&outcomes[0].1, Ok(PushOutcome::Pushed)));
        assert_eq!(io.write_count(), 1);
        assert_eq!(
            Kind::IdmEndpoint
                .decode_source(&store.load_config(&reference, "alpha").unwrap().unwrap())
                .unwrap(),
            b"local"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn force_avoids_put_and_refreshes_snapshot_when_remote_matches_local() {
        let (dir, store, workspace, reference) = push_fixture("local", "local");
        let remote = endpoint_script(&reference, "local", "fresh-remote-metadata");
        let io = FakeSyncIo::new(vec![Ok(remote.clone())]);

        let outcome = push_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            true,
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap();

        assert!(matches!(outcome, PushOutcome::AlreadyInSync));
        assert_eq!(io.write_count(), 0);
        assert_eq!(
            store.load_config(&reference, "alpha").unwrap(),
            Some(remote.raw_config)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn force_still_honours_syntax_refusal_without_advancing_snapshot() {
        use crate::scripts::syntax::SyntaxError;

        let (dir, store, workspace, reference) = push_fixture("local", "local");
        let before = snapshot_bytes(&store, &reference);
        let refusal = Refusal::Rejected(vec![SyntaxError {
            line: Some(1),
            column: Some(2),
            message: "broken".into(),
        }]);
        let io = FakeSyncIo::refusing(
            vec![Ok(endpoint_script(&reference, "remote", "before-write"))],
            refusal,
        );

        let outcome = push_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            true,
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap();

        assert!(matches!(outcome, PushOutcome::Refused { .. }));
        assert_eq!(io.write_count(), 0);
        assert_eq!(snapshot_bytes(&store, &reference), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn ordinary_pull_backs_up_poisoned_snapshot_source_before_replacing_it() {
        let (dir, store, workspace, reference) = push_fixture("local", "local");
        let io = FakeSyncIo::for_pull(
            reference.clone(),
            Ok(endpoint_script(&reference, "remote", "fresh")),
        );

        let outcomes = pull_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &Selector::Name(reference.name.clone()),
            false,
        )
        .await
        .unwrap();

        let PullStatus::LocalBackedUp(path) = &outcomes[0].status else {
            panic!("expected protected replacement")
        };
        assert_eq!(std::fs::read(path).unwrap(), b"local");
        assert_eq!(
            std::fs::read(workspace_file_in(&workspace, "alpha", &reference)).unwrap(),
            b"remote"
        );
        assert_eq!(
            Kind::IdmEndpoint
                .decode_source(&store.load_config(&reference, "alpha").unwrap().unwrap())
                .unwrap(),
            b"remote"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn unchanged_pull_repairs_missing_and_stale_supporting_files() {
        let dir = tmp();
        let store = store_at(&dir.join(".aic-sync"));
        let workspace = dir.join("workspace");
        let mut reference = am_ref("SupportFiles");
        reference.context = Some("LIBRARY".into());
        reference.evaluator_version = Some("2.0".into());
        let mut raw = json!({
            "_id": reference.id,
            "name": reference.name,
            "context": "LIBRARY",
            "evaluatorVersion": "2.0",
            "language": "JAVASCRIPT"
        });
        Kind::Am.encode_source(&mut raw, b"same source").unwrap();
        let remote = script(reference.clone(), raw);
        store.record(&remote, "alpha").unwrap();

        let source_path = workspace_file_in(&workspace, "alpha", &reference);
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, b"same source").unwrap();
        let supporting = Kind::Am.extra_files(&reference, "alpha");
        assert_eq!(supporting.len(), 2);
        let stale_path = workspace.join(&supporting[0].0);
        std::fs::write(&stale_path, b"stale").unwrap();
        let missing_path = workspace.join(&supporting[1].0);
        assert!(!missing_path.exists());

        let io = FakeSyncIo::for_pull(reference.clone(), Ok(remote));
        let outcomes = pull_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::Am,
            &Selector::Name(reference.name.clone()),
            false,
        )
        .await
        .unwrap();

        assert_eq!(outcomes[0].status, PullStatus::Unchanged);
        assert_eq!(std::fs::read(&source_path).unwrap(), b"same source");
        assert_eq!(
            std::fs::read_to_string(stale_path).unwrap(),
            supporting[0].1
        );
        assert_eq!(
            std::fs::read_to_string(missing_path).unwrap(),
            supporting[1].1
        );
        assert!(!store.backups_dir().exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn direct_pull_backup_bypass_remains_explicit() {
        let (dir, store, workspace, reference) = push_fixture("local", "local");
        let io = FakeSyncIo::for_pull(
            reference.clone(),
            Ok(endpoint_script(&reference, "remote", "fresh")),
        );

        let outcomes = pull_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &Selector::Name(reference.name.clone()),
            true,
        )
        .await
        .unwrap();

        assert_eq!(outcomes[0].status, PullStatus::Updated);
        assert!(!store.backups_dir().exists());
        assert_eq!(
            std::fs::read(workspace_file_in(&workspace, "alpha", &reference)).unwrap(),
            b"remote"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn ordinary_reconcile_backs_up_poisoned_snapshot_source_before_pull() {
        let (dir, store, workspace, reference) = push_fixture("local", "local");
        let io = FakeSyncIo::new(vec![Ok(endpoint_script(&reference, "remote", "fresh"))]);

        let outcome = reconcile_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap();

        let ReconcileOutcome::Pulled(PullStatus::LocalBackedUp(path)) = outcome else {
            panic!("expected protected reconcile pull")
        };
        assert_eq!(std::fs::read(path).unwrap(), b"local");
        assert_eq!(
            std::fs::read(workspace_file_in(&workspace, "alpha", &reference)).unwrap(),
            b"remote"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn resolve_local_pushes_a_poisoned_snapshot_and_never_replaces_local() {
        let (dir, store, workspace, reference) = push_fixture("local", "local");
        let io = FakeSyncIo::new(vec![
            Ok(endpoint_script(&reference, "remote", "before")),
            Ok(endpoint_script(&reference, "local", "confirmed")),
        ]);

        let outcome = reconcile_resolved_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            Resolution::Local,
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap();

        assert!(matches!(outcome, ReconcileOutcome::Pushed));
        assert_eq!(io.write_count(), 1);
        assert_eq!(
            std::fs::read(workspace_file_in(&workspace, "alpha", &reference)).unwrap(),
            b"local"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn resolve_remote_backs_up_a_poisoned_snapshot_before_replacing_local() {
        let (dir, store, workspace, reference) = push_fixture("local", "local");
        let io = FakeSyncIo::new(vec![Ok(endpoint_script(&reference, "remote", "fresh"))]);

        let outcome = reconcile_resolved_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            Resolution::Remote,
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap();

        let ReconcileOutcome::Pulled(PullStatus::LocalBackedUp(path)) = outcome else {
            panic!("expected protected remote resolution")
        };
        assert_eq!(std::fs::read(path).unwrap(), b"local");
        assert_eq!(
            std::fs::read(workspace_file_in(&workspace, "alpha", &reference)).unwrap(),
            b"remote"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn missing_local_direction_matrix_restores_except_when_local_must_win() {
        // Ordinary reconcile restores the missing source.
        let (dir, store, workspace, reference) = push_fixture("old", "discard");
        std::fs::remove_file(workspace_file_in(&workspace, "alpha", &reference)).unwrap();
        let io = FakeSyncIo::new(vec![Ok(endpoint_script(&reference, "remote", "fresh"))]);
        let ordinary = reconcile_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap();
        assert!(matches!(
            ordinary,
            ReconcileOutcome::Pulled(PullStatus::Created)
        ));
        std::fs::remove_dir_all(dir).unwrap();

        // Explicit local refuses to invent a local source or pull remote.
        let (dir, store, workspace, reference) = push_fixture("old", "discard");
        let local_path = workspace_file_in(&workspace, "alpha", &reference);
        std::fs::remove_file(&local_path).unwrap();
        let io = FakeSyncIo::new(Vec::new());
        let error = reconcile_resolved_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            Resolution::Local,
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("local file"));
        assert!(!local_path.exists());
        assert_eq!(io.write_count(), 0);
        std::fs::remove_dir_all(dir).unwrap();

        // Explicit remote restores it.
        let (dir, store, workspace, reference) = push_fixture("old", "discard");
        let local_path = workspace_file_in(&workspace, "alpha", &reference);
        std::fs::remove_file(&local_path).unwrap();
        let io = FakeSyncIo::new(vec![Ok(endpoint_script(&reference, "remote", "fresh"))]);
        let remote = reconcile_resolved_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            Resolution::Remote,
            false,
            SyntaxGate::Check,
        )
        .await
        .unwrap();
        assert!(matches!(
            remote,
            ReconcileOutcome::Pulled(PullStatus::Created)
        ));
        assert_eq!(std::fs::read(local_path).unwrap(), b"remote");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn equal_source_resolution_refreshes_baseline_without_write_or_replacement() {
        for resolution in [Resolution::Local, Resolution::Remote] {
            let (dir, store, workspace, reference) = push_fixture("old snapshot", "same");
            let local_path = workspace_file_in(&workspace, "alpha", &reference);
            let before_modified = std::fs::metadata(&local_path).unwrap().modified().unwrap();
            let remote = endpoint_script(&reference, "same", "fresh-metadata");
            let io = FakeSyncIo::new(vec![Ok(remote.clone())]);

            let outcome = reconcile_resolved_with(
                &store,
                &workspace,
                &io,
                "tenant",
                "alpha",
                Kind::IdmEndpoint,
                &reference.name,
                resolution,
                false,
                SyntaxGate::Check,
            )
            .await
            .unwrap();

            assert!(matches!(outcome, ReconcileOutcome::InSync));
            assert_eq!(io.write_count(), 0);
            assert_eq!(std::fs::read(&local_path).unwrap(), b"same");
            assert_eq!(
                std::fs::metadata(&local_path).unwrap().modified().unwrap(),
                before_modified
            );
            assert_eq!(
                store.load_config(&reference, "alpha").unwrap(),
                Some(remote.raw_config)
            );
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn missing_remote_is_an_error_in_every_reconcile_mode() {
        for resolution in [None, Some(Resolution::Local), Some(Resolution::Remote)] {
            let (dir, store, workspace, reference) = push_fixture("old", "local edit");
            let io = FakeSyncIo::new(vec![Err(Error::Api {
                status: 404,
                body: "missing".into(),
            })]);
            let result = match resolution {
                Some(direction) => {
                    reconcile_resolved_with(
                        &store,
                        &workspace,
                        &io,
                        "tenant",
                        "alpha",
                        Kind::IdmEndpoint,
                        &reference.name,
                        direction,
                        false,
                        SyntaxGate::Check,
                    )
                    .await
                }
                None => {
                    reconcile_with(
                        &store,
                        &workspace,
                        &io,
                        "tenant",
                        "alpha",
                        Kind::IdmEndpoint,
                        &reference.name,
                        false,
                        SyntaxGate::Check,
                    )
                    .await
                }
            };
            assert!(result.is_err());
            assert_eq!(io.write_count(), 0);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn backup_failure_prevents_local_replacement_and_snapshot_advance() {
        let (dir, store, workspace, reference) = push_fixture("local", "local");
        let before = snapshot_bytes(&store, &reference);
        std::fs::write(store.backups_dir(), b"not a directory").unwrap();
        let io = FakeSyncIo::new(vec![Ok(endpoint_script(&reference, "remote", "fresh"))]);

        let result = reconcile_with(
            &store,
            &workspace,
            &io,
            "tenant",
            "alpha",
            Kind::IdmEndpoint,
            &reference.name,
            false,
            SyntaxGate::Check,
        )
        .await;

        assert!(result.is_err());
        assert_eq!(
            std::fs::read(workspace_file_in(&workspace, "alpha", &reference)).unwrap(),
            b"local"
        );
        assert_eq!(snapshot_bytes(&store, &reference), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn same_second_backups_are_exclusive_and_keep_kind_realm_name_identity() {
        let dir = tmp();
        let store = store_at(&dir.join(".aic-sync"));
        let reference = am_ref("Same/Name");
        let stamp = "20260910T165833Z";
        let first = back_up_at(&store, &reference, "alpha", b"first", stamp).unwrap();
        let second = back_up_at(&store, &reference, "alpha", b"second", stamp).unwrap();

        assert_ne!(first, second);
        assert_eq!(std::fs::read(&first).unwrap(), b"first");
        assert_eq!(std::fs::read(&second).unwrap(), b"second");
        for path in [first, second] {
            let name = path.file_name().unwrap().to_string_lossy();
            assert!(name.starts_with("am.alpha.Same_Name."), "{name}");
            assert!(name.contains(stamp), "{name}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn same_named_backups_in_other_realms_and_kinds_have_distinct_identities() {
        let dir = tmp();
        let store = store_at(&dir.join(".aic-sync"));
        let am = am_ref("Shared");
        let endpoint = endpoint_ref("Shared");
        let alpha = back_up(&store, &am, "alpha", b"alpha").unwrap();
        let bravo = back_up(&store, &am, "bravo", b"bravo").unwrap();
        let idm = back_up(&store, &endpoint, "ignored", b"idm").unwrap();

        assert!(
            alpha
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("am.alpha.Shared.")
        );
        assert!(
            bravo
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("am.bravo.Shared.")
        );
        assert!(
            idm.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("idm.global.Shared.")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn realm_key_is_set_only_for_realm_scoped_kinds() {
        assert_eq!(
            SnapshotStore::realm_key(Kind::Am, "alpha"),
            Some("alpha".into())
        );
        assert_eq!(
            SnapshotStore::realm_key(Kind::Am, "bravo"),
            Some("bravo".into())
        );
        assert_eq!(SnapshotStore::realm_key(Kind::IdmEndpoint, "alpha"), None);
        assert_eq!(SnapshotStore::realm_key(Kind::IdmSchedule, "bravo"), None);
    }

    #[test]
    fn am_same_name_in_two_realms_stays_distinct() {
        let dir = tmp();
        let store = store_at(&dir);
        store
            .record(
                &script(am_ref("Shared"), json!({"script": "YQ=="})),
                "alpha",
            )
            .unwrap();
        store
            .record(
                &script(am_ref("Shared"), json!({"script": "Yg=="})),
                "bravo",
            )
            .unwrap();

        // Two manifest entries, one per realm — not one clobbering the other.
        assert_eq!(store.load_manifest().unwrap().len(), 2);
        assert_eq!(
            store
                .lookup(Kind::Am, "Shared", "alpha")
                .unwrap()
                .unwrap()
                .realm,
            Some("alpha".into())
        );
        assert_eq!(
            store
                .lookup(Kind::Am, "Shared", "bravo")
                .unwrap()
                .unwrap()
                .realm,
            Some("bravo".into())
        );
        // Configs are namespaced per realm too — no cross-realm clobber.
        assert_eq!(
            store.load_config(&am_ref("Shared"), "alpha").unwrap(),
            Some(json!({"script": "YQ=="}))
        );
        assert_eq!(
            store.load_config(&am_ref("Shared"), "bravo").unwrap(),
            Some(json!({"script": "Yg=="}))
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn idm_endpoint_normalizes_realm_to_none() {
        let dir = tmp();
        let store = store_at(&dir);
        store
            .record(
                &script(endpoint_ref("myEp"), json!({"source": "x"})),
                "alpha",
            )
            .unwrap();
        // "Re-syncing" the same endpoint under a different realm arg must not
        // create a second entry — IDM is tenant-global (realm key is None).
        store
            .record(
                &script(endpoint_ref("myEp"), json!({"source": "y"})),
                "bravo",
            )
            .unwrap();

        assert_eq!(store.load_manifest().unwrap().len(), 1);
        let found = store
            .lookup(Kind::IdmEndpoint, "myEp", "alpha")
            .unwrap()
            .unwrap();
        assert_eq!(found.realm, None);
        // Lookup resolves regardless of the realm argument passed.
        assert!(
            store
                .lookup(Kind::IdmEndpoint, "myEp", "bravo")
                .unwrap()
                .is_some()
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn re_recording_same_key_replaces_in_place() {
        let dir = tmp();
        let store = store_at(&dir);
        store
            .record(&script(am_ref("A"), json!({"script": "YQ=="})), "alpha")
            .unwrap();
        store
            .record(&script(am_ref("A"), json!({"script": "Yg=="})), "alpha")
            .unwrap();

        assert_eq!(store.load_manifest().unwrap().len(), 1);
        assert_eq!(
            store.load_config(&am_ref("A"), "alpha").unwrap(),
            Some(json!({"script": "Yg=="}))
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn remove_drops_only_the_targeted_realm() {
        let dir = tmp();
        let store = store_at(&dir);
        store
            .record(
                &script(am_ref("Shared"), json!({"script": "YQ=="})),
                "alpha",
            )
            .unwrap();
        store
            .record(
                &script(am_ref("Shared"), json!({"script": "Yg=="})),
                "bravo",
            )
            .unwrap();

        store.remove(Kind::Am, "Shared", "alpha").unwrap();

        assert!(store.lookup(Kind::Am, "Shared", "alpha").unwrap().is_none());
        assert!(store.lookup(Kind::Am, "Shared", "bravo").unwrap().is_some());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn selector_matches_all_or_exact_name() {
        let r = am_ref("Widget");
        assert!(Selector::All.matches(&r));
        assert!(Selector::Name("Widget".into()).matches(&r));
        assert!(!Selector::Name("Other".into()).matches(&r));
    }
}
