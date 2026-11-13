//! Scaffold's filesystem boundary. Preview retains final bytes and virtual
//! removals; apply executes immediately to preserve partial-failure ordering.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::Result;

/// A net byte change; identical files and directory operations are omitted.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FileChange {
    Create,
    Change,
    Remove,
}

pub(super) enum Mutations {
    Apply,
    Preview {
        files: BTreeMap<PathBuf, Option<Vec<u8>>>,
        removed_dirs: BTreeSet<PathBuf>,
    },
}

impl Mutations {
    pub(super) fn preview() -> Self {
        Self::Preview {
            files: BTreeMap::new(),
            removed_dirs: BTreeSet::new(),
        }
    }

    pub(super) fn create_dir_all(&mut self, path: &Path) -> Result<()> {
        if matches!(self, Self::Apply) {
            std::fs::create_dir_all(path)?;
        }
        Ok(())
    }

    pub(super) fn write(&mut self, path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
        match self {
            Self::Apply => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(path, contents)?;
            }
            Self::Preview { files, .. } => {
                files.insert(path.to_path_buf(), Some(contents.as_ref().to_vec()));
            }
        }
        Ok(())
    }

    pub(super) fn remove_file(&mut self, path: &Path) -> Result<()> {
        match self {
            Self::Apply => std::fs::remove_file(path)?,
            Self::Preview { files, .. } => {
                files.insert(path.to_path_buf(), None);
            }
        }
        Ok(())
    }

    pub(super) fn remove_dir(&mut self, path: &Path) -> Result<()> {
        match self {
            Self::Apply => std::fs::remove_dir(path)?,
            Self::Preview { removed_dirs, .. } => {
                removed_dirs.insert(path.to_path_buf());
            }
        }
        Ok(())
    }

    pub(super) fn exists(&self, path: &Path) -> Result<bool> {
        if let Self::Preview {
            files,
            removed_dirs,
        } = self
        {
            if removed_dirs.contains(path) {
                return Ok(false);
            }
            if let Some(bytes) = files.get(path) {
                return Ok(bytes.is_some());
            }
        }
        Ok(path.try_exists()?)
    }

    /// Missing directories are empty; unreadable or partial listings fail.
    /// Entries retain their non-following file_type for the scaffold walkers.
    pub(super) fn read_dir(&self, path: &Path) -> Result<Vec<std::fs::DirEntry>> {
        let entries = match std::fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut visible = Vec::new();
        for entry in entries {
            let entry = entry?;
            let removed = match self {
                Self::Apply => false,
                Self::Preview {
                    files,
                    removed_dirs,
                } => {
                    removed_dirs.contains(&entry.path())
                        || matches!(files.get(&entry.path()), Some(None))
                }
            };
            if !removed {
                visible.push(entry);
            }
        }
        Ok(visible)
    }

    pub(super) fn changes(&self, tree: &Path) -> Result<BTreeMap<PathBuf, FileChange>> {
        let mut changes = BTreeMap::new();
        if let Self::Preview { files, .. } = self {
            for (path, wanted) in files {
                let existing = match std::fs::read(path) {
                    Ok(bytes) => Some(bytes),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => return Err(error.into()),
                };
                let change = match (&existing, wanted) {
                    (None, Some(_)) => FileChange::Create,
                    (Some(_), None) => FileChange::Remove,
                    (Some(old), Some(new)) if old != new => FileChange::Change,
                    _ => continue,
                };
                changes.insert(
                    path.strip_prefix(tree).unwrap_or(path).to_path_buf(),
                    change,
                );
            }
        }
        Ok(changes)
    }
}
