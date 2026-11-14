//! Descriptor-relative IO: no path lookup beneath an opened directory follows
//! a symlink. Keep directory handles through creation, reads and replacement.
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

pub(super) struct Directory(File);

fn name(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"))
}

impl Directory {
    fn open(&self, path: &Path, flags: i32) -> io::Result<File> {
        let name = name(path)?;
        // SAFETY: name is NUL-terminated, the directory fd stays alive, and a
        // successful open returns a new owned fd.
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }

    fn child(&self, path: &Path, create: bool) -> io::Result<Self> {
        if create {
            let name = name(path)?;
            // SAFETY: valid fd and NUL-terminated single component.
            let result = unsafe { libc::mkdirat(self.0.as_raw_fd(), name.as_ptr(), 0o755) };
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(error);
                }
            }
        }
        self.open(path, libc::O_RDONLY | libc::O_DIRECTORY)
            .map(Self)
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!(
                        "refusing directory {} (symlink or inaccessible): {error}",
                        path.display()
                    ),
                )
            })
    }

    pub(super) fn root(path: &Path, create: bool) -> io::Result<Option<Self>> {
        // Resolve the operator's path with filesystem semantics (including
        // symlinks and ..), then create only its missing suffix through fds.
        let mut ancestor = std::path::absolute(path)?;
        let mut missing = Vec::new();
        let resolved = loop {
            match ancestor.canonicalize() {
                Ok(path) => break path,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    let part = ancestor.components().next_back().ok_or(error)?;
                    missing.push(part.as_os_str().to_owned());
                    ancestor.pop();
                }
                Err(error) => return Err(error),
            }
        };
        let mut dir = Self(File::open(resolved)?);
        for part in missing.into_iter().rev() {
            match dir.child(Path::new(&part), create) {
                Ok(child) => dir = child,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error),
            }
        }
        Ok(Some(dir))
    }

    pub(super) fn parent(&self, relative: &Path, create: bool) -> io::Result<Option<Self>> {
        let mut dir = Self(self.0.try_clone()?);
        for part in relative.parent().unwrap_or(Path::new("")).components() {
            let Component::Normal(part) = part else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid export path",
                ));
            };
            match dir.child(Path::new(part), create) {
                Ok(child) => dir = child,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error),
            }
        }
        Ok(Some(dir))
    }

    pub(super) fn read(&self, leaf: &Path) -> io::Result<Vec<u8>> {
        let mut file = self.open(leaf, libc::O_RDONLY | libc::O_NONBLOCK)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "export target is not a regular file",
            ));
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    pub(super) fn create_new(&self, leaf: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut file = self.open(leaf, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?;
        file.write_all(bytes)
    }

    pub(super) fn stage(&self, bytes: &[u8]) -> io::Result<Staged<'_>> {
        let temporary = PathBuf::from(format!(".aic-export-{}", uuid::Uuid::new_v4()));
        let mut file = self.open(&temporary, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?;
        let staged = Staged {
            parent: self,
            temporary,
        };
        file.write_all(bytes)?;
        Ok(staged)
    }
}

pub(super) struct Staged<'a> {
    parent: &'a Directory,
    temporary: PathBuf,
}

impl Staged<'_> {
    pub(super) fn install(&self, leaf: &Path, expected: Option<&[u8]>) -> io::Result<()> {
        let current = match self.parent.read(leaf) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        if current.as_deref() != expected {
            return Err(io::Error::other("export target changed before install"));
        }
        #[cfg(test)]
        super::replacement_hook("before rename");
        let src = name(&self.temporary)?;
        let dst = name(leaf)?;
        let fd = self.parent.0.as_raw_fd();
        let result = if expected.is_none() {
            // Neither install path can replace a concurrently created destination.
            #[cfg(target_os = "linux")]
            {
                // SAFETY: names are NUL-terminated and fd remains alive.
                unsafe {
                    libc::renameat2(fd, src.as_ptr(), fd, dst.as_ptr(), libc::RENAME_NOREPLACE)
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                return self.install_new_link(leaf);
            }
        } else {
            // SAFETY: names are NUL-terminated and fd remains alive. The bytes
            // were compared after staging and backup, immediately above.
            unsafe { libc::renameat(fd, src.as_ptr(), fd, dst.as_ptr()) }
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(any(not(target_os = "linux"), test))]
    fn install_new_link(&self, leaf: &Path) -> io::Result<()> {
        let src = name(&self.temporary)?;
        let dst = name(leaf)?;
        let fd = self.parent.0.as_raw_fd();
        // SAFETY: names are NUL-terminated under the live directory fd. With
        // flags=0 linkat installs this staged file without following symlinks
        // or replacing an existing name (including a dangling symlink).
        if unsafe { libc::linkat(fd, src.as_ptr(), fd, dst.as_ptr(), 0) } < 0 {
            return Err(io::Error::last_os_error());
        }
        // The destination is now installed. Drop retries temporary cleanup if
        // unlink fails; cleanup must not report the installed file as absent.
        unsafe {
            libc::unlinkat(fd, src.as_ptr(), 0);
        }
        Ok(())
    }
}

impl Drop for Staged<'_> {
    fn drop(&mut self) {
        if let Ok(name) = name(&self.temporary) {
            // SAFETY: unlink only our unique staging name under its live fd.
            unsafe {
                libc::unlinkat(self.parent.0.as_raw_fd(), name.as_ptr(), 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_link_install_is_atomic_no_replace_and_cleans_staging() {
        let root = std::env::temp_dir().join(format!("aic-link-install-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let dir = Directory::root(&root, false).unwrap().unwrap();
        let staged = dir.stage(b"new").unwrap();
        staged.install_new_link(Path::new("new.json")).unwrap();
        assert_eq!(dir.read(Path::new("new.json")).unwrap(), b"new");
        assert!(!root.join(&staged.temporary).exists());
        drop(staged);

        std::fs::write(root.join("existing.json"), b"old").unwrap();
        std::os::unix::fs::symlink("missing", root.join("symlink.json")).unwrap();
        std::fs::create_dir(root.join("directory.json")).unwrap();
        for leaf in ["existing.json", "symlink.json", "directory.json"] {
            let staged = dir.stage(b"replacement").unwrap();
            assert_eq!(
                staged.install_new_link(Path::new(leaf)).unwrap_err().kind(),
                io::ErrorKind::AlreadyExists
            );
            assert!(root.join(&staged.temporary).exists());
            drop(staged);
        }
        assert_eq!(dir.read(Path::new("existing.json")).unwrap(), b"old");
        assert_eq!(
            std::fs::read_link(root.join("symlink.json")).unwrap(),
            Path::new("missing")
        );
        assert!(root.join("directory.json").is_dir());
        assert!(!std::fs::read_dir(&root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".aic-export-")
        }));
        std::fs::remove_dir_all(root).unwrap();
    }
}
