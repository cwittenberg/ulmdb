//! Exclusive writer OFD ownership, independent of incidental fork inheritance.
use std::{
    fs::{File, TryLockError},
    ops::Deref,
};

/// Retain this guard (or an `Arc` of it), rather than a duplicated raw File,
/// for every capability that can still write. Read-only owners open independent
/// OFDs. After fork, inherited Environments are not supported for further writes.
/// A foreign child's destructor only closes its copy; it cannot unlock the parent.
#[derive(Debug)]
pub struct WriterFence {
    file: File,
    creator_pid: u32,
}
impl WriterFence {
    pub fn try_lock(file: File) -> Result<Self, TryLockError> {
        #[cfg(not(windows))]
        file.try_lock()?;
        #[cfg(windows)]
        ulmdb_platform::lock_file_range(
            &file,
            ulmdb_platform::LockMode::Exclusive,
            1u64 << 62,
            1,
            false,
        )
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                TryLockError::WouldBlock
            } else {
                TryLockError::Error(e)
            }
        })?;
        Ok(Self {
            file,
            creator_pid: std::process::id(),
        })
    }
    pub(crate) fn check_directory(&self, directory: &std::path::Path) -> crate::Result<()> {
        if self.creator_pid != std::process::id() {
            return Err(crate::Error::InvalidInput("inherited writer fence"));
        }
        let named = ulmdb_platform::path_identity(&directory.join("LOCK"))?;
        let held = ulmdb_platform::file_identity(&self.file)?;
        if !named.is_file() || !named.same_object(&held) {
            return Err(crate::Error::Corrupt("writer fence namespace mismatch"));
        }
        Ok(())
    }
    #[cfg(test)]
    #[allow(dead_code)] // Shared with packed, whose tests need no fake IO descriptor.
    pub(crate) fn unlocked_for_fault(file: File) -> Self {
        Self {
            file,
            creator_pid: 0,
        }
    }
}
impl Deref for WriterFence {
    type Target = File;
    fn deref(&self) -> &File {
        &self.file
    }
}
impl Drop for WriterFence {
    fn drop(&mut self) {
        if self.creator_pid == std::process::id() {
            // Failure leaves the original close behavior and remains fail-closed
            // if an inherited FD still holds this OFD. Never panic from Drop.
            #[cfg(not(windows))]
            let _ = self.file.unlock();
            #[cfg(windows)]
            let _ = ulmdb_platform::unlock_file_range(&self.file, 1u64 << 62, 1);
        }
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos", windows)))]
mod tests {
    use super::*;
    use ulmdb_platform::FileExt;
    #[test]
    fn writer_and_retirement_authorities_are_distinct_from_data_io() {
        #[cfg(target_os = "linux")]
        const NAME: &str =
            "file_fence::tests::writer_and_retirement_authorities_are_distinct_from_data_io";
        #[cfg(target_os = "linux")]
        if crate::history::tests::run_isolated(NAME) {
            return;
        }
        let path =
            std::env::temp_dir().join(format!("writer-lease-{}", crate::new_identity().unwrap()));
        let f = ulmdb_platform::create_private_file(&path, true).unwrap();
        let writer = WriterFence::try_lock(f).unwrap();
        writer.write_all_at(b"header", 0).unwrap();
        let anchor = File::open(&path).unwrap();
        crate::hybrid::retirement::shared(&anchor).unwrap();
        let rival = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert!(matches!(
            WriterFence::try_lock(rival),
            Err(TryLockError::WouldBlock)
        ));
        assert!(matches!(
            crate::hybrid::retirement::ExclusiveLease::try_acquire(&path, &writer),
            Err(crate::Error::Busy)
        ));
        let mut bytes = [0; 6];
        anchor.read_exact_at(&mut bytes, 0).unwrap();
        assert_eq!(&bytes, b"header");
        drop(anchor);
        let retirement =
            crate::hybrid::retirement::ExclusiveLease::try_acquire(&path, &writer).unwrap();
        writer.write_all_at(b"safe", 16).unwrap();
        drop(retirement);
        drop(writer);
        std::fs::remove_file(path).unwrap();
        #[cfg(target_os = "linux")]
        crate::history::tests::worker_completed(NAME);
    }
}
