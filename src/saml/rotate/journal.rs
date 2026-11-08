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
//!   which `ProjectConfig::gitignore_content` covers. A change installs
//!   itself by rename, so there are **three** names to cover, not one: the
//!   document, the lock the writers take ([`lock_path`]) and the copy a
//!   change is assembled in ([`temp_path`]).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::ProjectConfig;
use crate::saml::rotate::spec::StagePermit;
use crate::statefile;
use crate::{Error, Result};

/// The file, beside `undo.log` and the vault, under `.aic/`.
pub const JOURNAL_FILE: &str = "saml-rotations.json";

/// The lock a writer holds, beside the journal it protects
/// ([`crate::statefile`] says why it is a sidecar).
pub fn lock_path(file: &Path) -> PathBuf {
    statefile::lock_path(file)
}

/// Where a change is assembled before it is renamed over the journal.
pub fn temp_path(file: &Path) -> PathBuf {
    statefile::temp_path(file)
}

const DESCRIBE: statefile::Describe = statefile::Describe {
    what: "the SAML rotation journal",
    unreadable,
};

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
/// the sidecar lock, the temporary and the rename included — rather than a
/// copy of its predicate.
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
/// **A reader takes no lock, and does not need one.** [`update_at`] installs a
/// change by renaming a finished document over this path, so an `open` lands
/// either on the document before the change or on the one after it, never on
/// a file mid-write — and a descriptor already held keeps reading the version
/// it opened, because the rename gave the new content a different inode. A
/// lock here would buy a reader nothing it could use: it would still be racing
/// the writer, only with its answer taken a moment earlier.
pub(crate) fn load_at(file: &Path) -> Result<Vec<StagedRecord>> {
    statefile::load(file, &DESCRIBE)
}

/// Read, change and install the journal, with nothing able to interleave and
/// no instant at which the path names a half-applied change.
///
/// Two separate faults, one fix each.
///
/// **Exclusion.** Two `aic` commands finishing rollovers for different roles
/// each read the whole array, dropped one entry and wrote the whole array
/// back; whichever wrote second restored the entry the first had removed, or
/// erased the one it had added. The file is one document for every rollover
/// this install knows about, so every change is a read-modify-write of all of
/// them, and an exclusive lock held across the pair is what makes it one.
///
/// **Atomicity.** The new document is written to [`temp_path`], `sync_all`ed
/// and **renamed** over the journal, so the path resolves to a whole document
/// at every instant and a crash loses the change rather than corrupting it.
///
/// This used to be an in-place rewrite — seek, write, then trim — defended on
/// the grounds that a crash between the write and the trim leaves invalid
/// JSON, which [`load_at`] refuses loudly. **That defence is false, and it is
/// the reason for the rename.** A partial overwrite of a same-shaped document
/// need not be invalid: bumping `"version": "1"` to `"version": "2"` and dying
/// before the tail is rewritten leaves a document that parses, pairing the
/// **new** version number with the **old** certificate fingerprint. During a
/// two-certificate window each half is independently true of the tenant, so
/// `spec::usable_pairing` accepts the record and `complete` derives from it —
/// disabling the version that holds the certificate the operator asked to
/// keep, which is the single failure this verb exists to prevent. A journal
/// that is missing costs its operator two flags; a journal that is
/// confidently wrong costs a live federation, so the one exposure worth
/// keeping is the one that loses the change.
///
/// **Why the lock is a sidecar.** A rename installs a new inode, so a lock
/// taken on the journal itself would be stranded on the old one and the next
/// writer would lock a file nobody holds — protection that reads as
/// protection. The lock therefore lives on [`lock_path`], whose inode nothing
/// replaces; that file is only ever locked, never read or written. Its cost is
/// one more name for `ProjectConfig::gitignore_content` to carry, and nothing
/// else: the lock (`flock` underneath) is released by the kernel when the descriptor closes, so a
/// killed `aic` leaves a stale *file* and never a stale lock.
pub(crate) fn update_at(file: &Path, change: impl FnOnce(&mut Vec<StagedRecord>)) -> Result<()> {
    statefile::update(file, &DESCRIBE, change)
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

/// The error for a journal that does not parse. A zero-length one is read as
/// "nothing was staged" by [`statefile::load`] — safe in the direction that
/// matters, because `complete` without a record refuses to derive anything and
/// demands both `--retain` and `--disable-version`.
fn unreadable(file: &Path, error: &serde_json::Error) -> Error {
    Error::Config(format!(
        "the SAML rotation journal {} is not readable ({error}); it records which ESV \
         secret version holds which certificate, so move it aside and re-run naming both \
         halves — --retain <sha256> for the certificate to keep and --disable-version <n> \
         for the version to disable — rather than guessing. `aic saml rotate status` lists \
         the versions and the fingerprints.",
        file.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Read;

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
        // same conflict another `aic` would meet. It is taken on the sidecar,
        // because that is where the lock lives once a change installs itself
        // by rename — a lock on the journal would be stranded on the inode
        // the rename replaced.
        let mut locked_out = None;
        update_at(&file, |entries| {
            locked_out = Some(!shared_lock_available(&lock_path(&file)));
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
            "a second writer could have started part-way through this change"
        );

        // And the change itself is a read-modify-write: the entry that was
        // already there survives adding one, which is what a wholesale save
        // from a stale read would lose.
        let after = load_at(&file).expect("reload");
        assert_eq!(after.len(), 2);
        assert!(find_in(&after, &key("https://sp-a.example.com", "serviceProvider")).is_some());
        assert!(find_in(&after, &key("https://sp-b.example.com", "serviceProvider")).is_some());

        // Released afterwards, so the next command is not locked out forever.
        // Polled rather than read once: a sibling test that spawns a process
        // while the lock is held gives the child a copy of the descriptor,
        // which lasts until its exec closes it (seen as a rare failure under
        // the full parallel suite, never alone).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !shared_lock_available(&lock_path(&file)) {
            assert!(
                std::time::Instant::now() < deadline,
                "the journal lock was never released"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The counterexample to "a torn journal is caught by being invalid JSON",
    /// which is the argument the in-place rewrite rested on.
    ///
    /// Not a regression guard — `load_at` is *right* to accept these bytes,
    /// and would be wrong to guess which half of them is stale. It is the
    /// statement of the hazard, kept next to the code that no longer produces
    /// it, because the next person to weigh a rename against a third path
    /// under `.aic/` needs to see what the other side of that trade costs.
    #[test]
    fn a_half_applied_in_place_rewrite_parses_and_pairs_a_new_version_with_an_old_certificate() {
        // One record, one stage apart. A stage bumps the version AND the
        // fingerprint, and both strings keep their length, so the two
        // documents are byte-for-byte the same size — which is what makes the
        // old writer's trailing `set_len` a no-op and the tear invisible.
        let before = vec![record_for(
            "https://sp-a.example.com",
            "serviceProvider",
            "1",
            "aaaaaaaa",
        )];
        let after = vec![record_for(
            "https://sp-a.example.com",
            "serviceProvider",
            "2",
            "bbbbbbbb",
        )];
        let old_bytes = serde_json::to_vec_pretty(&before).expect("serialise");
        let new_bytes = serde_json::to_vec_pretty(&after).expect("serialise");
        assert_eq!(
            old_bytes.len(),
            new_bytes.len(),
            "the fixture only reproduces the hazard if the two documents are the same size"
        );

        // Die after the version field and before the fingerprint: seek, write
        // a prefix, never reach the rest. The tail is whatever was on disk.
        let cut = offset_of(&new_bytes, b"\"sha256\"");
        let torn = [&new_bytes[..cut], &old_bytes[cut..]].concat();

        let dir = std::env::temp_dir().join(format!("aic-rottorn-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join(JOURNAL_FILE);
        std::fs::write(&file, &torn).expect("write the torn document");

        let believed = load_at(&file).expect("a torn document that parses is not refused");
        assert_eq!(believed.len(), 1);
        assert_eq!(believed[0].version, "2", "the prefix is the new document");
        assert_eq!(
            believed[0].sha256, "aaaaaaaa",
            "the tail is the old one — and during a two-certificate window both halves are \
             independently true of the tenant, so nothing downstream can tell this record \
             from a real one"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Red the moment a change goes back into the journal's own inode.
    ///
    /// The two assertions are the same property from both ends: the path gets
    /// a new inode (so the change arrived by rename, whole), and a descriptor
    /// opened before the change still reads the document it opened (so no
    /// reader can be shown a half-applied one). An in-place rewrite of a
    /// same-sized document fails both — the inode is unchanged and the held
    /// descriptor sees the new bytes appear underneath it.
    #[test]
    fn a_change_arrives_by_rename_so_nothing_can_read_a_half_applied_journal() {
        let dir = std::env::temp_dir().join(format!("aic-rotrename-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join(JOURNAL_FILE);
        save_at(
            &file,
            &[record_for(
                "https://sp-a.example.com",
                "serviceProvider",
                "1",
                "aaaaaaaa",
            )],
        )
        .expect("seed");

        let was = std::fs::read_to_string(&file).expect("read the seeded document");
        let was_inode = inode(&file);
        let mut held = File::open(&file).expect("a reader that opened before the change");

        update_at(&file, |entries| {
            entries[0].version = "2".into();
            entries[0].sha256 = "bbbbbbbb".into();
        })
        .expect("update");

        assert_ne!(
            inode(&file),
            was_inode,
            "the change was written into the journal's own inode, so a crash part-way \
             through could leave a document that parses and lies"
        );
        let mut seen = String::new();
        held.read_to_string(&mut seen).expect("read");
        assert_eq!(
            seen, was,
            "a descriptor opened before the change watched it happen"
        );
        assert!(
            !temp_path(&file).exists(),
            "the copy the change was assembled in outlived the rename"
        );

        // And the change landed whole, not just atomically.
        let after = load_at(&file).expect("reload");
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].version, "2");
        assert_eq!(after[0].sha256, "bbbbbbbb");

        std::fs::remove_dir_all(&dir).ok();
    }

    fn inode(file: &Path) -> u64 {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(file).expect("stat").ino()
    }

    fn offset_of(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
            .expect("the field the tear falls after")
    }

    /// Whether another process could read the journal right now — a separate
    /// open is a separate lock holder even inside this process, so this is the
    /// conflict a second `aic` would meet.
    fn shared_lock_available(file: &Path) -> bool {
        let handle = File::open(file).expect("open");
        handle.try_lock_shared().is_ok()
    }

    /// `record`, but against a caller-chosen file rather than `.aic/`.
    fn record_at(file: &Path, entry: StagedRecord) -> Result<()> {
        update_at(file, |entries| {
            entries.retain(|existing| !key_of(&entry).matches(existing));
            entries.push(entry);
        })
    }
}
