//! Native file ownership, positioned IO and kernel locks shared by the storage
//! engine and C adapter. Raw HANDLEs are never truncated to CRT descriptors.
use std::{fs::File, io, path::Path};
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(any(windows, test))]
mod random;

/// Fill from the native Windows system-preferred CNG generator.
#[cfg(windows)]
pub fn fill_random(bytes: &mut [u8]) -> io::Result<()> {
    random::fill(bytes)
}
#[cfg(unix)]
use unix as sys;
#[cfg(windows)]
use windows as sys;
#[cfg(unix)]
pub type RawFileHandle = std::os::fd::RawFd;
#[cfg(windows)]
pub type RawFileHandle = std::os::windows::io::RawHandle;
#[cfg(unix)]
pub const INVALID_FILE_HANDLE: RawFileHandle = -1;
#[cfg(windows)]
pub const INVALID_FILE_HANDLE: RawFileHandle = -1isize as RawFileHandle;

/// An owned duplicate, sharing the caller's stream position. Dropping this File
/// never closes the supplied descriptor/HANDLE. Native positioned IO is separate.
pub fn duplicate_file_handle(handle: RawFileHandle) -> io::Result<File> {
    sys::duplicate(handle)
}
pub fn raw_file_handle(file: &File) -> RawFileHandle {
    sys::raw(file)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileIdentity {
    pub volume: u64,
    /// Full128-bit Windows FileIdInfo identifier; Unix inode is zero-extended.
    pub file_id: u128,
    pub length: u64,
    pub allocated_bytes: u64,
    pub links: u64,
    pub modified: i128,
    pub changed: i128,
    pub directory: bool,
    pub regular: bool,
    pub reparse: bool,
}
pub fn file_identity(file: &File) -> io::Result<FileIdentity> {
    sys::identity(file)
}
/// Open a real directory handle without following its final symlink/reparse point.
pub fn open_directory(path: &Path) -> io::Result<File> {
    sys::open_directory(path)
}
/// Open a regular read-only file without following the final symlink/reparse point.
pub fn open_readonly(path: &Path) -> io::Result<File> {
    sys::open_readonly(path)
}
/// Create a new owner-only artifact. Never opens or truncates an existing name.
pub fn create_private_file(path: &Path, read: bool) -> io::Result<File> {
    sys::create_private_file(path, read)
}
/// Create a private directory, excluding inherited group/everyone access.
pub fn create_private_directory(path: &Path) -> io::Result<()> {
    sys::create_private_directory(path)
}
/// Apply owner-only sealed/read-write permissions to an owned artifact.
/// Windows sealing denies overwrite/truncate while retaining narrow owner
/// append-only flush access; no API returns a writable handle from that right.
pub fn set_private_permissions(file: &File, writable: bool) -> io::Result<()> {
    sys::set_private_permissions(file, writable)
}
#[derive(Clone, Copy)]
pub enum RelativeKind {
    File,
    Directory,
}
/// Open exactly one relative component, rooted in an owned directory handle.
pub fn open_relative(
    parent: &File,
    name: &std::ffi::OsStr,
    kind: RelativeKind,
) -> io::Result<File> {
    sys::open_relative(parent, name, kind)
}

pub trait FileExt {
    fn read_at(&self, bytes: &mut [u8], offset: u64) -> io::Result<usize>;
    fn write_at(&self, bytes: &[u8], offset: u64) -> io::Result<usize>;
    fn read_exact_at(&self, mut bytes: &mut [u8], mut offset: u64) -> io::Result<()> {
        extent(offset, bytes.len())?;
        while !bytes.is_empty() {
            match self.read_at(bytes, offset) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) if n <= bytes.len() => {
                    offset += n as u64;
                    bytes = &mut bytes[n..];
                }
                Ok(_) => return Err(io::ErrorKind::InvalidData.into()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    fn write_all_at(&self, mut bytes: &[u8], mut offset: u64) -> io::Result<()> {
        extent(offset, bytes.len())?;
        while !bytes.is_empty() {
            match self.write_at(bytes, offset) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) if n <= bytes.len() => {
                    offset += n as u64;
                    bytes = &bytes[n..];
                }
                Ok(_) => return Err(io::ErrorKind::InvalidData.into()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}
impl FileExt for File {
    fn read_at(&self, b: &mut [u8], at: u64) -> io::Result<usize> {
        extent(at, b.len())?;
        sys::read_at(self, b, at)
    }
    fn write_at(&self, b: &[u8], at: u64) -> io::Result<usize> {
        extent(at, b.len())?;
        sys::write_at(self, b, at)
    }
}
fn extent(offset: u64, length: usize) -> io::Result<()> {
    offset
        .checked_add(u64::try_from(length).map_err(|_| io::ErrorKind::InvalidInput)?)
        .ok_or(io::ErrorKind::InvalidInput)?;
    Ok(())
}
#[derive(Clone, Copy, Debug)]
pub enum LockMode {
    Shared,
    Exclusive,
}
/// One owned kernel range lock. Callers pass an independently opened handle;
/// no process-global descriptor registry or emulated lock file is introduced.
pub struct RangeLock {
    file: File,
    creator: u32,
    offset: u64,
    length: u64,
}
impl RangeLock {
    pub fn acquire(
        file: File,
        mode: LockMode,
        offset: u64,
        length: u64,
        wait: bool,
    ) -> io::Result<Self> {
        Self::acquire_returning_file(file, mode, offset, length, wait).map_err(|(error, _)| error)
    }
    /// Same ownership as acquire, but failed admission returns the original
    /// File so a registry scan can inspect a live owner's unlocked data bytes.
    pub fn acquire_returning_file(
        file: File,
        mode: LockMode,
        offset: u64,
        length: u64,
        wait: bool,
    ) -> Result<Self, (io::Error, File)> {
        if length == 0 || offset.checked_add(length).is_none() {
            return Err((io::ErrorKind::InvalidInput.into(), file));
        }
        if let Err(error) = sys::lock_range(&file, mode, offset, length, wait) {
            return Err((error, file));
        }
        Ok(Self {
            file,
            creator: std::process::id(),
            offset,
            length,
        })
    }
    pub fn file(&self) -> &File {
        &self.file
    }
}
impl Drop for RangeLock {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            let _ = sys::unlock_range(&self.file, self.offset, self.length);
        }
    }
}
pub fn page_size() -> io::Result<usize> {
    sys::page_size()
}
/// Real mapped-view flush; callers separately flush the file when their policy requires it.
#[allow(unsafe_code)]
pub unsafe fn flush_view(pointer: *mut u8, length: usize) -> io::Result<()> {
    unsafe { sys::flush_view(pointer, length) }
}
#[allow(unsafe_code)]
pub unsafe fn lock_memory(pointer: *const u8, length: usize) -> io::Result<()> {
    unsafe { sys::lock_memory(pointer, length) }
}
#[allow(unsafe_code)]
pub unsafe fn unlock_memory(pointer: *const u8, length: usize) -> io::Result<()> {
    unsafe { sys::unlock_memory(pointer, length) }
}

#[cfg(test)]
mod tests;

#[cfg(unix)]
pub const READ_NOFOLLOW_FLAGS: i32 = libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
#[cfg(windows)]
pub const READ_NOFOLLOW_FLAGS: u32 = 0x00200000;
impl FileIdentity {
    pub fn same_object(&self, other: &Self) -> bool {
        (self.volume, self.file_id) == (other.volume, other.file_id)
    }
    pub fn len(&self) -> u64 {
        self.length
    }
    pub fn is_file(&self) -> bool {
        self.regular && !self.reparse
    }
    pub fn is_dir(&self) -> bool {
        self.directory && !self.reparse
    }
}
/// Query a named object without following its final symlink/reparse point.
/// Reparse state is reported explicitly; it never supplies target authority.
pub fn path_identity(path: &Path) -> io::Result<FileIdentity> {
    sys::path_identity(path)
}

/// Acquire a real kernel range lock whose lifetime is owned by the supplied
/// file object. Existing low-level WAL owners release it by last close.
pub fn lock_file_range(
    file: &File,
    mode: LockMode,
    offset: u64,
    length: u64,
    wait: bool,
) -> io::Result<()> {
    if length == 0 || offset.checked_add(length).is_none() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    sys::lock_range(file, mode, offset, length, wait)
}

/// Resource exhaustion is operational, not evidence of corrupt selected data.
pub fn is_resource_exhaustion(error: &io::Error) -> bool {
    #[cfg(unix)]
    {
        matches!(
            error.raw_os_error(),
            Some(libc::EMFILE | libc::ENFILE | libc::ENOBUFS | libc::ENOSPC | libc::EDQUOT)
        )
    }
    #[cfg(windows)]
    {
        matches!(
            error.raw_os_error(),
            Some(4 | 8 | 14 | 39 | 112 | 1450 | 1455 | 1816)
        )
    }
}

/// Release exactly a lock acquired on this owned file object.
pub fn unlock_file_range(file: &File, offset: u64, length: u64) -> io::Result<()> {
    if length == 0 || offset.checked_add(length).is_none() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    sys::unlock_range(file, offset, length)
}

/// Locate the module containing an address without adopting a caller handle.
/// # Safety
/// The caller must keep the containing module loaded throughout this call.
/// POSIX dladdr returns a borrowed loader pathname which must remain live.
#[allow(unsafe_code)]
pub unsafe fn module_path(address: *const std::ffi::c_void) -> io::Result<std::path::PathBuf> {
    sys::module_path(address)
}

/// Match LMDB's Windows CP_UTF8/flags0 pathname conversion (including invalid
/// sequence replacement); no process codepage or lossy ANSI path is used.
#[cfg(windows)]
pub fn path_from_utf8(bytes: &[u8]) -> io::Result<std::path::PathBuf> {
    windows::path_from_utf8(bytes)
}

/// Establish documented zero backing for a newly created private mapping file.
/// Existing contents are never truncated: only an empty, regular file is valid.
pub fn initialize_zeroed_file(file: &File, length: u64) -> io::Result<()> {
    let identity = file_identity(file)?;
    if !identity.is_file() || identity.length != 0 || length > i64::MAX as u64 {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    sys::initialize_zeroed_file(file, length)
}

/// Flush an exact held regular file without changing its contents/permissions.
pub fn sync_file(file: &File) -> io::Result<()> {
    sys::sync_file(file)
}
/// Flush held file data. Windows uses the stronger real data+metadata fence;
/// Unix preserves the caller's original fdatasync rather than adding fsync.
pub fn sync_data(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        file.sync_data()
    }
    #[cfg(windows)]
    {
        sys::sync_file(file)
    }
}
/// Flush the exact held directory, not another object at its former pathname.
pub fn sync_directory_file(directory: &File) -> io::Result<()> {
    sys::sync_directory_file(directory)
}
/// Convenience for callers which already validate this pathname around the fence.
pub fn sync_directory(path: &Path) -> io::Result<()> {
    sys::sync_directory(path)
}
/// Same-volume namespace replacement. Callers perform the directory fence
/// required by their sampled durability policy after successful replacement.
pub fn rename_replace(from: &Path, to: &Path) -> io::Result<()> {
    sys::rename_replace(from, to)
}
#[derive(Debug)]
struct RenameOutcomeUnknown(io::Error);
impl std::fmt::Display for RenameOutcomeUnknown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for RenameOutcomeUnknown {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}
/// A failed Windows rename may already have changed the namespace. Publication
/// callers must fence their writer instead of acknowledging an unchanged head.
pub fn rename_outcome_unknown(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|e| e.is::<RenameOutcomeUnknown>())
}
fn uncertain_rename(error: io::Error) -> io::Error {
    io::Error::new(error.kind(), RenameOutcomeUnknown(error))
}

/// Recover the underlying OS error for an operation on a still-unselected
/// asset. Only the caller can prove that its business head was never touched;
/// selectors must instead honor rename_outcome_unknown and fence publication.
pub fn rename_original_error(error: io::Error) -> io::Error {
    if !rename_outcome_unknown(&error) {
        return error;
    }
    let kind = error.kind();
    match error
        .into_inner()
        .and_then(|e| e.downcast::<RenameOutcomeUnknown>().ok())
    {
        Some(error) => error.0,
        None => io::Error::new(kind, "rename outcome unknown"),
    }
}

/// Preserve the ordinary POSIX open+fsync sequence while selecting the real
/// Windows file/directory fence from the exact opened object.
pub fn sync_path(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(windows)]
    {
        let named = path_identity(path)?;
        let file = if named.is_dir() {
            open_directory(path)?
        } else {
            open_readonly(path)?
        };
        if !file_identity(&file)?.same_object(&named) {
            return Err(io::ErrorKind::InvalidData.into());
        }
        if named.is_dir() {
            sync_directory_file(&file)
        } else {
            sync_file(&file)
        }
    }
}

/// Rooted no-reparse Windows coordination metadata open. Creation never
/// truncates an existing file and uses the protected private creation DACL.
#[cfg(windows)]
pub fn open_relative_writable(
    parent: &File,
    name: &std::ffi::OsStr,
    kind: RelativeKind,
    create: bool,
    exclusive: bool,
) -> io::Result<File> {
    sys::open_relative_options(parent, name, kind, true, create, exclusive)
}
#[cfg(windows)]
pub fn current_thread_id() -> u32 {
    sys::current_thread_id()
}
/// Remove the exact owned Windows regular file, not another pathname object.
#[cfg(windows)]
pub fn remove_owned_file(file: &File) -> io::Result<()> {
    sys::remove_owned_file(file)
}

/// Descriptor-rooted private directory export and no-replace publication.
pub mod owned_export;
