//! A JSON array of records kept under `.aic/`, changed atomically.
//!
//! Extracted from `saml::rotate::journal` when `managed relationship relink`
//! needed the same guarantees for its recovery journal; the reasoning below was
//! learned there and applies to both.
//!
//! **Exclusion.** Every change is a read-modify-write of the whole array, so
//! two `aic` commands each dropping or adding their own entry would otherwise
//! erase each other's. An exclusive lock is held across the pair.
//!
//! **Atomicity.** The new document is written to [`temp_path`], `sync_all`ed
//! and **renamed** over the file, so the path resolves to a whole document at
//! every instant and a crash loses the change rather than corrupting it. An
//! in-place rewrite is not enough even though truncated JSON fails to parse: a
//! partial overwrite of a same-shaped document can still parse, pairing new
//! fields with old ones.
//!
//! **Why the lock is a sidecar.** A rename installs a new inode, so a lock on
//! the file itself would be stranded on the old one. The lock lives on
//! [`lock_path`], whose inode nothing replaces; it is only ever locked, never
//! read or written. `flock` underneath is released when the descriptor
//! closes, so a killed `aic` leaves a stale *file* and never a stale lock.
//!
//! So every such file has **three** names for
//! `ProjectConfig::gitignore_content` to carry: the document, `.lock` and
//! `.new`.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{Error, Result};

/// How a caller's file is named in errors, and what an unreadable one means
/// for that caller's operator.
pub struct Describe {
    /// "the SAML rotation journal", used as `"<verb> <what> <path>: <error>"`.
    pub what: &'static str,
    /// The error for a file that exists and does not parse. Never "empty":
    /// garbage read as "nothing recorded" is the silent failure these files
    /// exist to prevent.
    pub unreadable: fn(&Path, &serde_json::Error) -> Error,
}

/// The lock a writer holds, beside the file it protects.
pub fn lock_path(file: &Path) -> PathBuf {
    sidecar(file, ".lock")
}

/// Where a change is assembled before it is renamed over the file. A fixed
/// name: only the lock holder assembles, and a predictable name is one a
/// `.gitignore` can carry. A copy left by a crash is overwritten, never read.
pub fn temp_path(file: &Path) -> PathBuf {
    sidecar(file, ".new")
}

fn sidecar(file: &Path, suffix: &str) -> PathBuf {
    let mut name = file.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// Read the file, treating an absent or zero-length one as empty and a
/// corrupt one as an error.
///
/// **A reader takes no lock, and does not need one.** [`update`] installs by
/// rename, so an `open` lands on a whole document before or after a change.
pub fn load<T: DeserializeOwned>(file: &Path, describe: &Describe) -> Result<Vec<T>> {
    let mut handle = match File::open(file) {
        Ok(handle) => handle,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_failure("read", file, describe, &error)),
    };
    let mut bytes = Vec::new();
    handle
        .read_to_end(&mut bytes)
        .map_err(|error| io_failure("read", file, describe, &error))?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_slice(&bytes).map_err(|error| (describe.unreadable)(file, &error))
}

/// Read, change and install the file, with nothing able to interleave and no
/// instant at which the path names a half-applied change. Returns whatever
/// `change` returns.
pub fn update<T, R>(
    file: &Path,
    describe: &Describe,
    change: impl FnOnce(&mut Vec<T>) -> R,
) -> Result<R>
where
    T: Serialize + DeserializeOwned,
{
    if let Some(parent) = parent_of(file) {
        std::fs::create_dir_all(parent)?;
    }
    let lock_file = lock_path(file);
    // Held across the read and the write both, and released when `guard`
    // drops — on the error paths, and on a kill.
    let guard = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_file)
        .map_err(|error| io_failure("lock", &lock_file, describe, &error))?;
    guard
        .lock()
        .map_err(|error| io_failure("lock", &lock_file, describe, &error))?;

    let mut entries = load(file, describe)?;
    let out = change(&mut entries);
    install(file, describe, &serde_json::to_vec_pretty(&entries)?)?;
    Ok(out)
}

/// Make `body` the whole file, by rename. The temporary is synced before the
/// rename; the directory after it, best effort.
fn install(file: &Path, describe: &Describe, body: &[u8]) -> Result<()> {
    let temp = temp_path(file);
    let mut handle = OpenOptions::new()
        .mode(0o600)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temp)
        .map_err(|error| io_failure("write", &temp, describe, &error))?;
    handle
        .write_all(body)
        .and_then(|()| handle.sync_all())
        .map_err(|error| io_failure("write", &temp, describe, &error))?;
    drop(handle);

    std::fs::rename(&temp, file).map_err(|error| io_failure("install", file, describe, &error))?;

    if let Some(dir) = parent_of(file).and_then(|parent| File::open(parent).ok()) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// The directory `file` lives in, or `None` when the path is a bare name.
fn parent_of(file: &Path) -> Option<&Path> {
    file.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
}

fn io_failure(verb: &str, file: &Path, describe: &Describe, error: &std::io::Error) -> Error {
    Error::Config(format!(
        "{verb} {} {}: {error}",
        describe.what,
        file.display()
    ))
}
