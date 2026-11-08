//! Shared Windows native/C writer ownership with cross-runtime OS thread recursion detection.
use crate::{Error, Result};
use std::{fs::File, marker::PhantomData, path::Path, rc::Rc};
use ulmdb_platform::FileExt;

const RECORD_BYTES: usize = 32;
const MAGIC: &[u8; 8] = b"ULMWR001";

#[derive(Clone, Copy, PartialEq, Eq)]
struct Owner {
    pid: u32,
    tid: u32,
}
impl Owner {
    fn current() -> Result<Self> {
        let tid = ulmdb_platform::current_thread_id();
        if tid == 0 {
            return Err(Error::Corrupt("invalid Windows thread ID"));
        }
        Ok(Self {
            pid: std::process::id(),
            tid,
        })
    }
    fn encode(self) -> [u8; RECORD_BYTES] {
        let mut bytes = [0; RECORD_BYTES];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.pid.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.tid.to_le_bytes());
        // 20..28 are reserved, checked zero on decode.
        let checksum = crc(&bytes[..28]);
        bytes[28..].copy_from_slice(&checksum.to_le_bytes());
        bytes
    }
}
fn crc(bytes: &[u8]) -> u32 {
    let mut value = !0u32;
    for byte in bytes {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            value = (value >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(value & 1)));
        }
    }
    !value
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
    if length != RECORD_BYTES
        || &bytes[..8] != MAGIC
        || bytes[8..12] != 1u32.to_le_bytes()
        || bytes[20..28] != [0; 8]
        || bytes[28..32] != crc(&bytes[..28]).to_le_bytes()
    {
        return Err(Error::Corrupt("invalid coordinated writer owner record"));
    }
    let pid = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    let tid = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
    if pid == 0 || tid == 0 {
        return Err(Error::Corrupt("invalid coordinated writer owner identity"));
    }
    Ok(Some(Owner { pid, tid }))
}
fn validate(directory: &Path, directory_file: &File, file: &File) -> Result<()> {
    let selected = ulmdb_platform::path_identity(directory)?;
    let owned = ulmdb_platform::file_identity(directory_file)?;
    if !selected.is_dir() || !selected.same_object(&owned) {
        return Err(Error::Corrupt("coordinated writer directory was replaced"));
    }
    let named = ulmdb_platform::path_identity(&directory.join("LMDB-WRITER"))?;
    let owned = ulmdb_platform::file_identity(file)?;
    if !named.is_file()
        || !owned.is_file()
        || named.links != 1
        || owned.links != 1
        || !named.same_object(&owned)
    {
        return Err(Error::Corrupt("invalid coordinated writer lease object"));
    }
    if owned.length > RECORD_BYTES as u64 {
        return Err(Error::Corrupt("oversized coordinated writer owner record"));
    }
    Ok(())
}
/// An exclusive writer lease shared by native callers and the C adapter.
/// Other threads/processes wait; recursive acquisition on the same Windows
/// OS thread returns Busy. Retain it through native owner destruction.
pub struct WriterLease {
    lock: ulmdb_platform::RangeLock,
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
    pub fn try_acquire(directory: impl AsRef<Path>) -> Result<Self> {
        Self::acquire_inner(directory.as_ref(), false)
    }
    fn acquire_inner(directory: &Path, wait: bool) -> Result<Self> {
        let directory_file = ulmdb_platform::open_directory(directory)?;
        let file = ulmdb_platform::open_relative_writable(
            &directory_file,
            std::ffi::OsStr::new("LMDB-WRITER"),
            ulmdb_platform::RelativeKind::File,
            true,
            false,
        )?;
        validate(directory, &directory_file, &file)?;
        let current = Owner::current()?;
        let lock = match ulmdb_platform::RangeLock::acquire_returning_file(
            file,
            ulmdb_platform::LockMode::Exclusive,
            1 << 62,
            1,
            false,
        ) {
            Ok(lock) => lock,
            Err((error, file)) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if owner(&file)? == Some(current) || !wait {
                    return Err(Error::Busy);
                }
                ulmdb_platform::RangeLock::acquire(
                    file,
                    ulmdb_platform::LockMode::Exclusive,
                    1 << 62,
                    1,
                    true,
                )?
            }
            Err((error, _)) => return Err(error.into()),
        };
        // From here every error path explicitly releases the creator's range lease,
        // including owner-record publication failure.
        let mut lease = Self {
            lock,
            _directory: directory_file,
            creator: current.pid,
            clear: false,
            _thread: PhantomData,
        };
        validate(directory, &lease._directory, lease.lock.file())?;
        lease.clear = true;
        // A successful kernel lock supersedes stale or crash-partial state.
        // Complete the new record before any caller can enter its callback.
        lease.lock.file().set_len(0)?;
        lease.lock.file().write_all_at(&current.encode(), 0)?;
        Ok(lease)
    }
}
impl Drop for WriterLease {
    fn drop(&mut self) {
        if self.creator == std::process::id() && self.clear {
            // Clear before the owned range lease unlocks. Only an acquired
            // kernel lock can supersede an incomplete/crash owner record.
            let _ = self.lock.file().set_len(0);
        }
    }
}
