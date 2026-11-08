//! File paths the operator types on the command line.
//!
//! `aic` moves the process into the project root before parsing (see
//! `cli::bootstrap_project_root`), so a relative path left to the OS would
//! resolve against the root rather than where the operator is — `--from
//! idm/endpoint/x.cjs`, typed from `workspace/sandbox/`, looked for
//! `<root>/idm/endpoint/x.cjs`. A [`UserPath`] resolves against the
//! directory the command behaves as if it started in: `--project <DIR>` (or
//! `AIC_PROJECT`) when given, which stands in for the working directory, and
//! the real working directory otherwise. An absolute path is kept as typed.
//!
//! Every CLI argument that names a file or directory of the operator's is a
//! `UserPath`; `cli`'s tests walk the whole command tree and refuse a plain
//! `PathBuf`, so a new flag cannot quietly resolve against the root.

use std::ffi::OsStr;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static USER_BASE: OnceLock<PathBuf> = OnceLock::new();

/// Record the directory relative paths resolve against. First call wins, so a
/// command already parsing cannot have its base moved.
pub fn set_user_base(dir: PathBuf) {
    let _ = USER_BASE.set(dir);
}

/// A path from the command line, made absolute against the start directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserPath(PathBuf);

impl UserPath {
    /// The pure half: `typed` resolved against `base`, or kept as typed when it
    /// is absolute or there is no base (outside a bootstrapped CLI run).
    pub fn resolve(base: Option<&Path>, typed: &Path) -> Self {
        match base {
            Some(base) if typed.is_relative() => Self(base.join(typed)),
            _ => Self(typed.to_path_buf()),
        }
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl std::str::FromStr for UserPath {
    type Err = std::convert::Infallible;

    fn from_str(typed: &str) -> Result<Self, Self::Err> {
        Ok(Self::resolve(
            USER_BASE.get().map(PathBuf::as_path),
            Path::new(typed),
        ))
    }
}

impl Deref for UserPath {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for UserPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<OsStr> for UserPath {
    fn as_ref(&self) -> &OsStr {
        self.0.as_os_str()
    }
}

/// Taken as already resolved: for code that builds an argument itself, and
/// for tests. Only clap's parse goes through the base.
impl From<PathBuf> for UserPath {
    fn from(path: PathBuf) -> Self {
        Self(path)
    }
}

impl From<&str> for UserPath {
    fn from(path: &str) -> Self {
        Self(PathBuf::from(path))
    }
}

impl PartialEq<PathBuf> for UserPath {
    fn eq(&self, other: &PathBuf) -> bool {
        &self.0 == other
    }
}

impl From<UserPath> for PathBuf {
    fn from(path: UserPath) -> Self {
        path.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_path_resolves_against_the_base_and_an_absolute_one_is_kept() {
        let base = Path::new("/work/project/workspace/sandbox");
        for (typed, want) in [
            (
                "idm/endpoint/x.cjs",
                "/work/project/workspace/sandbox/idm/endpoint/x.cjs",
            ),
            (
                "../other.json",
                "/work/project/workspace/sandbox/../other.json",
            ),
            ("/abs/x.cjs", "/abs/x.cjs"),
        ] {
            assert_eq!(
                UserPath::resolve(Some(base), Path::new(typed)).into_path_buf(),
                PathBuf::from(want),
                "{typed}"
            );
        }
        // Outside a bootstrapped run there is no base, and nothing is invented.
        assert_eq!(
            UserPath::resolve(None, Path::new("x.cjs")).into_path_buf(),
            PathBuf::from("x.cjs")
        );
    }
}
