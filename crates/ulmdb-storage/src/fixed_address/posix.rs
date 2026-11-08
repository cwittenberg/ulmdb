//! POSIX operations confined to an owned address window. Initial Darwin
//! reservation is a non-destructive hint, followed by exact-address admission.
#![allow(unsafe_code)]
use std::{fs::File, io, os::fd::AsRawFd, ptr::NonNull};
pub(super) struct Window {
    base: NonNull<u8>,
    length: usize,
}
pub(super) fn page() -> io::Result<usize> {
    let value = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    usize::try_from(value)
        .ok()
        .filter(|n| n.is_power_of_two())
        .ok_or(io::ErrorKind::Unsupported.into())
}
impl Window {
    pub(super) fn reserve(requested: Option<usize>, length: usize) -> io::Result<Self> {
        #[cfg(target_os = "linux")]
        let exact = if requested.is_some() {
            libc::MAP_FIXED_NOREPLACE
        } else {
            0
        };
        #[cfg(not(target_os = "linux"))]
        let exact = 0;
        let pointer = unsafe {
            libc::mmap(
                requested.unwrap_or(0) as *mut _,
                length,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE | exact,
                -1,
                0,
            )
        };
        if pointer == libc::MAP_FAILED {
            let e = io::Error::last_os_error();
            return Err(if e.raw_os_error() == Some(libc::EEXIST) {
                io::ErrorKind::AddrInUse.into()
            } else {
                e
            });
        }
        // NOREPLACE can be ignored by older Linux kernels. Darwin never uses
        // destructive MAP_FIXED for initial reservation. An alternative is ours
        // to unmap, never authority to overwrite the requested occupied address.
        if pointer.is_null()
            || requested.is_some_and(|p| p != pointer as usize)
            || (pointer as usize).checked_add(length).is_none()
        {
            unsafe { libc::munmap(pointer, length) };
            return Err(io::ErrorKind::AddrInUse.into());
        }
        Ok(Self {
            base: NonNull::new(pointer.cast()).ok_or(io::ErrorKind::AddrInUse)?,
            length,
        })
    }
    pub(super) fn address(&self) -> NonNull<u8> {
        self.base
    }
    pub(super) fn map(
        &self,
        file: &File,
        offset: u64,
        address: *mut u8,
        length: usize,
        writable: bool,
    ) -> io::Result<*mut u8> {
        let offset = libc::off_t::try_from(offset).map_err(|_| io::ErrorKind::InvalidInput)?;
        let pointer = unsafe {
            libc::mmap(
                address.cast(),
                length,
                libc::PROT_READ | if writable { libc::PROT_WRITE } else { 0 },
                libc::MAP_SHARED | libc::MAP_FIXED,
                file.as_raw_fd(),
                offset,
            )
        };
        if pointer == libc::MAP_FAILED {
            Err(io::Error::last_os_error())
        } else {
            Ok(pointer.cast())
        }
    }
    pub(super) fn restore(&self, address: *mut u8, length: usize) -> io::Result<()> {
        let pointer = unsafe {
            libc::mmap(
                address.cast(),
                length,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE | libc::MAP_FIXED,
                -1,
                0,
            )
        };
        if pointer != address.cast() {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    pub(super) fn release(&self) -> io::Result<()> {
        if unsafe { libc::munmap(self.base.as_ptr().cast(), self.length) } != 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
pub(super) fn random(pointer: *mut u8, length: usize) -> io::Result<()> {
    if unsafe { libc::madvise(pointer.cast(), length, libc::MADV_RANDOM) } != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
pub(super) fn lock(pointer: *mut u8, length: usize) -> io::Result<()> {
    if unsafe { libc::mlock(pointer.cast(), length) } != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
pub(super) fn unlock(pointer: *mut u8, length: usize) -> io::Result<()> {
    if unsafe { libc::munlock(pointer.cast(), length) } != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
pub(super) fn flush(
    pointer: *mut u8,
    length: usize,
    asynchronous: bool,
    _file: Option<&File>,
) -> io::Result<()> {
    if unsafe {
        libc::msync(
            pointer.cast(),
            length,
            if asynchronous {
                libc::MS_ASYNC
            } else {
                libc::MS_SYNC
            },
        )
    } != 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
