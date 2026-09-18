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
//!   entry here, and `complete` then requires **both** `--retain <sha256>`
//!   and `--disable-version <n>`: a fingerprint names a certificate and
//!   nothing readable says which version holds it, so naming one without the
//!   other still leaves the choice to ordering.
//! - it holds fingerprints, ids and version numbers — **no key material** —
//!   and lives beside the other per-machine runtime state under `.aic/`,
//!   which `ProjectConfig::gitignore_content` covers.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::ProjectConfig;
use crate::saml::rotate::spec::StagePermit;
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
///
/// The [`StagePermit`] is the point: this file answers "which ESV secret
/// version holds which certificate" **during a two-certificate window**, and
/// only `stage` opens one. `init` used to reach the same writer through a
/// callback shared with `stage` and record its single certificate as version
/// "1", which a later `complete` then read as the rollover this install
/// staged. Requiring the permit the stage authorizer mints makes that
/// uncompilable rather than merely wrong — the same routing proof as
/// `scripts::gate`'s `WritePermit`, pointed at a local file instead of a
/// tenant.
pub fn record(entry: StagedRecord, _permit: &StagePermit) -> Result<()> {
    update_at(&path(), |entries| {
        entries.retain(|existing| !key_of(&entry).matches(existing));
        entries.push(entry);
    })
}

/// Forget a rollover that has been completed.
pub fn clear(key: &Key) -> Result<()> {
    update_at(&path(), |entries| {
        entries.retain(|existing| !key.matches(existing));
    })
}

/// Forget every record for an entity, both roles, because it no longer exists.
///
/// [`Key`] names a role, and a deleted entity takes all of its roles with it —
/// a dual-role entity mid-rollover on its IdP side leaves a record that
/// `clear` would need to be called twice to reach, with the caller guessing
/// which roles existed.
///
/// This is **hygiene, not the guard.** `spec::usable_pairing` is what protects
/// a recreated entity, by requiring the identifier, the secret id, the version
/// and the published fingerprint all to match the tenant before a record counts
/// as a pairing — so a survivor of a delete-and-recreate already fails to
/// match. That has to stay true independently, because an entity deleted in the
/// AM console, or from another install, never reaches this function at all.
/// Clearing here just stops `status` showing a rollover for something that is
/// gone.
pub fn clear_entity(tenant: &str, realm: &str, entity_id: &str) -> Result<()> {
    clear_entity_at(&path(), tenant, realm, entity_id)
}

/// [`clear_entity`] against a named file, so a test drives the real writer —
/// the lock, the in-place rewrite and the trim included — rather than a copy
/// of its predicate.
pub(crate) fn clear_entity_at(
    file: &Path,
    tenant: &str,
    realm: &str,
    entity_id: &str,
) -> Result<()> {
    update_at(file, |entries| {
        entries.retain(|existing| {
            !(existing.tenant == tenant
                && existing.realm == realm
                && existing.entity_id == entity_id)
        });
    })
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
/// staged" would silently downgrade `complete` to the two-flag path, which is
/// the point at which an operator supplies a fingerprint from memory.
///
/// The read takes a **shared** lock, so it waits out a rewrite rather than
/// seeing the middle of one. That is the half of the atomicity story a reader
/// owns; [`update_at`] owns the other half.
pub(crate) fn load_at(file: &Path) -> Result<Vec<StagedRecord>> {
    let mut handle = match File::open(file) {
        Ok(handle) => handle,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_failure("read", file, &error)),
    };
    // Released when `handle` drops, on the error paths too.
    lock(&handle, libc::LOCK_SH, file)?;
    let mut bytes = Vec::new();
    handle
        .read_to_end(&mut bytes)
        .map_err(|error| io_failure("read", file, &error))?;
    parse(&bytes, file)
}

/// Read, change and rewrite the journal, with nothing able to interleave.
///
/// Two things were wrong with the read-then-write this replaced, and they are
/// separate faults with one fix each.
///
/// **Exclusion.** Two `aic` commands finishing rollovers for different roles
/// each read the whole array, dropped one entry and wrote the whole array
/// back; whichever wrote second restored the entry the first had removed, or
/// erased the one it had added. The file is one document for every rollover
/// this install knows about, so every change is a read-modify-write of all of
/// them, and an exclusive lock held across the pair is what makes it one.
///
/// **Atomicity.** The lock lives on the journal itself rather than on a lock
/// file beside it, which is what decides the shape of the write. A rewrite
/// cannot then be `write a temporary and rename`: the rename swaps in a new
/// inode, the lock stays on the old one, and the next process to open the
/// path locks something nobody else is holding — a lock that protects
/// nothing is worse than none, because it reads as protection. So the bytes
/// go back into the file that is locked: seek to the start, write, then trim
/// to length. Writing *before* trimming is deliberate — a crash in between
/// leaves a good prefix followed by a stale tail, which is invalid JSON and
/// therefore refused by [`load_at`], where truncating first would leave a
/// zero-length file that reads as "nothing was staged".
///
/// The remaining exposure is a process killed mid-write, which loses the
/// pairing and says so loudly; `complete` then needs `--retain` and
/// `--disable-version`. That is the same position as an install that never
/// staged, and it is the failure this file is allowed to have.
pub(crate) fn update_at(file: &Path, change: impl FnOnce(&mut Vec<StagedRecord>)) -> Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut handle = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(file)
        .map_err(|error| io_failure("open", file, &error))?;
    lock(&handle, libc::LOCK_EX, file)?;

    let mut bytes = Vec::new();
    handle
        .read_to_end(&mut bytes)
        .map_err(|error| io_failure("read", file, &error))?;
    let mut entries = parse(&bytes, file)?;
    change(&mut entries);

    let body = serde_json::to_vec_pretty(&entries)?;
    handle
        .rewind()
        .and_then(|()| handle.write_all(&body))
        .and_then(|()| handle.set_len(body.len() as u64))
        .and_then(|()| handle.sync_all())
        .map_err(|error| io_failure("write", file, &error))
}

/// Replace the journal wholesale, under the same lock as any other change.
///
/// Nothing in production replaces the array — every change is a
/// read-modify-write of a document shared by every rollover this install
/// knows about — so this exists to seed and round-trip the file in tests.
#[cfg(test)]
pub(crate) fn save_at(file: &Path, entries: &[StagedRecord]) -> Result<()> {
    update_at(file, |current| {
        current.clear();
        current.extend_from_slice(entries);
    })
}

/// An empty file is an empty journal; anything else has to parse.
///
/// Zero bytes is reachable only where the file was created and never written
/// — [`update_at`] creates before it reads — and there "nothing was staged"
/// is the truth rather than a guess.
fn parse(bytes: &[u8], file: &Path) -> Result<Vec<StagedRecord>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_slice(bytes).map_err(|error| {
        Error::Config(format!(
            "the SAML rotation journal {} is not readable ({error}); it records which ESV \
             secret version holds which certificate, so move it aside and re-run naming both \
             halves — --retain <sha256> for the certificate to keep and --disable-version <n> \
             for the version to disable — rather than guessing. `aic saml rotate status` lists \
             the versions and the fingerprints.",
            file.display()
        ))
    })
}

/// Take a whole-file advisory lock, blocking until it is granted.
///
/// `std::fs::File::lock` is the obvious answer and is the wrong one here: it
/// is stable since 1.89, this crate declares 1.85, and
/// `clippy::incompatible_msrv` enforces that — so reaching for it would smuggle
/// a project-wide policy change in behind a journal fix. `flock` is the same
/// primitive one layer down, and the property that matters is the kernel's:
/// the lock is released when the process dies, so a killed `aic` cannot leave
/// the journal permanently unwritable the way a lock *file* would.
///
/// A raw syscall is the established shape for this in this binary
/// (`src/cli/mod.rs`'s `kill`, `src/agent/client.rs`'s `setsid`), which is
/// also why there is no `cfg(unix)` here: those two are unguarded, so the
/// crate already builds nowhere else.
fn lock(handle: &File, operation: i32, file: &Path) -> Result<()> {
    // SAFETY: `handle` owns the descriptor for the whole call, and `flock`
    // dereferences no pointer and writes no memory through one.
    if unsafe { libc::flock(handle.as_raw_fd(), operation) } == 0 {
        return Ok(());
    }
    Err(io_failure("lock", file, &std::io::Error::last_os_error()))
}

fn io_failure(verb: &str, file: &Path, error: &std::io::Error) -> Error {
    Error::Config(format!(
        "{verb} the SAML rotation journal {}: {error}",
        file.display()
    ))
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

    /// Red when `clear_entity` keys on the role, or forgets more than the
    /// entity it was given.
    #[test]
    fn deleting_an_entity_forgets_both_its_roles_and_nothing_else() {
        // Three records that make the two wrong implementations visible. Two
        // belong to the deleted entity and differ only in role, so a clear
        // that reuses `Key` — which names a role — leaves whichever role the
        // caller did not guess. The third is a different entity in the same
        // tenant and realm, so a clear that truncated the file, or matched on
        // tenant and realm alone, takes a live rollover with it.
        let dir = std::env::temp_dir().join(format!("aic-journal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let file = dir.join("saml-rotations.json");
        save_at(
            &file,
            &[
                record_for("https://sp-a.example.com", "identityProvider", "2", "aaa"),
                record_for("https://sp-a.example.com", "serviceProvider", "5", "bbb"),
                record_for("https://sp-b.example.com", "serviceProvider", "3", "ccc"),
            ],
        )
        .expect("seed the journal");

        clear_entity_at(&file, "sandbox", "bravo", "https://sp-a.example.com")
            .expect("clear the deleted entity");

        let left = load_at(&file).expect("reload");
        assert_eq!(left.len(), 1, "only the other entity should survive");
        assert_eq!(left[0].entity_id, "https://sp-b.example.com");
        assert_eq!(left[0].sha256, "ccc");

        // A different tenant or realm with the same entity id is a different
        // entity, and must be left alone.
        clear_entity_at(&file, "sandbox", "alpha", "https://sp-b.example.com")
            .expect("clear a realm that holds nothing");
        assert_eq!(load_at(&file).expect("reload").len(), 1);

        std::fs::remove_dir_all(&dir).ok();
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
        // A *change* refuses for the same reason — rewriting the array means
        // reading it first, and a journal that cannot be read is not one an
        // update may quietly replace.
        std::fs::write(&file, b"{ this is not json").expect("write");
        assert!(load_at(&file).is_err());
        assert!(
            record_at(
                &file,
                record_for("https://sp-a.example.com", "serviceProvider", "2", "abc")
            )
            .is_err()
        );
        std::fs::remove_file(&file).expect("move the corrupt one aside");

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

    /// Red when the read-modify-write happens without a lock across the pair.
    ///
    /// The defect this pins is two `aic` processes each rewriting the whole
    /// array: one drops the rollover it just completed, the other adds the
    /// one it just staged, and whichever writes second restores or erases the
    /// other's entry. Nothing fails, and the lost record is only noticed by
    /// the `complete` that then has no pairing.
    #[test]
    fn a_change_holds_the_journal_against_a_second_writer_across_the_whole_change() {
        let dir = std::env::temp_dir().join(format!("aic-rotlock-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join(JOURNAL_FILE);
        save_at(
            &file,
            &[record_for(
                "https://sp-a.example.com",
                "serviceProvider",
                "1",
                "aaa",
            )],
        )
        .expect("seed");

        // Observed from *inside* the change, which is the interval that
        // matters: the entries have been read and not yet written back, and
        // that is exactly when a second writer must not get in. A separate
        // open is a separate lock holder even in this process, so this is the
        // same conflict another `aic` would meet.
        let mut locked_out = None;
        update_at(&file, |entries| {
            locked_out = Some(File::open(&file).expect("open").try_lock_shared().is_err());
            entries.push(record_for(
                "https://sp-b.example.com",
                "serviceProvider",
                "2",
                "bbb",
            ));
        })
        .expect("update");
        assert_eq!(
            locked_out,
            Some(true),
            "the journal was readable part-way through its own rewrite"
        );

        // And the change itself is a read-modify-write: the entry that was
        // already there survives adding one, which is what a wholesale save
        // from a stale read would lose.
        let after = load_at(&file).expect("reload");
        assert_eq!(after.len(), 2);
        assert!(find_in(&after, &key("https://sp-a.example.com", "serviceProvider")).is_some());
        assert!(find_in(&after, &key("https://sp-b.example.com", "serviceProvider")).is_some());

        // Released afterwards, so the next command is not locked out forever.
        assert!(shared_lock_available(&file));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Whether another process could read the journal right now — a separate
    /// open is a separate lock holder even inside this process, so this is the
    /// conflict a second `aic` would meet.
    fn shared_lock_available(file: &Path) -> bool {
        let handle = File::open(file).expect("open");
        // SAFETY: as `lock`; the descriptor outlives the call.
        unsafe { libc::flock(handle.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) == 0 }
    }

    /// `record`, but against a caller-chosen file rather than `.aic/`.
    fn record_at(file: &Path, entry: StagedRecord) -> Result<()> {
        update_at(file, |entries| {
            entries.retain(|existing| !key_of(&entry).matches(existing));
            entries.push(entry);
        })
    }
}
