//! Rendering two versions of one thing as a diff.
//!
//! Shared by `aic script diff` and `aic oauth diff` so both surfaces spell a
//! comparison the same way — same side labels, same "identical" wording, same
//! pager. The rendering shells out to `git diff --no-index` with stdio
//! inherited, so your git pager and colour theme (delta, …) apply
//! interactively while `aic … diff X | <tool>` still pipes plain unified diff.

use crate::{Error, Result};

/// Removes the temp dir when it goes out of scope, including on an unwind.
///
/// The plain statement it replaces was skipped by any panic between creating
/// the dir and reaching it. This still cannot survive a `SIGKILL`, an
/// `abort`, or the machine losing power — a leaked `aic-diff-*` is 0700 with
/// 0600 files, which is the mitigation for those.
struct DiffDir {
    path: std::path::PathBuf,
}

impl DiffDir {
    fn new() -> Result<Self> {
        Ok(Self {
            path: create_diff_dir()?,
        })
    }

    /// Remove it now and report a failure, rather than swallowing it in
    /// `drop`. The `Drop` impl is the backstop for the paths that do not get
    /// here.
    fn remove(self) -> Result<()> {
        match std::fs::remove_dir_all(&self.path) {
            Ok(()) => {
                // Do not let `drop` try again on a path that is already gone.
                std::mem::forget(self);
                Ok(())
            }
            // Keep the guard alive so `drop` gets a second attempt: forgetting
            // here abandoned a directory that was still on disk, which is the
            // one case the guard exists for.
            Err(e) => Err(Error::Config(format!(
                "remove temp dir {}: {e}",
                self.path.display()
            ))),
        }
    }
}

impl Drop for DiffDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A private, exclusively-created temp dir for the two sides. Private because
/// the content is tenant configuration, and this lands in a world-readable
/// `/tmp`.
pub(crate) fn create_diff_dir() -> Result<std::path::PathBuf> {
    use std::os::unix::fs::DirBuilderExt;

    let dir = std::env::temp_dir().join(format!("aic-diff-{}", uuid::Uuid::new_v4()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|e| Error::Config(format!("create temp dir {}: {e}", dir.display())))?;
    Ok(dir)
}

pub(crate) fn write_diff_file(dir: &std::path::Path, name: &str, contents: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let path = dir.join(name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| Error::Config(format!("create temp file {}: {e}", path.display())))?;
    file.write_all(contents.as_bytes())
        .map_err(|e| Error::Config(format!("write temp file {}: {e}", path.display())))
}

/// Show `left` vs `right` as a real diff. `left`/`right` are side labels
/// (e.g. "tenant", "local", "snapshot") shown in the headers; `full` names the
/// subject. Requires `git` on PATH.
pub(crate) fn show_diff(
    full: &str,
    left_label: &str,
    left: &str,
    right_label: &str,
    right: &str,
) -> Result<()> {
    use std::process::Command;

    if left == right {
        println!("{full}: {left_label} and {right_label} are identical");
        return Ok(());
    }
    let guard = DiffDir::new()?;
    let dir = guard.path.clone();
    // `--no-prefix` makes the headers read `--- <name> (tenant)` etc.; `/` in
    // the full-name isn't path-safe, so swap it for `_`.
    let safe = full.replace('/', "_");
    let left_name = format!("{safe} ({left_label})");
    let right_name = format!("{safe} ({right_label})");
    let render = (|| -> Result<()> {
        for (name, contents) in [(&left_name, left), (&right_name, right)] {
            write_diff_file(&dir, name, contents)?;
        }
        // Run git *in* the temp dir with relative names so the diff headers read
        // `--- <name> (tenant)` rather than the full temp path.
        let status = Command::new("git")
            .current_dir(&dir)
            .args(["diff", "--no-index", "--no-prefix", "--"])
            .arg(&left_name)
            .arg(&right_name)
            .status()
            .map_err(|e| {
                Error::Config(format!(
                    "couldn't run `git` to render the diff ({e}) — is git on your PATH?"
                ))
            })?;
        // `git diff --no-index` exits 1 when the files differ.
        match status.code() {
            Some(0 | 1) => Ok(()),
            Some(code) => Err(Error::Config(format!(
                "`git diff --no-index` failed with exit code {code}"
            ))),
            None => Err(Error::Config(
                "`git diff --no-index` terminated by signal".into(),
            )),
        }
    })();
    let cleanup = guard.remove();
    match (render, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(e), Ok(())) => Err(e),
        (Ok(()), Err(e)) => Err(e),
        (Err(render), Err(cleanup)) => Err(Error::Config(format!("{render}; also {cleanup}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn diff_temp_files_are_private_and_exclusive() {
        let dir = create_diff_dir().unwrap();
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o077,
            0
        );

        write_diff_file(&dir, "left", "contents").unwrap();
        let file = dir.join("left");
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o077,
            0
        );
        assert!(write_diff_file(&dir, "left", "replacement").is_err());

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The guard is the reason a panic between creating the dir and the
    /// cleanup statement no longer leaves tenant configuration in `/tmp`.
    /// The discriminating case is the unwind: the plain statement it replaced
    /// would have been skipped entirely.
    #[test]
    fn the_temp_dir_is_removed_even_when_the_render_panics() {
        let path = {
            let guard = DiffDir::new().unwrap();
            let path = guard.path.clone();
            assert!(path.is_dir());
            let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = guard;
                panic!("render blew up");
            }));
            assert!(unwound.is_err());
            path
        };
        assert!(!path.exists(), "{} survived the unwind", path.display());
    }

    #[test]
    fn removing_the_guard_reports_its_own_failure() {
        let guard = DiffDir::new().unwrap();
        let path = guard.path.clone();
        std::fs::remove_dir_all(&path).unwrap();

        let error = guard.remove().unwrap_err();
        assert!(error.to_string().contains("remove temp dir"), "{error}");
    }

    /// A failed `remove` must not also disarm the backstop. The earlier
    /// version called `mem::forget` on every path, so a directory that was
    /// still on disk when removal failed was abandoned — the exact leak the
    /// guard was added to prevent.
    #[test]
    fn a_failed_remove_leaves_the_backstop_armed() {
        let outer = create_diff_dir().unwrap();
        let path = outer.join("inner");
        std::fs::DirBuilder::new().create(&path).unwrap();
        let guard = DiffDir { path: path.clone() };

        // Make removal fail without destroying the directory: a read-only
        // parent refuses the unlink.
        std::fs::set_permissions(&outer, std::fs::Permissions::from_mode(0o500)).unwrap();
        let outcome = guard.remove();
        std::fs::set_permissions(&outer, std::fs::Permissions::from_mode(0o700)).unwrap();

        // Only meaningful where the environment actually refuses it — as
        // root, it would not, and the assertion would be vacuous either way.
        if outcome.is_err() {
            assert!(path.is_dir(), "the failing remove destroyed it after all");
        }
        std::fs::remove_dir_all(&outer).unwrap();
    }
}
