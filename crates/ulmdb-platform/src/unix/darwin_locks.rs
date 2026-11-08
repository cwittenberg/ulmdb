//! Darwin open-description byte-range locks. Independent opens remain distinct
//! owners within one process; closing another descriptor cannot release these
//! locks. No POSIX process-lock or whole-file fallback is permitted.
use super::{File, LockMode, io};
use std::os::fd::AsRawFd;

fn request(kind: libc::c_short, offset: u64, length: u64) -> io::Result<libc::flock> {
    if length == 0 {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let start = libc::off_t::try_from(offset).map_err(|_| io::ErrorKind::InvalidInput)?;
    let length = libc::off_t::try_from(length).map_err(|_| io::ErrorKind::InvalidInput)?;
    start
        .checked_add(length)
        .ok_or(io::ErrorKind::InvalidInput)?;
    Ok(libc::flock {
        l_start: start,
        l_len: length,
        l_pid: 0,
        l_type: kind,
        l_whence: libc::SEEK_SET as _,
    })
}

#[allow(unsafe_code)]
fn apply(file: &File, command: libc::c_int, request: &libc::flock) -> io::Result<()> {
    loop {
        if unsafe { libc::fcntl(file.as_raw_fd(), command, request) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if matches!(
            error.raw_os_error(),
            Some(libc::EAGAIN) | Some(libc::EACCES)
        ) {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        // Old kernels/filesystems may decline OFD locks. Preserve the error;
        // substituting process-scoped F_SETLK would lose lifetime protection.
        return Err(error);
    }
}

pub(super) fn lock(
    file: &File,
    mode: LockMode,
    offset: u64,
    length: u64,
    wait: bool,
) -> io::Result<()> {
    let kind = match mode {
        LockMode::Shared => libc::F_RDLCK,
        LockMode::Exclusive => libc::F_WRLCK,
    };
    let request = request(kind as _, offset, length)?;
    apply(
        file,
        if wait {
            libc::F_OFD_SETLKW
        } else {
            libc::F_OFD_SETLK
        },
        &request,
    )
}

pub(super) fn unlock(file: &File, offset: u64, length: u64) -> io::Result<()> {
    apply(
        file,
        libc::F_OFD_SETLK,
        &request(libc::F_UNLCK as _, offset, length)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ofd_requests_keep_full_offsets_and_exact_nonempty_ranges() {
        let value = request(libc::F_RDLCK as _, 1 << 40, 3).unwrap();
        assert_eq!(value.l_start, 1 << 40);
        assert_eq!(value.l_len, 3);
        assert_eq!(value.l_pid, 0);
        assert_eq!(value.l_whence, libc::SEEK_SET as _);
        assert!(request(libc::F_RDLCK as _, 1, 0).is_err());
        assert!(request(libc::F_RDLCK as _, i64::MAX as u64, 1).is_err());
        assert!(request(libc::F_RDLCK as _, u64::MAX, 1).is_err());
    }
}
