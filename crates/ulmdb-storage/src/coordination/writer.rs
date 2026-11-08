//! Shared native/C writer ownership, including independently linked Rust copies.
//!
//! `LMDB-WRITER` is a stable flock inode. Its checked owner record identifies
//! the process and native kernel thread before an acquisition returns. A caller
//! callback can therefore never recursively wait for its own lock, even when a
//! second Rust runtime does not share its TLS. Old unrecorded binaries on that
//! same thread are outside this protocol and must be drained before mixed use.
//! This is volatile coordination metadata, not a data durability/publication
//! record. Cooperating participants never replace or directly edit lease paths.

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "writer_record.rs"]
mod writer_record;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
    use crate::{Error, Result};
    use std::{
        fs::{self, File, OpenOptions, TryLockError},
        marker::PhantomData,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
        path::Path,
        rc::Rc,
    };
    use ulmdb_platform::FileExt;

    use super::writer_record::{Owner, RECORD_BYTES};

    #[allow(unsafe_code)]
    fn current_owner() -> Result<Owner> {
        #[cfg(target_os = "linux")]
        let tid = {
            // SAFETY: this no-argument syscall identifies the current OS thread,
            // independently of any separately linked Rust runtime's TLS.
            let tid = unsafe { libc::gettid() };
            u64::try_from(tid).map_err(|_| Error::Corrupt("invalid kernel thread ID"))?
        };
        #[cfg(target_os = "macos")]
        let tid = {
            let mut tid = 0u64;
            // SAFETY: a null pthread selects the current thread, and the valid
            // out-pointer receives Darwin's full-width kernel thread identity.
            let error = unsafe { libc::pthread_threadid_np(0 as libc::pthread_t, &mut tid) };
            if error != 0 {
                return Err(std::io::Error::from_raw_os_error(error).into());
            }
            tid
        };
        if tid == 0 {
            return Err(Error::Corrupt("invalid kernel thread ID"));
        }
        Ok(Owner {
            pid: std::process::id(),
            tid,
        })
    }
    fn owner(file: &File) -> Result<Option<Owner>> {
        let mut bytes = [0; RECORD_BYTES + 1];
        let length = loop {
            match file.read_at(&mut bytes, 0) {
                Ok(length) => break length,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        };
        // Publication starts by truncating; release clears EOF before unlock.
        // An empty/short locked record is therefore an acquisition/release
        // transition before/after caller code. A complete malformed record is
        // corruption, never permission to wait for an unidentified owner.
        if length < RECORD_BYTES {
            return Ok(None);
        }
        super::writer_record::decode(&bytes[..length], super::writer_record::CURRENT).map(Some)
    }
    fn same_inode(left: &fs::Metadata, right: &fs::Metadata) -> bool {
        left.dev() == right.dev() && left.ino() == right.ino()
    }
    fn validate(directory: &Path, directory_file: &File, file: &File) -> Result<()> {
        let selected_directory = fs::symlink_metadata(directory)?;
        if !selected_directory.is_dir()
            || selected_directory.file_type().is_symlink()
            || !same_inode(&selected_directory, &directory_file.metadata()?)
        {
            return Err(Error::Corrupt("coordinated writer directory was replaced"));
        }
        let selected = fs::symlink_metadata(directory.join("LMDB-WRITER"))?;
        let opened = file.metadata()?;
        if !selected.is_file()
            || selected.file_type().is_symlink()
            || !opened.is_file()
            || selected.nlink() != 1
            || opened.nlink() != 1
            || !same_inode(&selected, &opened)
        {
            return Err(Error::Corrupt("invalid coordinated writer lease inode"));
        }
        if opened.len() > RECORD_BYTES as u64 {
            return Err(Error::Corrupt("oversized coordinated writer owner record"));
        }
        Ok(())
    }

    /// An exclusive writer lease shared by native callers and the C adapter.
    /// Other threads/processes wait; recursive acquisition on the same native
    /// kernel thread returns Busy. Retain it through native owner destruction.
    pub struct WriterLease {
        file: File,
        _directory: File,
        creator: u32,
        clear: bool,
        // Like the native writer's MutexGuard, ownership ends on its thread.
        _thread: PhantomData<Rc<()>>,
    }
    impl WriterLease {
        pub fn acquire(directory: impl AsRef<Path>) -> Result<Self> {
            Self::acquire_inner(directory.as_ref(), true)
        }
        /// Admit only an idle writer; never wait while holding facade setup.
        pub fn try_acquire(directory: impl AsRef<Path>) -> Result<Self> {
            Self::acquire_inner(directory.as_ref(), false)
        }
        fn acquire_inner(directory: &Path, wait: bool) -> Result<Self> {
            let directory_file = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(directory)?;
            let file = crate::access::options_for_entry(&directory.join("LMDB-WRITER"))?
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(directory.join("LMDB-WRITER"))?;
            crate::access::validate_entry(&directory.join("LMDB-WRITER"), &file)?;
            validate(directory, &directory_file, &file)?;
            let current = current_owner()?;
            let attempted = loop {
                match file.try_lock() {
                    Err(TryLockError::Error(error))
                        if error.kind() == std::io::ErrorKind::Interrupted => {}
                    result => break result,
                }
            };
            match attempted {
                Ok(()) => {}
                Err(TryLockError::WouldBlock) => {
                    if owner(&file)? == Some(current) || !wait {
                        return Err(Error::Busy);
                    }
                    loop {
                        match file.lock() {
                            Ok(()) => break,
                            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                            Err(error) => return Err(error.into()),
                        }
                    }
                }
                Err(TryLockError::Error(error)) => return Err(error.into()),
            }
            // From here every error path explicitly releases the creator's OFD,
            // including when another process inherited an incidental descriptor.
            let mut lease = Self {
                file,
                _directory: directory_file,
                creator: current.pid,
                clear: false,
                _thread: PhantomData,
            };
            validate(directory, &lease._directory, &lease.file)?;
            lease.clear = true;
            // A successful kernel lock supersedes stale or crash-partial state.
            // Complete the new record before any caller can enter its callback.
            lease.file.set_len(0)?;
            lease.file.write_all_at(
                &super::writer_record::encode(current, super::writer_record::CURRENT)?,
                0,
            )?;
            Ok(lease)
        }
    }
    impl Drop for WriterLease {
        fn drop(&mut self) {
            if self.creator == std::process::id() && (!self.clear || self.file.set_len(0).is_ok()) {
                let _ = self.file.unlock();
            }
            // Foreign fork children only close their inherited descriptors.
            // If clearing fails, close rather than explicitly unlock: retaining
            // an inherited OFD is fail-closed until that foreign owner exits.
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use unix::WriterLease;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub struct WriterLease;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
impl WriterLease {
    pub fn try_acquire(path: impl AsRef<std::path::Path>) -> crate::Result<Self> {
        Self::acquire(path)
    }
    pub fn acquire(_: impl AsRef<std::path::Path>) -> crate::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "coordinated writer leases are unavailable on this platform",
        )
        .into())
    }
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::WriterLease;
