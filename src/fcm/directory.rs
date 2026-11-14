//! Descriptor-relative IO: no path lookup beneath an opened directory follows
//! a symlink. Keep directory handles through creation, reads and replacement.
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

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
        let absolute = std::path::absolute(path)?;
        let mut dir = Self(File::open("/")?);
        for component in absolute.components() {
            match component {
                Component::RootDir | Component::CurDir => continue,
                Component::Normal(part) => match dir.child(Path::new(part), create) {
                    Ok(child) => dir = child,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                    Err(error) => return Err(error),
                },
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "invalid config directory",
                    ));
                }
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

    pub(super) fn replace(&self, leaf: &Path, bytes: &[u8]) -> io::Result<()> {
        let temporary = format!(".aic-export-{}", uuid::Uuid::new_v4());
        self.create_new(Path::new(&temporary), bytes)?;
        let src = name(Path::new(&temporary))?;
        let dst = name(leaf)?;
        // SAFETY: both names are NUL-terminated and relative to this live fd.
        let result = unsafe {
            libc::renameat(
                self.0.as_raw_fd(),
                src.as_ptr(),
                self.0.as_raw_fd(),
                dst.as_ptr(),
            )
        };
        if result < 0 {
            let error = io::Error::last_os_error();
            unsafe {
                libc::unlinkat(self.0.as_raw_fd(), src.as_ptr(), 0);
            }
            return Err(error);
        }
        Ok(())
    }
}
