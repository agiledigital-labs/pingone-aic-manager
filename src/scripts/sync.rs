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

use super::{Kind, RemoteRef, RemoteScript};
use crate::config::ProjectConfig;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Which scripts an operation targets.
#[derive(Debug, Clone)]
pub enum Selector {
    All,
    Name(String),
}

impl Selector {
    fn matches(&self, r: &RemoteRef) -> bool {
        match self {
            Selector::All => true,
            Selector::Name(n) => r.name == *n,
        }
    }
}

/// Per-script outcome of a pull.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullStatus {
    Created,
    Updated,
    Unchanged,
    /// Local `.cjs` had un-pushed edits and remote also moved; we backed the
    /// local copy up to the given path before overwriting.
    LocalBackedUp(PathBuf),
}

#[derive(Debug, Clone)]
pub struct PullOutcome {
    pub name: String,
    pub kind: Kind,
    pub status: PullStatus,
}

/// Per-script outcome of a push.
#[derive(Debug, Clone)]
pub enum PushOutcome {
    Pushed,
    /// Local matches the last-synced snapshot — nothing to push.
    Unchanged,
    /// Remote already equals local — snapshot refreshed, no write needed.
    AlreadyInSync,
    /// Remote drifted from the snapshot and doesn't match local. Blocked
    /// unless `--force`. Carries the 3-way texts for display.
    Conflict(ThreeWay),
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
    ProjectConfig::workspace_tree(tenant).join(r.kind.workspace_subpath(r, realm))
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

/// A candidate's source for preview: the local workspace file if present
/// (what a push would send), else the last-synced snapshot, else `None`
/// (never pulled — nothing local to show). Cheap; no network.
pub fn preview_source(tenant: &str, c: &Candidate) -> Option<String> {
    let realm = c.realm.as_deref().unwrap_or_default();
    let r = RemoteRef {
        kind: c.kind,
        id: String::new(),
        name: c.name.clone(),
        context: c.context.clone(),
        is_default: c.is_default,
        evaluator_version: c.evaluator_version.clone(),
    };
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

/// Pull scripts of `kind` matching `selector` into the workspace, updating the
/// snapshot store. Protects un-pushed local edits with a backup unless `force`.
/// `realm` selects the AM realm (ignored for IDM).
pub async fn pull(
    tenant: &str,
    realm: &str,
    kind: Kind,
    selector: &Selector,
    force: bool,
) -> Result<Vec<PullOutcome>> {
    let store = SnapshotStore::open(tenant);
    let refs: Vec<RemoteRef> = kind
        .list(tenant, realm)
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
        let script = kind.fetch(tenant, realm, &r.id).await?;
        let remote_src = kind.decode_source(&script.raw_config)?;

        let dest = workspace_file(tenant, realm, &script.reference);
        let snapshot_src = match store.load_config(&script.reference, realm)? {
            Some(cfg) => Some(kind.decode_source(&cfg)?),
            None => None,
        };
        let local_src = read_local(&dest)?;

        // Local content we'd lose by overwriting: either edited since the last
        // pull, or an untracked file that was here before we ever synced (no
        // snapshot). Both are the user's work — back it up unless --force.
        let local_has_unsynced = match (&local_src, &snapshot_src) {
            (Some(l), Some(s)) => l != s,
            (Some(_), None) => true,
            _ => false,
        };
        let differs_from_remote = local_src.as_deref() != Some(remote_src.as_slice());

        let status = if local_has_unsynced && differs_from_remote && !force {
            let backup = back_up(&store, &script.reference, local_src.as_deref().unwrap())?;
            PullStatus::LocalBackedUp(backup)
        } else if local_src.is_none() {
            PullStatus::Created
        } else if !differs_from_remote {
            PullStatus::Unchanged
        } else {
            PullStatus::Updated
        };

        // Write source (+ any extra generated files), then refresh snapshot.
        write_workspace_files(tenant, realm, &script, &remote_src)?;
        store.record(&script, realm)?;

        outcomes.push(PullOutcome {
            name: script.reference.name.clone(),
            kind,
            status,
        });
    }
    Ok(outcomes)
}

/// Create a standalone script after refusing an existing name, then pull the
/// server's canonical representation into the workspace and snapshot store.
pub async fn create(
    tenant: &str,
    realm: &str,
    script: &RemoteScript,
    confirmed_prod: bool,
) -> Result<RemoteScript> {
    let kind = script.reference.kind;
    if kind
        .list(tenant, realm)
        .await?
        .iter()
        .any(|existing| existing.name == script.reference.name)
    {
        return Err(Error::Config(format!(
            "{} already exists; use `aic script push` to overwrite it",
            script.reference.name
        )));
    }
    kind.write(tenant, realm, script, confirmed_prod).await?;
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
    kind.fetch(tenant, realm, &script.reference.id).await
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
) -> Result<RemoteScript> {
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
    create(tenant, realm, &script, confirmed_prod).await
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

fn write_workspace_files(
    tenant: &str,
    realm: &str,
    script: &RemoteScript,
    source: &[u8],
) -> Result<()> {
    let tree = ProjectConfig::workspace_tree(tenant);
    let r = &script.reference;
    let dest = tree.join(r.kind.workspace_subpath(r, realm));
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, source)?;
    for (rel, contents) in r.kind.extra_files(r, realm) {
        let p = tree.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(p, contents)?;
    }
    Ok(())
}

fn back_up(store: &SnapshotStore, r: &RemoteRef, local: &[u8]) -> Result<PathBuf> {
    let dir = store.backups_dir();
    std::fs::create_dir_all(&dir)?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let path = dir.join(format!("{}.{stamp}.cjs", r.name));
    std::fs::write(&path, local)?;
    Ok(path)
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
) -> Result<PushOutcome> {
    let store = SnapshotStore::open(tenant);
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

    let dest = workspace_file(tenant, realm, r);
    let local_src = read_local(&dest)?.ok_or_else(|| {
        Error::Config(format!(
            "local file {} not found — pull first",
            dest.display()
        ))
    })?;

    // No local change vs the snapshot → nothing to do.
    if local_src == snapshot_src {
        return Ok(PushOutcome::Unchanged);
    }

    // Product-shipped defaults are editable too — no special guard. The only
    // thing that blocks a push is remote drift (handled below).

    // Conflict check: refetch remote, compare decoded bytes to the snapshot.
    let remote = kind.fetch(tenant, realm, &r.id).await?;
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
    kind.write(tenant, realm, &to_push, confirmed_prod).await?;

    // Refresh the snapshot to exactly what we just pushed.
    store.record(&to_push, realm)?;
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
pub enum ReconcileOutcome {
    InSync,
    Pushed,
    Pulled,
    /// Both sides changed to the same content; snapshot refreshed.
    Converged,
    /// Both sides changed differently — the caller resolves.
    Conflict(ThreeWay),
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
) -> Result<ReconcileOutcome> {
    let store = SnapshotStore::open(tenant);
    let entry = store
        .lookup(kind, name, realm)?
        .ok_or_else(|| Error::Config(format!("{name:?} not synced")))?;
    let r = &entry.reference;
    let snap_cfg = store
        .load_config(r, realm)?
        .ok_or_else(|| Error::Config(format!("snapshot for {name:?} missing — pull again")))?;
    let snapshot = kind.decode_source(&snap_cfg)?;

    let remote_script = kind.fetch(tenant, realm, &r.id).await?;
    let remote = kind.decode_source(&remote_script.raw_config)?;
    let remote_changed = remote != snapshot;

    let dest = workspace_file(tenant, realm, r);
    let local = match read_local(&dest)? {
        Some(bytes) => bytes,
        None => {
            // Local file gone — restore it from the remote.
            write_workspace_files(tenant, realm, &remote_script, &remote)?;
            store.record(&remote_script, realm)?;
            return Ok(ReconcileOutcome::Pulled);
        }
    };
    let local_changed = local != snapshot;

    match (local_changed, remote_changed) {
        (false, false) => Ok(ReconcileOutcome::InSync),
        (false, true) => {
            write_workspace_files(tenant, realm, &remote_script, &remote)?;
            store.record(&remote_script, realm)?;
            Ok(ReconcileOutcome::Pulled)
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
            kind.write(tenant, realm, &to_push, confirmed_prod).await?;
            store.record(&to_push, realm)?;
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

/// Remove a script's snapshot + manifest entry after a remote delete. Does not
/// touch the user's local `.cjs` (they may still want it).
pub fn forget(tenant: &str, realm: &str, kind: Kind, name: &str) -> Result<()> {
    let store = SnapshotStore::open(tenant);
    store.remove(kind, name, realm)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn script(reference: RemoteRef, raw: Value) -> RemoteScript {
        RemoteScript {
            reference,
            raw_config: raw,
        }
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
