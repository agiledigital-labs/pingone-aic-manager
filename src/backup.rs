//! Collision-safe backups for workspace files replaced by pull operations.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::Result;

pub(crate) fn create_in(
    dir: &Path,
    kind: &str,
    realm: &str,
    name: &str,
    extension: &str,
    bytes: &[u8],
) -> Result<PathBuf> {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    create_at(dir, kind, realm, name, extension, bytes, &stamp)
}

fn component(value: &str) -> String {
    let encoded: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if encoded.is_empty() {
        "_".into()
    } else {
        encoded
    }
}

/// Ignore every backup file. `!.gitignore` keeps the ignore rule itself
/// trackable so a later `git add -A` cannot pick the backups up.
const GITIGNORE: &str = "*\n!.gitignore\n";

fn ensure_untracked(dir: &Path) -> Result<()> {
    let path = dir.join(".gitignore");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    match options.open(&path) {
        Ok(mut file) => {
            file.write_all(GITIGNORE.as_bytes())?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn create_at(
    dir: &Path,
    kind: &str,
    realm: &str,
    name: &str,
    extension: &str,
    bytes: &[u8],
    stamp: &str,
) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    ensure_untracked(dir)?;
    let identity = format!(
        "{}.{}.{}",
        component(kind),
        component(realm),
        component(name)
    );
    loop {
        let path = dir.join(format!(
            "{identity}.{stamp}.{}.{}",
            uuid::Uuid::new_v4(),
            component(extension)
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(mut file) => {
                file.write_all(bytes)?;
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_second_backups_are_exclusive_and_keep_resource_identity() {
        let dir = std::env::temp_dir().join(format!("aic-backup-{}", uuid::Uuid::new_v4()));
        let stamp = "20260911T120000Z";
        let first = create_at(
            &dir,
            "policy-set",
            "alpha",
            "Same/Name",
            "json",
            b"first",
            stamp,
        )
        .unwrap();
        let second = create_at(
            &dir,
            "policy-set",
            "alpha",
            "Same/Name",
            "json",
            b"second",
            stamp,
        )
        .unwrap();

        assert_ne!(first, second);
        assert_eq!(std::fs::read(&first).unwrap(), b"first");
        assert_eq!(std::fs::read(&second).unwrap(), b"second");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&first).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        for path in [&first, &second] {
            assert!(
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("policy-set.alpha.Same_Name.20260911T120000Z.")
            );
        }

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn create_in_leaves_a_gitignore_that_ignores_everything() {
        let dir = std::env::temp_dir().join(format!("aic-backup-gi-{}", uuid::Uuid::new_v4()));
        create_in(&dir, "oauth", "alpha", "client", "json", b"secret").unwrap();
        let gitignore = dir.join(".gitignore");
        let contents = std::fs::read_to_string(&gitignore).unwrap();
        assert_eq!(contents, "*\n!.gitignore\n");

        std::fs::write(&gitignore, "do-not-touch\n").unwrap();
        create_in(&dir, "oauth", "alpha", "other", "json", b"also").unwrap();
        assert_eq!(
            std::fs::read_to_string(&gitignore).unwrap(),
            "do-not-touch\n"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }
}
