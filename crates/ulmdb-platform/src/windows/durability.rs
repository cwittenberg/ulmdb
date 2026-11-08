//! NTFS namespace/file fences. No success fallback for unsupported filesystem
//! directory flushing, no ACL changes during sync, and no copy/delete rename.
use super::*;
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtFlushBuffersFileEx(
        file: Handle,
        flags: u32,
        parameters: *mut c_void,
        size: u32,
        status: *mut IoStatus,
    ) -> i32;
    fn NtQueryVolumeInformationFile(
        file: Handle,
        status: *mut IoStatus,
        info: *mut c_void,
        length: u32,
        class: u32,
    ) -> i32;
    fn NtSetInformationFile(
        file: Handle,
        status: *mut IoStatus,
        info: *mut c_void,
        length: u32,
        class: u32,
    ) -> i32;
}
fn status_error(status: i32) -> io::Error {
    io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(status) } as i32)
}
fn complete(file: &File, status: *const IoStatus, result: i32) -> io::Result<()> {
    let result = if result == 0x103 {
        // All owned handles here are synchronous; keep IOSB and request buffers
        // live through completion even if a filter pends an operation.
        if unsafe { WaitForSingleObject(file.as_raw_handle(), u32::MAX) } != 0 {
            std::process::abort(); // Cannot safely unwind an outstanding output.
        }
        unsafe { (*status).status as i32 }
    } else {
        result
    };
    if result < 0 {
        return Err(status_error(result));
    }
    let completed = unsafe { (*status).status as i32 };
    if completed < 0 {
        Err(status_error(completed))
    } else {
        Ok(())
    }
}
fn reopened(file: &File, access: u32, directory: bool) -> io::Result<File> {
    let handle = unsafe {
        ReOpenFile(
            file.as_raw_handle(),
            access | 0x80 | 0x100000,
            SHARE,
            if directory { BACKUP } else { 0 },
        )
    };
    if handle == INVALID {
        return Err(io::Error::last_os_error());
    }
    let opened = unsafe { File::from_raw_handle(handle) };
    if !identity(&opened)?.same_object(&identity(file)?) {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(opened)
}
#[repr(align(8))]
struct QueryBuffer<const N: usize>([u8; N]);
fn query<const N: usize>(
    file: &File,
    class: u32,
    output: &mut QueryBuffer<N>,
) -> io::Result<usize> {
    let mut status = IoStatus {
        status: 0,
        information: 0,
    };
    let result = unsafe {
        NtQueryVolumeInformationFile(
            file.as_raw_handle(),
            &mut status,
            output.0.as_mut_ptr().cast(),
            N as u32,
            class,
        )
    };
    complete(file, &raw const status, result)?;
    if status.information > N {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(status.information)
}
fn local_ntfs(file: &File) -> io::Result<()> {
    let mut device = QueryBuffer([0; 8]);
    if query(file, 4, &mut device)? < 8 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let characteristics = u32::from_le_bytes(device.0[4..8].try_into().unwrap());
    if characteristics & 0x10 != 0 {
        return Err(io::Error::from_raw_os_error(50));
    }
    let mut attributes = QueryBuffer([0; 1024]);
    let length = query(file, 5, &mut attributes)?;
    if length < 12 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let bytes = u32::from_le_bytes(attributes.0[8..12].try_into().unwrap()) as usize;
    if bytes % 2 != 0 || bytes > length - 12 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let name: Vec<u16> = attributes.0[12..12 + bytes]
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if name != [78, 84, 70, 83] {
        return Err(io::Error::from_raw_os_error(50));
    }
    Ok(())
}
fn flush(file: &File) -> io::Result<()> {
    let mut status = IoStatus {
        status: 0,
        information: 0,
    };
    let result =
        unsafe { NtFlushBuffersFileEx(file.as_raw_handle(), 0, ptr::null_mut(), 0, &mut status) };
    complete(file, &raw const status, result)
}
pub(super) fn sync_file(file: &File) -> io::Result<()> {
    let expected = identity(file)?;
    if !expected.is_file() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    // APPEND_DATA allows the real NT flush, without granting overwrite access.
    // Sealed artifacts retain this narrow owner right from initial sealing.
    let owner = reopened(file, 4, false)?;
    flush(&owner)?;
    let after = identity(&owner)?;
    if !after.same_object(&expected) || after.length != expected.length {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(())
}
pub(super) fn sync_directory_file(directory: &File) -> io::Result<()> {
    let expected = identity(directory)?;
    if !expected.is_dir() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    // Directory FILE_ADD_FILE is the same access bit as file WRITE_DATA.
    // Use directory-specific rights, never a pretend read-only file fsync.
    let owner = reopened(directory, 2, true)?;
    local_ntfs(&owner)?;
    flush(&owner)?;
    if !identity(&owner)?.same_object(&expected) {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(())
}
fn name_units(name: &OsStr) -> io::Result<Vec<u16>> {
    let mut name = wide(name)?;
    name.pop();
    if name.len() > 32767 {
        return Err(io::Error::from_raw_os_error(206));
    }
    if name.is_empty()
        || name.iter().any(|c| matches!(*c, 0 | 47 | 92 | 58))
        || name == [46]
        || name == [46, 46]
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(name)
}
pub(super) fn rename_replace(from: &Path, to: &Path) -> io::Result<()> {
    let parent = to
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = open_directory(parent)?;
    local_ntfs(&parent)?;
    let name = name_units(to.file_name().ok_or(io::ErrorKind::InvalidInput)?)?;
    let before = path_identity(from)?;
    if before.directory {
        match path_identity(to) {
            Ok(_) => return Err(io::Error::from_raw_os_error(50)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    if before.reparse || !(before.regular || before.directory) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let path = wide(from.as_os_str())?;
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            0x10000 | 0x80 | 0x100000,
            SHARE,
            ptr::null_mut(),
            3,
            BACKUP | OPEN_REPARSE,
            ptr::null_mut(),
        )
    };
    if handle == INVALID {
        return Err(io::Error::last_os_error());
    }
    let source = unsafe { File::from_raw_handle(handle) };
    if !identity(&source)?.same_object(&before) || before.volume != identity(&parent)?.volume {
        return Err(io::ErrorKind::InvalidData.into());
    }
    // FILE_RENAME_INFORMATION_EX (class65), Windows10+. A held destination
    // directory handle prevents cwd/path rebinding. POSIX file replacement
    // retains old open objects; existing directory targets were rejected before
    // mutation because Windows does not provide that replacement guarantee.
    let pointer = mem::size_of::<usize>();
    let root = (4 + pointer - 1) / pointer * pointer;
    let size = root + pointer;
    let offset = size + 4;
    let length = offset
        .checked_add(
            name.len()
                .checked_mul(2)
                .ok_or(io::ErrorKind::InvalidInput)?,
        )
        .ok_or(io::ErrorKind::InvalidInput)?;
    let mut aligned = vec![0u64; length.div_ceil(8)];
    let bytes = unsafe {
        std::slice::from_raw_parts_mut(aligned.as_mut_ptr().cast::<u8>(), aligned.len() * 8)
    };
    bytes[..4].copy_from_slice(&3u32.to_le_bytes()); // REPLACE_IF_EXISTS|POSIX_SEMANTICS
    bytes[root..size].copy_from_slice(&(parent.as_raw_handle() as usize).to_le_bytes());
    bytes[size..offset].copy_from_slice(&((name.len() * 2) as u32).to_le_bytes());
    for (value, output) in name
        .iter()
        .zip(bytes[offset..offset + name.len() * 2].chunks_exact_mut(2))
    {
        output.copy_from_slice(&value.to_le_bytes());
    }
    let mut status = IoStatus {
        status: 0,
        information: 0,
    };
    let result = unsafe {
        NtSetInformationFile(
            source.as_raw_handle(),
            &mut status,
            aligned.as_mut_ptr().cast(),
            length as u32,
            65,
        )
    };
    complete(&source, &raw const status, result).map_err(crate::uncertain_rename)?;
    let named = open_relative(
        &parent,
        to.file_name().unwrap(),
        if before.directory {
            RelativeKind::Directory
        } else {
            RelativeKind::File
        },
    )
    .map_err(crate::uncertain_rename)?;
    if !identity(&named)
        .map_err(crate::uncertain_rename)?
        .same_object(&before)
    {
        return Err(crate::uncertain_rename(io::ErrorKind::InvalidData.into()));
    }
    Ok(())
}
