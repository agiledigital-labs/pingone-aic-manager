//! What this install remembers about a rollover it started.
//!
//! **Almost everything a rotation needs is readable from the tenant**, and
//! that is where it is read from: whether the identifier is set, whether the
//! label is mapped, which versions are ENABLED, how many certificates the
//! export publishes. An interrupted run resumes by reading those, not by
//! trusting a file — so this journal is deliberately not a transaction log and
//! nothing refuses to proceed because it is missing.
//!
//! It records the one fact the tenant structurally cannot give back: **which
//! ESV secret version holds which certificate.** Secret values are write-only
//! (`docs/api/03-esvs.md`) and AM publishes no `<ds:KeyName>`
//! (`docs/api/06-saml.md`), so during a two-certificate window nothing
//! readable says which fingerprint came from which version. The metadata lists
//! the newest ENABLED version first, but that is an ordering convention, and a
//! `complete` that acts on it is one convention-change away from disabling the
//! new certificate and keeping the old — the single failure this whole verb
//! exists to prevent.
//!
//! So: `stage` writes an entry **after** the tenant's own export confirms the
//! new certificate is published, never from the bytes it sent (`.ai/core.md`
//! §5); `complete` reads it to know what to keep and removes it when the
//! window closes; and `status` says plainly when there is no entry, rather
//! than inferring one.
//!
//! Consequences worth stating, because they are limits and not bugs:
//!
//! - it is **per install**. A rollover staged on another machine leaves no
//!   entry here, and `complete` then requires `--retain <sha256>`.
//! - it holds fingerprints, ids and version numbers — **no key material** —
//!   and lives beside the other per-machine runtime state under `.aic/`,
//!   which `ProjectConfig::gitignore_content` covers.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::ProjectConfig;
use crate::{Error, Result};

/// The file, beside `undo.log` and the vault, under `.aic/`.
pub const JOURNAL_FILE: &str = "saml-rotations.json";

/// One staged rollover, waiting to be completed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedRecord {
    pub tenant: String,
    pub realm: String,
    pub entity_id: String,
    /// The role's wire key — `identityProvider` or `serviceProvider`.
    pub role: String,
    pub identifier: String,
    pub secret_id: String,
    /// The ESV secret version `stage` created.
    pub version: String,
    /// The certificate that version carries, as published in the export that
    /// confirmed the stage.
    pub sha256: String,
    /// RFC 3339, so `status` can say how long the window has been open.
    pub staged_at: String,
}

/// What addresses one rollover. A dual-role entity has two, independently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub tenant: String,
    pub realm: String,
    pub entity_id: String,
    pub role: String,
}

impl Key {
    fn matches(&self, record: &StagedRecord) -> bool {
        record.tenant == self.tenant
            && record.realm == self.realm
            && record.entity_id == self.entity_id
            && record.role == self.role
    }
}

fn path() -> PathBuf {
    ProjectConfig::dir().join(JOURNAL_FILE)
}

/// Read one entry, if this install staged that rollover.
pub fn find(key: &Key) -> Result<Option<StagedRecord>> {
    Ok(find_in(&load_at(&path())?, key))
}

/// Record a staged rollover, replacing any earlier entry for the same key.
pub fn record(entry: StagedRecord) -> Result<()> {
    let file = path();
    let mut entries = load_at(&file)?;
    entries.retain(|existing| !key_of(&entry).matches(existing));
    entries.push(entry);
    save_at(&file, &entries)
}

/// Forget a rollover that has been completed.
pub fn clear(key: &Key) -> Result<()> {
    let file = path();
    let mut entries = load_at(&file)?;
    let before = entries.len();
    entries.retain(|existing| !key.matches(existing));
    if entries.len() == before {
        return Ok(());
    }
    save_at(&file, &entries)
}

fn key_of(record: &StagedRecord) -> Key {
    Key {
        tenant: record.tenant.clone(),
        realm: record.realm.clone(),
        entity_id: record.entity_id.clone(),
        role: record.role.clone(),
    }
}

pub(crate) fn find_in(entries: &[StagedRecord], key: &Key) -> Option<StagedRecord> {
    entries.iter().find(|entry| key.matches(entry)).cloned()
}

/// Read the journal, treating an absent file as an empty one.
///
/// A **corrupt** file is not treated as empty. Reading garbage as "nothing was
/// staged" would silently downgrade `complete` to the `--retain` path, which
/// is the point at which an operator supplies a fingerprint from memory.
pub(crate) fn load_at(file: &Path) -> Result<Vec<StagedRecord>> {
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(Error::Config(format!(
                "read the SAML rotation journal {}: {error}",
                file.display()
            )));
        }
    };
    serde_json::from_slice(&bytes).map_err(|error| {
        Error::Config(format!(
            "the SAML rotation journal {} is not readable ({error}); it records which ESV \
             secret version holds which certificate, so move it aside and re-run with \
             --retain <sha256> rather than guessing",
            file.display()
        ))
    })
}

pub(crate) fn save_at(file: &Path, entries: &[StagedRecord]) -> Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_vec_pretty(entries)?;
    std::fs::write(file, body).map_err(|error| {
        Error::Config(format!(
            "write the SAML rotation journal {}: {error}",
            file.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_for(entity: &str, role: &str, version: &str, sha: &str) -> StagedRecord {
        StagedRecord {
            tenant: "sandbox".into(),
            realm: "bravo".into(),
            entity_id: entity.into(),
            role: role.into(),
            identifier: "spa".into(),
            secret_id: "esv-saml-sp-a-signing".into(),
            version: version.into(),
            sha256: sha.into(),
            staged_at: "2026-09-17T00:00:00Z".into(),
        }
    }

    fn key(entity: &str, role: &str) -> Key {
        Key {
            tenant: "sandbox".into(),
            realm: "bravo".into(),
            entity_id: entity.into(),
            role: role.into(),
        }
    }

    #[test]
    fn a_dual_role_entity_keeps_two_independent_records() {
        // The discriminating case for the key: both entries carry the same
        // tenant, realm and entity id and differ only in the role, so a key
        // that stopped at the entity id would return the IdP's certificate
        // for an SP completion — and `complete` would keep the wrong one.
        let entries = vec![
            record_for("https://sp-a.example.com", "identityProvider", "2", "aaa"),
            record_for("https://sp-a.example.com", "serviceProvider", "5", "bbb"),
        ];
        assert_eq!(
            find_in(
                &entries,
                &key("https://sp-a.example.com", "serviceProvider")
            )
            .expect("the SP entry")
            .sha256,
            "bbb"
        );
        assert_eq!(
            find_in(
                &entries,
                &key("https://sp-a.example.com", "identityProvider")
            )
            .expect("the IdP entry")
            .version,
            "2"
        );
        assert!(
            find_in(
                &entries,
                &key("https://sp-b.example.com", "serviceProvider")
            )
            .is_none()
        );
    }

    #[test]
    fn an_absent_journal_is_empty_and_a_corrupt_one_is_an_error() {
        let dir = std::env::temp_dir().join(format!("aic-rotjournal-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join(JOURNAL_FILE);

        assert_eq!(load_at(&file).expect("absent is empty"), Vec::new());

        // Corruption must not read as "nothing was staged": that silently
        // downgrades `complete` to asking the operator for a fingerprint.
        std::fs::write(&file, b"{ this is not json").expect("write");
        assert!(load_at(&file).is_err());

        let entries = vec![record_for(
            "https://sp-a.example.com",
            "serviceProvider",
            "2",
            "abc",
        )];
        save_at(&file, &entries).expect("save");
        assert_eq!(load_at(&file).expect("reload"), entries);

        std::fs::remove_dir_all(&dir).ok();
    }
}
