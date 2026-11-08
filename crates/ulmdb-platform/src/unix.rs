use super::*;
use std::{
    ffi::{CString, OsStr},
    fs::OpenOptions,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{
                DirBuilderExt, FileExt as StdFileExt, MetadataExt, OpenOptionsExt, PermissionsExt,
            },
        },
    },
};
pub(super) fn raw(file: &File) -> RawFileHandle {
    file.as_raw_fd()
}
#[allow(unsafe_code)]
pub(super) fn duplicate(handle: RawFileHandle) -> io::Result<File> {
    let fd = unsafe { libc::fcntl(handle, libc::F_DUPFD_CLOEXEC, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
pub(super) fn identity(file: &File) -> io::Result<FileIdentity> {
    metadata_identity(&file.metadata()?)
}
pub(super) fn path_identity(path: &Path) -> io::Result<FileIdentity> {
    metadata_identity(&std::fs::symlink_metadata(path)?)
}
fn metadata_identity(m: &std::fs::Metadata) -> io::Result<FileIdentity> {
    Ok(FileIdentity {
        volume: m.dev(),
        file_id: m.ino() as u128,
        length: m.len(),
        allocated_bytes: m
            .blocks()
            .checked_mul(512)
            .ok_or(io::ErrorKind::InvalidData)?,
        links: m.nlink(),
        modified: (m.mtime() as i128) * 1_000_000_000 + m.mtime_nsec() as i128,
        changed: (m.ctime() as i128) * 1_000_000_000 + m.ctime_nsec() as i128,
        directory: m.is_dir(),
        regular: m.is_file(),
        reparse: m.file_type().is_symlink(),
    })
}
pub(super) fn open_directory(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}
#[allow(unsafe_code)]
pub(super) fn open_relative(parent: &File, name: &OsStr, kind: RelativeKind) -> io::Result<File> {
    if std::path::Path::new(name).components().count() != 1
        || !matches!(
            std::path::Path::new(name).components().next(),
            Some(std::path::Component::Normal(_))
        )
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let name = CString::new(name.as_bytes()).map_err(|_| io::ErrorKind::InvalidInput)?;
    let flags = libc::O_RDONLY
        | libc::O_NOFOLLOW
        | libc::O_CLOEXEC
        | libc::O_NONBLOCK
        | if matches!(kind, RelativeKind::Directory) {
            libc::O_DIRECTORY
        } else {
            0
        };
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let m = file.metadata()?;
    if matches!(kind, RelativeKind::File) && !m.is_file() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(file)
}
pub(super) fn read_at(f: &File, b: &mut [u8], at: u64) -> io::Result<usize> {
    StdFileExt::read_at(f, b, at)
}
pub(super) fn write_at(f: &File, b: &[u8], at: u64) -> io::Result<usize> {
    StdFileExt::write_at(f, b, at)
}
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
pub(super) fn lock_range(
    f: &File,
    mode: LockMode,
    at: u64,
    len: u64,
    wait: bool,
) -> io::Result<()> {
    let at = i64::try_from(at).map_err(|_| io::ErrorKind::InvalidInput)?;
    let len = i64::try_from(len).map_err(|_| io::ErrorKind::InvalidInput)?;
    let request = libc::flock64 {
        l_type: if matches!(mode, LockMode::Exclusive) {
            libc::F_WRLCK as _
        } else {
            libc::F_RDLCK as _
        },
        l_whence: libc::SEEK_SET as _,
        l_start: at,
        l_len: len,
        l_pid: 0,
    };
    loop {
        let rc = unsafe {
            #[cfg(target_pointer_width = "64")]
            {
                libc::fcntl(
                    f.as_raw_fd(),
                    if wait {
                        libc::F_OFD_SETLKW
                    } else {
                        libc::F_OFD_SETLK
                    },
                    &request,
                ) as libc::c_long
            }
            #[cfg(target_pointer_width = "32")]
            {
                libc::syscall(
                    libc::SYS_fcntl64,
                    f.as_raw_fd(),
                    if wait {
                        libc::F_OFD_SETLKW
                    } else {
                        libc::F_OFD_SETLK
                    },
                    &request,
                )
            }
        };
        if rc == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        if e.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if matches!(e.raw_os_error(), Some(libc::EACCES) | Some(libc::EAGAIN)) {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        return Err(e);
    }
}
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
pub(super) fn unlock_range(f: &File, at: u64, len: u64) -> io::Result<()> {
    let request = libc::flock64 {
        l_type: libc::F_UNLCK as _,
        l_whence: libc::SEEK_SET as _,
        l_start: i64::try_from(at).map_err(|_| io::ErrorKind::InvalidInput)?,
        l_len: i64::try_from(len).map_err(|_| io::ErrorKind::InvalidInput)?,
        l_pid: 0,
    };
    let result = unsafe {
        #[cfg(target_pointer_width = "64")]
        {
            libc::fcntl(f.as_raw_fd(), libc::F_OFD_SETLK, &request) as libc::c_long
        }
        #[cfg(target_pointer_width = "32")]
        {
            libc::syscall(
                libc::SYS_fcntl64,
                f.as_raw_fd(),
                libc::F_OFD_SETLK,
                &request,
            )
        }
    };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(target_os = "macos")]
#[path = "unix/darwin_locks.rs"]
mod darwin_locks;
#[cfg(target_os = "macos")]
pub(super) fn lock_range(
    file: &File,
    mode: LockMode,
    offset: u64,
    length: u64,
    wait: bool,
) -> io::Result<()> {
    darwin_locks::lock(file, mode, offset, length, wait)
}
#[cfg(target_os = "macos")]
pub(super) fn unlock_range(file: &File, offset: u64, length: u64) -> io::Result<()> {
    darwin_locks::unlock(file, offset, length)
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn lock_range(_: &File, _: LockMode, _: u64, _: u64, _: bool) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn unlock_range(_: &File, _: u64, _: u64) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}
#[allow(unsafe_code)]
pub(super) fn page_size() -> io::Result<usize> {
    let n = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    usize::try_from(n)
        .ok()
        .filter(|n| n.is_power_of_two())
        .ok_or(io::ErrorKind::InvalidData.into())
}
#[allow(unsafe_code)]
pub(super) unsafe fn flush_view(p: *mut u8, n: usize) -> io::Result<()> {
    if unsafe { libc::msync(p.cast(), n, libc::MS_ASYNC) } != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[allow(unsafe_code)]
pub(super) unsafe fn lock_memory(p: *const u8, n: usize) -> io::Result<()> {
    if unsafe { libc::mlock(p.cast(), n) } != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[allow(unsafe_code)]
pub(super) unsafe fn unlock_memory(p: *const u8, n: usize) -> io::Result<()> {
    if unsafe { libc::munlock(p.cast(), n) } != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(super) fn open_readonly(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(file)
}
pub(super) fn create_private_file(path: &Path, read: bool) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .read(read)
        .create_new(true)
        .mode(0o600)
        .open(path)
}
pub(super) fn create_private_directory(path: &Path) -> io::Result<()> {
    std::fs::DirBuilder::new().mode(0o700).create(path)
}
pub(super) fn set_private_permissions(file: &File, writable: bool) -> io::Result<()> {
    file.set_permissions(std::fs::Permissions::from_mode(if writable {
        0o600
    } else {
        0o400
    }))
}

#[allow(unsafe_code)]
pub(super) fn module_path(address: *const std::ffi::c_void) -> io::Result<std::path::PathBuf> {
    if address.is_null() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
    if unsafe { libc::dladdr(address, &mut info) } == 0 || info.dli_fname.is_null() {
        return Err(io::ErrorKind::NotFound.into());
    }
    let bytes = unsafe { std::ffi::CStr::from_ptr(info.dli_fname) }.to_bytes();
    Ok(std::path::PathBuf::from(OsStr::from_bytes(bytes)))
}

pub(super) fn initialize_zeroed_file(file: &File, length: u64) -> io::Result<()> {
    // POSIX ftruncate establishes zero-filled bytes beyond the old EOF.
    file.set_len(length)
}

pub(super) fn sync_file(file: &File) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        // Some portable callers pass an already-owned directory handle. Device
        // cache flush applies to regular files; directory namespace uses fsync.
        if file.metadata()?.is_dir() {
            return sync_directory_file(file);
        }
        darwin_full_sync(file)
    }
    #[cfg(not(target_os = "macos"))]
    {
        file.sync_all()
    }
}
/// Darwin fsync alone need not flush the device cache. Full file durability uses
/// the stronger OS operation (also used by LMDB), with no successful fallback.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn darwin_full_sync(file: &File) -> io::Result<()> {
    loop {
        // SAFETY: F_FULLFSYNC takes no extra argument; file owns a live fd.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}
#[allow(unsafe_code)]
pub(super) fn sync_directory_file(directory: &File) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    loop {
        // SAFETY: directory owns the descriptor. F_FULLFSYNC is a regular-file
        // fence; use the namespace fsync operation directly for directories.
        if unsafe { libc::fsync(directory.as_raw_fd()) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
    #[cfg(not(target_os = "macos"))]
    directory.sync_all()
}
pub(super) fn sync_directory(path: &Path) -> io::Result<()> {
    sync_directory_file(&File::open(path)?)
}
pub(super) fn rename_replace(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}
