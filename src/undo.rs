//! Best-effort undo log.
//!
//! Screens record reversible writes through this module. The write path does
//! not need to know whether entries live only in memory or are also persisted
//! to disk.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::ProjectConfig;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UndoId(pub uuid::Uuid);

impl UndoId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for UndoId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for UndoId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sensitivity {
    PublicMetadata,
    TenantConfig,
    SensitiveValue,
    SecretValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    Undoable,
    BestEffort,
    Irreversible,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryStatus {
    Pending,
    AppliedSuccess,
    AppliedConflict,
    AppliedFailure,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictCheck {
    ContentEqualsAfter { body: serde_json::Value },
    ResourceAbsent,
    ContentEqualsBefore { body: serde_json::Value },
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UndoOp {
    EsvVariableRestore {
        tenant: String,
        body: serde_json::Value,
    },
    EsvVariableDelete {
        tenant: String,
        id: String,
        recorded_body: serde_json::Value,
    },
    EsvVariableUpdateTo {
        tenant: String,
        id: String,
        body: serde_json::Value,
    },
}

impl UndoOp {
    pub fn tenant(&self) -> &str {
        match self {
            UndoOp::EsvVariableRestore { tenant, .. }
            | UndoOp::EsvVariableDelete { tenant, .. }
            | UndoOp::EsvVariableUpdateTo { tenant, .. } => tenant,
        }
    }

    pub fn resource_id(&self) -> Option<&str> {
        match self {
            UndoOp::EsvVariableRestore { body, .. } => body.get("_id").and_then(|v| v.as_str()),
            UndoOp::EsvVariableDelete { id, .. } | UndoOp::EsvVariableUpdateTo { id, .. } => {
                Some(id)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoEntry {
    pub id: UndoId,
    pub created_at: DateTime<Utc>,
    pub tenant: String,
    pub actor: String,
    pub description: String,
    pub sensitivity: Sensitivity,
    pub capability: Capability,
    pub op: Option<UndoOp>,
    pub conflict_check: ConflictCheck,
    pub status: EntryStatus,
}

impl UndoEntry {
    pub fn pending(
        tenant: String,
        actor: impl Into<String>,
        description: impl Into<String>,
        sensitivity: Sensitivity,
        capability: Capability,
        op: Option<UndoOp>,
        conflict_check: ConflictCheck,
    ) -> Self {
        Self {
            id: UndoId::new(),
            created_at: Utc::now(),
            tenant,
            actor: actor.into(),
            description: description.into(),
            sensitivity,
            capability,
            op,
            conflict_check,
            status: EntryStatus::Pending,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UndoSummary {
    pub id: UndoId,
    pub created_at: DateTime<Utc>,
    pub tenant: String,
    pub actor: String,
    pub description: String,
    pub sensitivity: Sensitivity,
    pub capability: Capability,
    pub status: EntryStatus,
}

impl From<&UndoEntry> for UndoSummary {
    fn from(entry: &UndoEntry) -> Self {
        Self {
            id: entry.id,
            created_at: entry.created_at,
            tenant: entry.tenant.clone(),
            actor: entry.actor.clone(),
            description: entry.description.clone(),
            sensitivity: entry.sensitivity,
            capability: entry.capability,
            status: entry.status,
        }
    }
}

pub trait UndoLog: Send {
    fn record(&mut self, entry: UndoEntry) -> Result<UndoId>;
    fn list(&self, limit: usize) -> Vec<UndoSummary>;
    fn load(&self, id: UndoId) -> Result<UndoEntry>;
    fn mark_applied(&mut self, id: UndoId, status: EntryStatus) -> Result<()>;
    fn latest_pending(&self, tenant: &str) -> Option<UndoSummary>;
}

#[derive(Debug, Default)]
pub struct MemoryLog {
    entries: Vec<UndoEntry>,
}

impl MemoryLog {
    pub fn new() -> Self {
        Self::default()
    }
}

impl UndoLog for MemoryLog {
    fn record(&mut self, entry: UndoEntry) -> Result<UndoId> {
        let id = entry.id;
        self.entries.push(entry);
        Ok(id)
    }

    fn list(&self, limit: usize) -> Vec<UndoSummary> {
        summaries(&self.entries, limit)
    }

    fn load(&self, id: UndoId) -> Result<UndoEntry> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
            .ok_or_else(|| Error::Config(format!("undo entry not found: {id}")))
    }

    fn mark_applied(&mut self, id: UndoId, status: EntryStatus) -> Result<()> {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.status = status;
            return Ok(());
        }
        Err(Error::Config(format!("undo entry not found: {id}")))
    }

    fn latest_pending(&self, tenant: &str) -> Option<UndoSummary> {
        latest_pending_summary(&self.entries, tenant)
    }
}

#[derive(Debug)]
pub struct DiskLog {
    path: PathBuf,
    entries: Vec<UndoEntry>,
}

impl DiskLog {
    pub fn load_default() -> Result<Self> {
        Self::load(ProjectConfig::dir().join("undo.log"))
    }

    pub fn load(path: PathBuf) -> Result<Self> {
        let entries = load_entries(&path)?;
        Ok(Self { path, entries })
    }

    fn append_entry(&self, entry: &UndoEntry) -> Result<()> {
        if !persistable(entry.sensitivity) {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        ProjectConfig::write_gitignore()?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{}", serde_json::to_string(entry)?)?;
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    }

    fn rewrite(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        ProjectConfig::write_gitignore()?;
        let tmp = self.path.with_extension("log.tmp");
        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp)?;
            for entry in self
                .entries
                .iter()
                .filter(|entry| persistable(entry.sensitivity))
            {
                writeln!(file, "{}", serde_json::to_string(entry)?)?;
            }
        }
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
        fs::rename(tmp, &self.path)?;
        Ok(())
    }
}

impl UndoLog for DiskLog {
    fn record(&mut self, entry: UndoEntry) -> Result<UndoId> {
        let id = entry.id;
        self.append_entry(&entry)?;
        self.entries.push(entry);
        Ok(id)
    }

    fn list(&self, limit: usize) -> Vec<UndoSummary> {
        summaries(&self.entries, limit)
    }

    fn load(&self, id: UndoId) -> Result<UndoEntry> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
            .ok_or_else(|| Error::Config(format!("undo entry not found: {id}")))
    }

    fn mark_applied(&mut self, id: UndoId, status: EntryStatus) -> Result<()> {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.status = status;
            self.rewrite()?;
            return Ok(());
        }
        Err(Error::Config(format!("undo entry not found: {id}")))
    }

    fn latest_pending(&self, tenant: &str) -> Option<UndoSummary> {
        latest_pending_summary(&self.entries, tenant)
    }
}

fn load_entries(path: &Path) -> Result<Vec<UndoEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let body = fs::read_to_string(path)?;
    let mut entries = Vec::new();
    for line in body.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<UndoEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(err) => tracing::warn!(error = %err, "skipping corrupt undo log entry"),
        }
    }
    Ok(entries)
}

fn persistable(sensitivity: Sensitivity) -> bool {
    matches!(sensitivity, Sensitivity::PublicMetadata | Sensitivity::TenantConfig)
}

fn summaries(entries: &[UndoEntry], limit: usize) -> Vec<UndoSummary> {
    let mut out: Vec<_> = entries.iter().map(UndoSummary::from).collect();
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    out.truncate(limit);
    out
}

fn latest_pending_summary(entries: &[UndoEntry], tenant: &str) -> Option<UndoSummary> {
    entries
        .iter()
        .filter(|entry| {
            entry.tenant == tenant
                && entry.status == EntryStatus::Pending
                && matches!(entry.capability, Capability::Undoable | Capability::BestEffort)
                && entry.op.is_some()
        })
        .max_by_key(|entry| entry.created_at)
        .map(UndoSummary::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(description: &str) -> UndoEntry {
        UndoEntry::pending(
            "sandbox".to_string(),
            "esv",
            description,
            Sensitivity::PublicMetadata,
            Capability::Undoable,
            Some(UndoOp::EsvVariableDelete {
                tenant: "sandbox".to_string(),
                id: "esv-test".to_string(),
                recorded_body: serde_json::json!({ "_id": "esv-test" }),
            }),
            ConflictCheck::ContentEqualsAfter {
                body: serde_json::json!({ "_id": "esv-test" }),
            },
        )
    }

    #[test]
    fn memory_log_lists_newest_first_and_marks_status() {
        let mut log = MemoryLog::new();
        let first = log.record(entry("first")).unwrap();
        let second = log.record(entry("second")).unwrap();

        let listed = log.list(10);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, second);
        assert_eq!(listed[1].id, first);

        log.mark_applied(second, EntryStatus::AppliedSuccess).unwrap();
        assert_eq!(
            log.load(second).unwrap().status,
            EntryStatus::AppliedSuccess
        );
        assert_eq!(log.latest_pending("sandbox").unwrap().id, first);
    }

    #[test]
    fn secret_entries_are_memory_only_for_disk_log() {
        let path = std::env::temp_dir().join(format!("aic-edit-undo-{}.log", uuid::Uuid::new_v4()));
        let mut log = DiskLog::load(path.clone()).unwrap();
        let mut secret = entry("secret");
        secret.sensitivity = Sensitivity::SecretValue;

        let id = log.record(secret).unwrap();
        assert!(path.exists() == false || fs::read_to_string(&path).unwrap().is_empty());
        assert_eq!(log.load(id).unwrap().sensitivity, Sensitivity::SecretValue);

        let _ = fs::remove_file(path);
    }
}
