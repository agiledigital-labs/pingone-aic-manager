//! Kind-agnostic sync engine: snapshot store, pull, push, status, diff.
//!
//! This module never matches on [`Kind`] — every per-kind concern is reached
//! through the `Kind` methods. Conflict detection is **content-based** (scripts
//! and IDM endpoints both lack `_rev`): we keep the last-synced raw config as a
//! snapshot and compare *decoded source bytes* (CLAUDE.md §5,
//! `docs/api/04-scripts.md`).

use super::{Kind, RemoteRef, RemoteScript};
use crate::config::ProjectConfig;
use crate::{Error, Result};
use serde_json::Value;
use std::path::PathBuf;

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
    pub state: ScriptState,
}

// ---------------------------------------------------------------------------
// Snapshot store: .aic-sync/{manifest.json, configs/<kind>/<file>, backups/}
// ---------------------------------------------------------------------------

/// On-disk record of the last-synced state for one tenant + realm tree.
pub struct SnapshotStore {
    dir: PathBuf,
}

impl SnapshotStore {
    pub fn open(tenant: &str, realm: &str) -> Self {
        SnapshotStore {
            dir: ProjectConfig::aic_sync_dir(tenant, realm),
        }
    }

    fn manifest_path(&self) -> PathBuf {
        self.dir.join("manifest.json")
    }

    fn config_path(&self, r: &RemoteRef) -> PathBuf {
        self.dir
            .join("configs")
            .join(r.kind.as_str())
            .join(r.kind.config_filename(r))
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.dir.join("backups")
    }

    /// The manifest of every script we've synced into this tree.
    pub fn load_manifest(&self) -> Result<Vec<RemoteRef>> {
        let path = self.manifest_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = std::fs::read(&path)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn save_manifest(&self, entries: &[RemoteRef]) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.manifest_path(), serde_json::to_vec_pretty(entries)?)?;
        Ok(())
    }

    /// Insert or replace a manifest entry, keyed by (kind, name).
    fn upsert_ref(&self, r: &RemoteRef) -> Result<()> {
        let mut entries = self.load_manifest()?;
        if let Some(slot) = entries
            .iter_mut()
            .find(|e| e.kind == r.kind && e.name == r.name)
        {
            *slot = r.clone();
        } else {
            entries.push(r.clone());
        }
        self.save_manifest(&entries)
    }

    fn remove_ref(&self, kind: Kind, name: &str) -> Result<()> {
        let mut entries = self.load_manifest()?;
        entries.retain(|e| !(e.kind == kind && e.name == name));
        self.save_manifest(&entries)
    }

    /// Last-synced raw config (the snapshot we forked from), if present.
    pub fn load_config(&self, r: &RemoteRef) -> Result<Option<Value>> {
        let path = self.config_path(r);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path)?;
        Ok(Some(serde_json::from_slice(&bytes)?))
    }

    fn save_config(&self, r: &RemoteRef, raw: &Value) -> Result<()> {
        let path = self.config_path(r);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(raw)?)?;
        Ok(())
    }

    /// Record a script as synced: store its raw config + manifest entry.
    fn record(&self, script: &RemoteScript) -> Result<()> {
        self.save_config(&script.reference, &script.raw_config)?;
        self.upsert_ref(&script.reference)
    }

    fn manifest_lookup(&self, kind: Kind, name: &str) -> Result<Option<RemoteRef>> {
        Ok(self
            .load_manifest()?
            .into_iter()
            .find(|e| e.kind == kind && e.name == name))
    }
}

fn workspace_file(tenant: &str, realm: &str, r: &RemoteRef) -> PathBuf {
    ProjectConfig::workspace_tree(tenant, realm).join(r.kind.workspace_subpath(r))
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

// ---------------------------------------------------------------------------
// Pull
// ---------------------------------------------------------------------------

/// Pull scripts of `kind` matching `selector` into the workspace, updating the
/// snapshot store. Protects un-pushed local edits with a backup unless `force`.
pub async fn pull(
    tenant: &str,
    realm: &str,
    kind: Kind,
    selector: &Selector,
    force: bool,
) -> Result<Vec<PullOutcome>> {
    let store = SnapshotStore::open(tenant, realm);
    let refs: Vec<RemoteRef> = kind
        .list(tenant, realm)
        .await?
        .into_iter()
        .filter(|r| selector.matches(r))
        .collect();

    if let Selector::Name(n) = selector {
        if refs.is_empty() {
            return Err(Error::Config(format!("no {} script named {n:?}", kind.as_str())));
        }
    }

    let mut outcomes = Vec::new();
    for r in &refs {
        let script = kind.fetch(tenant, realm, &r.id).await?;
        let remote_src = kind.decode_source(&script.raw_config)?;

        let dest = workspace_file(tenant, realm, &script.reference);
        let snapshot_src = match store.load_config(&script.reference)? {
            Some(cfg) => Some(kind.decode_source(&cfg)?),
            None => None,
        };
        let local_src = if dest.exists() {
            Some(std::fs::read(&dest)?)
        } else {
            None
        };

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
        store.record(&script)?;

        outcomes.push(PullOutcome {
            name: script.reference.name.clone(),
            kind,
            status,
        });
    }
    Ok(outcomes)
}

fn write_workspace_files(
    tenant: &str,
    realm: &str,
    script: &RemoteScript,
    source: &[u8],
) -> Result<()> {
    let tree = ProjectConfig::workspace_tree(tenant, realm);
    let dest = tree.join(script.reference.kind.workspace_subpath(&script.reference));
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, source)?;
    for (rel, contents) in script.reference.kind.extra_files(&script.reference) {
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
/// exist). Content-based conflict check unless `force`.
pub async fn push(
    tenant: &str,
    realm: &str,
    kind: Kind,
    name: &str,
    force: bool,
    confirmed_prod: bool,
) -> Result<PushOutcome> {
    let store = SnapshotStore::open(tenant, realm);
    let r = store
        .manifest_lookup(kind, name)?
        .ok_or_else(|| Error::Config(format!("{name:?} not synced yet — `aic script pull {name}` first")))?;

    // Product-shipped defaults shouldn't be overwritten (AIC may 403 or
    // silently no-op anyway — see docs/api/04-scripts.md). --force is the
    // explicit escape hatch.
    if r.is_default && !force {
        return Err(Error::Config(format!(
            "{name:?} is a default (product-shipped) script — refusing to overwrite; pass --force to override"
        )));
    }

    let snapshot_cfg = store
        .load_config(&r)?
        .ok_or_else(|| Error::Config(format!("snapshot for {name:?} missing — pull again")))?;
    let snapshot_src = kind.decode_source(&snapshot_cfg)?;

    let dest = workspace_file(tenant, realm, &r);
    let local_src = std::fs::read(&dest)
        .map_err(|_| Error::Config(format!("local file {} not found — pull first", dest.display())))?;

    // No local change vs the snapshot → nothing to do.
    if local_src == snapshot_src {
        return Ok(PushOutcome::Unchanged);
    }

    // Conflict check: refetch remote, compare decoded bytes to the snapshot.
    let remote = kind.fetch(tenant, realm, &r.id).await?;
    let remote_src = kind.decode_source(&remote.raw_config)?;

    if remote_src == local_src {
        // Someone already pushed identical content; just refresh the snapshot.
        store.record(&remote)?;
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
    store.record(&to_push)?;
    Ok(PushOutcome::Pushed)
}

/// The raw config we last synced for a script — used by the CLI to record an
/// undo entry (previous remote content) before a push.
pub fn last_synced_config(tenant: &str, realm: &str, kind: Kind, name: &str) -> Result<Option<Value>> {
    let store = SnapshotStore::open(tenant, realm);
    match store.manifest_lookup(kind, name)? {
        Some(r) => store.load_config(&r),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Status / diff
// ---------------------------------------------------------------------------

/// Compute the state of every synced script (optionally just one kind).
pub async fn status(tenant: &str, realm: &str, only: Option<Kind>) -> Result<Vec<StatusEntry>> {
    let store = SnapshotStore::open(tenant, realm);
    let mut out = Vec::new();
    for r in store.load_manifest()? {
        if let Some(k) = only {
            if r.kind != k {
                continue;
            }
        }
        let snapshot_cfg = match store.load_config(&r)? {
            Some(c) => c,
            None => continue,
        };
        let snapshot_src = r.kind.decode_source(&snapshot_cfg)?;

        let dest = workspace_file(tenant, realm, &r);
        if !dest.exists() {
            out.push(StatusEntry { name: r.name.clone(), kind: r.kind, state: ScriptState::LocalMissing });
            continue;
        }
        let local_src = std::fs::read(&dest)?;
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
        out.push(StatusEntry { name: r.name.clone(), kind: r.kind, state });
    }
    Ok(out)
}

/// 3-way content for one script: last-synced snapshot, current remote, current
/// local. The CLI renders these; a real merge UI is future work.
pub async fn diff(tenant: &str, realm: &str, kind: Kind, name: &str) -> Result<ThreeWay> {
    let store = SnapshotStore::open(tenant, realm);
    let r = store
        .manifest_lookup(kind, name)?
        .ok_or_else(|| Error::Config(format!("{name:?} not synced yet")))?;
    let snapshot_cfg = store
        .load_config(&r)?
        .ok_or_else(|| Error::Config(format!("snapshot for {name:?} missing")))?;
    let snapshot_src = kind.decode_source(&snapshot_cfg)?;

    let dest = workspace_file(tenant, realm, &r);
    let local_src = if dest.exists() {
        std::fs::read(&dest)?
    } else {
        Vec::new()
    };
    let remote = kind.fetch(tenant, realm, &r.id).await?;
    let remote_src = kind.decode_source(&remote.raw_config)?;

    Ok(ThreeWay {
        last_synced: lossy(&snapshot_src),
        remote: lossy(&remote_src),
        local: lossy(&local_src),
    })
}

/// Remove a script's snapshot + manifest entry after a remote delete. Does not
/// touch the user's local `.cjs` (they may still want it).
pub fn forget(tenant: &str, realm: &str, kind: Kind, name: &str) -> Result<()> {
    let store = SnapshotStore::open(tenant, realm);
    store.remove_ref(kind, name)
}
