//! Fresh length of the already-pinned WAL descriptor; no cached size or path lookup.
use std::{fs::File, io};

/// Immutable query strategy chosen once from the pinned WAL descriptor.
/// Stores no capacity, pathname, file handle or filesystem status structure.
#[derive(Clone, Copy)]
pub(super) struct CapacityProbe {
    use_fstat: bool,
}
impl CapacityProbe {
    pub(super) fn for_file(file: &File) -> Self {
        Self {
            use_fstat: ram_filesystem(file),
        }
    }
    pub(super) fn length(&self, file: &File) -> io::Result<u64> {
        if self.use_fstat {
            file_length(file)
        } else {
            file.metadata().map(|metadata| metadata.len())
        }
    }
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
#[allow(unsafe_code)]
fn ram_filesystem(file: &File) -> bool {
    use std::{mem::MaybeUninit, os::fd::AsRawFd};

    let mut status = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: File pins the live descriptor; libc::statfs has the target's
    // correct output ABI. No output is read unless fstatfs initializes it.
    if unsafe { libc::fstatfs(file.as_raw_fd(), status.as_mut_ptr()) } != 0 {
        return false; // Detection failure retains the existing metadata path.
    }
    let status = unsafe { status.assume_init() };
    ram_filesystem_kind(status.f_type as u64)
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
fn ram_filesystem_kind(kind: u64) -> bool {
    // Linux UAPI include/uapi/linux/magic.h. libc exposes TMPFS_MAGIC but
    // libc 0.2.189 does not expose RAMFS_MAGIC; keep that constant private.
    const RAMFS_MAGIC: u64 = 0x8584_58f6;
    kind == libc::TMPFS_MAGIC as u64 || kind == RAMFS_MAGIC
}

#[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
fn ram_filesystem(_file: &File) -> bool {
    false
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
#[allow(unsafe_code)]
pub(super) fn file_length(file: &File) -> io::Result<u64> {
    use std::{mem::MaybeUninit, os::fd::AsRawFd};

    let mut status = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: File keeps this descriptor live through the call; the output is a
    // correctly sized/aligned libc::stat for the target libc ABI. fstat initializes
    // that output on success, and no output bytes are read after an error.
    if unsafe { libc::fstat(file.as_raw_fd(), status.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let status = unsafe { status.assume_init() };
    // Match Unix Metadata::len's conversion. The caller retains its exact
    // comparison with the positive, admitted WAL capacity and mismatch poison.
    Ok(status.st_size as u64)
}

#[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
pub(super) fn file_length(file: &File) -> io::Result<u64> {
    file.metadata().map(|metadata| metadata.len())
}

#[cfg(test)]
#[path = "capacity/tests.rs"]
mod tests;
