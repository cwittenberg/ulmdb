//! Genuine Windows publication serialization; no no-op History authority.
use crate::{Error, Result};
use std::os::windows::fs::OpenOptionsExt;
use std::{
    fs::File,
    path::{Path, PathBuf},
};
pub(super) struct Gate {
    file: File,
    directory: PathBuf,
    directory_file: File,
    area: PathBuf,
    area_file: File,
    creator: u32,
}
impl Gate {
    pub(super) fn acquire(path: &Path, shared: bool) -> Result<Self> {
        let directory_file = ulmdb_platform::open_directory(path)?;
        let directory = path.canonicalize()?;
        let area = directory.join(".ulmdb-coordination");
        match ulmdb_platform::create_private_directory(&area) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        let area_file = ulmdb_platform::open_directory(&area)?;
        let lock_path = area.join("history");
        let file = match ulmdb_platform::create_private_file(&lock_path, true) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS)
                .open(&lock_path)?,
            Err(e) => return Err(e.into()),
        };
        let gate = Self {
            file,
            directory,
            directory_file,
            area,
            area_file,
            creator: std::process::id(),
        };
        gate.check(path)?;
        // Rust's Windows File lock invokes real LockFileEx. This dedicated,
        // zero-length inode has no user IO to conflict with mandatory locks.
        loop {
            let result = if shared {
                gate.file.lock_shared()
            } else {
                gate.file.lock()
            };
            match result {
                Ok(()) => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
        gate.check(path)?;
        Ok(gate)
    }
    pub(super) fn check(&self, path: &Path) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited Windows history gate"));
        }
        if path.canonicalize()? != self.directory {
            return Err(Error::Corrupt("history directory binding"));
        }
        check(&self.directory, &self.directory_file, true)?;
        check(&self.area, &self.area_file, true)?;
        check(&self.area.join("history"), &self.file, false)
    }
}
fn check(path: &Path, file: &File, directory: bool) -> Result<()> {
    let named = ulmdb_platform::path_identity(path)?;
    let owned = ulmdb_platform::file_identity(file)?;
    if !named.same_object(&owned)
        || if directory {
            !named.is_dir() || !owned.is_dir()
        } else {
            !named.is_file() || !owned.is_file() || owned.links != 1 || owned.length != 0
        }
    {
        return Err(Error::Corrupt("invalid Windows history gate object"));
    }
    Ok(())
}
impl Drop for Gate {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            let _ = self.file.unlock();
        }
    }
}
