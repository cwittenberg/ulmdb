//! Rooted private output on Windows; no path opens, replacement or copy/delete.
#![allow(unsafe_code)]
use super::*;
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtSetInformationFile(
        file: Handle,
        status: *mut IoStatus,
        info: *mut c_void,
        length: u32,
        class: u32,
    ) -> i32;
}
fn complete(file: &File, status: &IoStatus, result: i32) -> io::Result<()> {
    let result = if result == 0x103 {
        if unsafe { WaitForSingleObject(file.as_raw_handle(), u32::MAX) } != 0 {
            std::process::abort();
        }
        status.status as i32
    } else {
        result
    };
    if result < 0 {
        Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32,
        ))
    } else {
        Ok(())
    }
}
fn units(name: &OsStr) -> io::Result<Vec<u16>> {
    let mut n = wide(name)?;
    n.pop();
    if n.is_empty()
        || n.len() > 32767
        || n.iter().any(|v| matches!(*v, 0 | 47 | 92 | 58))
        || n == [46]
        || n == [46, 46]
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(n)
}
pub(crate) fn create(parent: &File, name: &OsStr, directory: bool) -> io::Result<File> {
    if !identity(parent)?.is_dir() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut name = units(name)?;
    let length = u16::try_from(name.len() * 2).map_err(|_| io::ErrorKind::InvalidInput)?;
    let mut text = UnicodeString {
        length,
        maximum: length,
        buffer: name.as_mut_ptr(),
    };
    let descriptor = Descriptor::private(true, directory)?;
    let mut attrs = ObjectAttributes {
        length: mem::size_of::<ObjectAttributes>() as u32,
        root: parent.as_raw_handle(),
        name: &mut text,
        attributes: 0x40 | 0x1000,
        security: descriptor.0,
        quality: ptr::null_mut(),
    };
    let mut status = IoStatus {
        status: 0,
        information: 0,
    };
    let mut handle = ptr::null_mut();
    let access = if directory {
        0x1 | 0x2 | 0x4 | 0x20 | 0x40 | 0x80 | 0x10000 | 0x100000
    } else {
        GENERIC_READ | GENERIC_WRITE | 0x10000 | 0x100000
    };
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &mut attrs,
            &mut status,
            ptr::null_mut(),
            0,
            SHARE,
            2,
            0x200000 | 0x4000 | 0x20 | if directory { 1 } else { 0x40 },
            ptr::null_mut(),
            0,
        )
    };
    if result < 0 {
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32,
        ));
    }
    if handle.is_null() || handle == INVALID {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let file = unsafe { File::from_raw_handle(handle) };
    complete(&file, &status, result)?;
    let id = identity(&file)?;
    if id.reparse || id.directory != directory {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(file)
}
pub(crate) fn remove(
    _parent: &File,
    _name: &OsStr,
    file: &File,
    _directory: bool,
) -> io::Result<()> {
    let mut status = IoStatus {
        status: 0,
        information: 0,
    };
    let mut flags = 3u32;
    let rc = unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut status,
            (&mut flags as *mut u32).cast(),
            4,
            64,
        )
    };
    complete(file, &status, rc)
}
pub(crate) fn rename(_source: &File, image: &File, parent: &File, name: &OsStr) -> io::Result<()> {
    let before = identity(image)?;
    if !before.is_dir() || !identity(parent)?.is_dir() || before.volume != identity(parent)?.volume
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    // Reopen the exact image object with DELETE even for the legacy held-FD API.
    let raw = unsafe {
        ReOpenFile(
            image.as_raw_handle(),
            0x10000 | 0x80 | 0x100000,
            SHARE,
            BACKUP,
        )
    };
    if raw == INVALID {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_handle(raw) };
    if !identity(&file)?.same_object(&before) {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let name = units(name)?;
    let pointer = mem::size_of::<usize>();
    let root = (4 + pointer - 1) / pointer * pointer;
    let end = root + pointer;
    let offset = end + 4;
    let length = offset + name.len() * 2;
    let mut aligned = vec![0u64; length.div_ceil(8)];
    let bytes = unsafe {
        std::slice::from_raw_parts_mut(aligned.as_mut_ptr().cast::<u8>(), aligned.len() * 8)
    };
    // FILE_RENAME_INFORMATION_EX flags=0: never REPLACE_IF_EXISTS.
    bytes[root..end].copy_from_slice(&(parent.as_raw_handle() as usize).to_le_bytes());
    bytes[end..offset].copy_from_slice(&((name.len() * 2) as u32).to_le_bytes());
    for (unit, out) in name.iter().zip(bytes[offset..length].chunks_exact_mut(2)) {
        out.copy_from_slice(&unit.to_le_bytes());
    }
    let mut status = IoStatus {
        status: 0,
        information: 0,
    };
    let rc = unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut status,
            aligned.as_mut_ptr().cast(),
            length as u32,
            65,
        )
    };
    complete(&file, &status, rc).map_err(|e| {
        if e.kind() == io::ErrorKind::AlreadyExists {
            e
        } else {
            crate::uncertain_rename(e)
        }
    })
}
