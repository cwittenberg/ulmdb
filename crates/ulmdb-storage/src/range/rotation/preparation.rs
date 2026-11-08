//! Created-inode authority for one unpublished rotation. Never scans filenames.
use super::*;
use std::sync::Arc;

struct Created {
    path: PathBuf,
    file: File,
}
/// Owns only files actually returned by this preparation's create_new calls.
/// The pack's original descriptor is retained across writer construction so
/// even the first header/lock/admission failure has exact deletion authority.
pub(super) struct Preparation {
    namespace: Arc<super::super::checkpoint::Namespace>,
    directory: PathBuf,
    creator: u32,
    created: Vec<Created>,
    cleanup: bool,
}
impl Preparation {
    pub(super) fn create(
        directory: &Path,
        id: [u8; 16],
        namespace: Arc<super::super::checkpoint::Namespace>,
    ) -> Result<Self> {
        // All path/vector allocations precede the first creation.
        let mut this = Self {
            namespace,
            directory: directory.to_path_buf(),
            creator: std::process::id(),
            created: Vec::with_capacity(3),
            cleanup: true,
        };
        let path = named(directory, id, "pack");
        this.create_owned(path)?;
        Ok(this)
    }
    fn check(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("foreign Range preparation"));
        }
        if !self.namespace.matches(&self.directory)? {
            return Err(Error::StaleHandle);
        }
        Ok(())
    }
    fn create_owned(&mut self, path: PathBuf) -> Result<usize> {
        self.check()?;
        if self.created.len() == self.created.capacity() {
            return Err(Error::LimitExceeded);
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path)?;
        // The reserved vector push is infallible; no operation can lose the
        // newly created File before it becomes cleanup-owned.
        self.created.push(Created { path, file });
        Ok(self.created.len() - 1)
    }
    pub(super) fn writer_file(&self) -> Result<File> {
        self.check()?;
        Ok(self.created[0].file.try_clone()?)
    }
    pub(super) fn archive_global(&mut self, id: [u8; 16], bytes: &[u8; pack::BLOCK]) -> Result<()> {
        let path = named(&self.directory, id, "anchor");
        match self.create_owned(path.clone()) {
            Ok(index) => {
                let file = &mut self.created[index].file;
                file.write_all(bytes)?;
                ulmdb_platform::sync_file(file)?;
                Ok(())
            }
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Existing certificates never acquire ownership from their name
                // or matching contents; a later failure cannot delete them.
                let file = crate::read_mode::ReadMode::Sync.open(&path)?;
                if file.metadata()?.len() != pack::BLOCK as u64 {
                    return Err(Error::Corrupt("range anchor file length"));
                }
                let mut old = [0; pack::BLOCK];
                pack::io::read_exact_at(&file, &mut old, 0)?;
                if old != *bytes {
                    return Err(Error::Corrupt("range anchor collision"));
                }
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
    /// Called immediately before the selector rename is attempted. Errors from
    /// that point must preserve every asset: publication may have occurred.
    pub(super) fn selector_attempted(&mut self) {
        self.cleanup = false;
    }
    pub(super) fn cleanup(&mut self) -> Result<()> {
        if !self.cleanup {
            return Ok(());
        }
        self.check()?;
        for created in self.created.iter().rev() {
            let expected = ulmdb_platform::file_identity(&created.file)?;
            let named = match ulmdb_platform::path_identity(&created.path) {
                Ok(id) => id,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            if !expected.is_file() || !named.is_file() || !expected.same_object(&named) {
                return Err(Error::Corrupt("Range preparation path replaced"));
            }
            // Recheck namespace immediately before each owned unlink.
            self.check()?;
            fs::remove_file(&created.path)?;
        }
        ulmdb_platform::sync_directory(&self.directory)?;
        self.cleanup = false;
        Ok(())
    }
}
impl Drop for Preparation {
    fn drop(&mut self) {
        // Never inspect inherited mutable state or unlink through a foreign
        // owner. Fields only close/deallocate; no explicit kernel unlock occurs.
        if self.creator == std::process::id() {
            let _ = self.cleanup();
        }
    }
}
