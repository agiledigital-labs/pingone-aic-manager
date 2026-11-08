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
//!
//! clap reaches it through [`ValueParserFactory`], which takes the argument as
//! an `OsStr`: a Unix path need not be UTF-8, and a `From<&str>` impl would
//! otherwise outrank `FromStr` in clap's choice of parser and skip the base.

use clap::builder::{TypedValueParser, ValueParserFactory};
use std::ffi::OsStr;
use std::ops::Deref;
use std::path::{Path, PathBuf};

// The base is process-wide in the binary. Under test it is per thread, so a
// test can parse against a base of its own without moving every other test's.
#[cfg(not(test))]
static USER_BASE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
#[cfg(test)]
thread_local! {
    static USER_BASE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Record the directory relative paths resolve against. First call wins, so a
/// command already parsing cannot have its base moved.
pub fn set_user_base(dir: PathBuf) {
    #[cfg(not(test))]
    let _ = USER_BASE.set(dir);
    #[cfg(test)]
    USER_BASE.with(|base| {
        base.borrow_mut().get_or_insert(dir);
    });
}

fn user_base() -> Option<PathBuf> {
    #[cfg(not(test))]
    return USER_BASE.get().cloned();
    #[cfg(test)]
    return USER_BASE.with(|base| base.borrow().clone());
}

/// Forget this thread's base, so a test that set one leaves none behind.
#[cfg(test)]
pub(crate) fn clear_user_base() {
    USER_BASE.with(|base| *base.borrow_mut() = None);
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

/// clap's parser for [`UserPath`]: the raw argument, resolved against the base.
#[derive(Clone, Copy, Debug)]
pub struct UserPathParser;

impl TypedValueParser for UserPathParser {
    type Value = UserPath;

    fn parse_ref(
        &self,
        _cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        typed: &OsStr,
    ) -> Result<UserPath, clap::Error> {
        Ok(UserPath::resolve(user_base().as_deref(), Path::new(typed)))
    }
}

impl ValueParserFactory for UserPath {
    type Parser = UserPathParser;

    fn value_parser() -> UserPathParser {
        UserPathParser
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

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_path_parses_and_keeps_its_bytes() {
        use clap::CommandFactory;
        use std::os::unix::ffi::OsStrExt;

        let typed = OsStr::from_bytes(b"caf\xe9.json");
        let parsed = UserPathParser
            .parse_ref(&crate::cli::Cli::command(), None, typed)
            .expect("a non-UTF-8 path is a path");
        assert_eq!(parsed.as_os_str().as_bytes(), typed.as_bytes());
    }
}
