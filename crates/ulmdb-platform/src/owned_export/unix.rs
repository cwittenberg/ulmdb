#![allow(unsafe_code)]
use super::*;
use std::{
    ffi::CString,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    },
};
fn name(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| io::ErrorKind::InvalidInput.into())
}
pub(super) fn create(parent: &File, component: &OsStr, directory: bool) -> io::Result<File> {
    let component = name(component)?;
    if directory && unsafe { libc::mkdirat(parent.as_raw_fd(), component.as_ptr(), 0o700) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let flags = libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | if directory {
            libc::O_DIRECTORY | libc::O_RDONLY
        } else {
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL
        };
    let fd = unsafe { libc::openat(parent.as_raw_fd(), component.as_ptr(), flags, 0o600) };
    if fd < 0 {
        let error = io::Error::last_os_error();
        if directory {
            unsafe {
                libc::unlinkat(parent.as_raw_fd(), component.as_ptr(), libc::AT_REMOVEDIR);
            }
        }
        return Err(error);
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
pub(super) fn remove(
    parent: &File,
    component: &OsStr,
    _file: &File,
    directory: bool,
) -> io::Result<()> {
    let component = name(component)?;
    if unsafe {
        libc::unlinkat(
            parent.as_raw_fd(),
            component.as_ptr(),
            if directory { libc::AT_REMOVEDIR } else { 0 },
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub(super) fn rename(
    source: &File,
    _image: &File,
    parent: &File,
    component: &OsStr,
) -> io::Result<()> {
    let component = name(component)?;
    #[cfg(target_os = "linux")]
    let rc = unsafe {
        libc::renameat2(
            source.as_raw_fd(),
            c"image".as_ptr(),
            parent.as_raw_fd(),
            component.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(target_os = "macos")]
    let rc = unsafe {
        libc::renameatx_np(
            source.as_raw_fd(),
            c"image".as_ptr(),
            parent.as_raw_fd(),
            component.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    return Err(io::ErrorKind::Unsupported.into());
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
