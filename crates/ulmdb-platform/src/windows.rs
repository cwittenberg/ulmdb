//! Win32 ownership/IO boundary. Positioned calls reopen an independent
//! OVERLAPPED file object; std seek_read on synchronous File would alter its
//! shared cursor. Each request has its own event and waits before buffers drop.
#![allow(unsafe_code)]
use super::*;
mod durability;
pub(super) mod owned_export;
use std::{
    ffi::{OsStr, c_void},
    mem::{self, MaybeUninit},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    ptr,
};
type Handle = *mut c_void;
const INVALID: Handle = -1isize as Handle;
const SHARE: u32 = 7;
const GENERIC_READ: u32 = 0x80000000;
const GENERIC_WRITE: u32 = 0x40000000;
const OVERLAPPED_FLAG: u32 = 0x40000000;
const BACKUP: u32 = 0x02000000;
const OPEN_REPARSE: u32 = 0x00200000;
const REPARSE: u32 = 0x400;
const ERROR_PENDING: i32 = 997;
const ERROR_EOF: i32 = 38;
#[repr(C)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: Handle,
}
#[repr(C)]
struct SystemInfo {
    architecture: u16,
    reserved: u16,
    page: u32,
    min: *mut c_void,
    max: *mut c_void,
    mask: usize,
    processors: u32,
    kind: u32,
    granularity: u32,
    level: u16,
    revision: u16,
}
#[repr(C)]
struct IdInfo {
    volume: u64,
    id: [u8; 16],
}
#[repr(C)]
struct StandardInfo {
    allocated: i64,
    end: i64,
    links: u32,
    pending: u8,
    directory: u8,
    pad: [u8; 2],
}
#[repr(C)]
struct BasicInfo {
    creation: i64,
    access: i64,
    write: i64,
    change: i64,
    attributes: u32,
    pad: u32,
}
#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum: u16,
    buffer: *mut u16,
}
#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root: Handle,
    name: *mut UnicodeString,
    attributes: u32,
    security: *mut c_void,
    quality: *mut c_void,
}
#[repr(C)]
struct IoStatus {
    status: usize,
    information: usize,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> Handle;
    fn MultiByteToWideChar(
        page: u32,
        flags: u32,
        source: *const u8,
        length: i32,
        output: *mut u16,
        capacity: i32,
    ) -> i32;
    fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut Handle) -> i32;
    fn GetModuleFileNameW(module: Handle, buffer: *mut u16, length: u32) -> u32;
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
    fn CreateDirectoryW(path: *const u16, security: *mut SecurityAttributes) -> i32;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn DeviceIoControl(
        file: Handle,
        code: u32,
        input: *mut c_void,
        input_len: u32,
        output: *mut c_void,
        output_len: u32,
        transferred: *mut u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn GetFileType(handle: Handle) -> u32;
    fn DuplicateHandle(
        process: Handle,
        source: Handle,
        target: Handle,
        result: *mut Handle,
        access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
    fn ReOpenFile(file: Handle, access: u32, share: u32, flags: u32) -> Handle;
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *mut c_void,
        disposition: u32,
        flags: u32,
        template: Handle,
    ) -> Handle;
    fn CreateEventW(security: *mut c_void, manual: i32, initial: i32, name: *const u16) -> Handle;
    fn ReadFile(
        file: Handle,
        buffer: *mut c_void,
        length: u32,
        done: *mut u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn WriteFile(
        file: Handle,
        buffer: *const c_void,
        length: u32,
        done: *mut u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn GetOverlappedResult(
        file: Handle,
        overlapped: *mut Overlapped,
        done: *mut u32,
        wait: i32,
    ) -> i32;
    fn GetFileInformationByHandleEx(
        file: Handle,
        class: u32,
        information: *mut c_void,
        size: u32,
    ) -> i32;
    fn LockFileEx(
        file: Handle,
        flags: u32,
        reserved: u32,
        low: u32,
        high: u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn UnlockFileEx(
        file: Handle,
        reserved: u32,
        low: u32,
        high: u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn GetSystemInfo(info: *mut SystemInfo);
    fn FlushViewOfFile(address: *const c_void, length: usize) -> i32;
    fn VirtualLock(address: *const c_void, length: usize) -> i32;
    fn VirtualUnlock(address: *const c_void, length: usize) -> i32;
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        handle: *mut Handle,
        access: u32,
        attributes: *mut ObjectAttributes,
        status: *mut IoStatus,
        allocation: *mut i64,
        file_attributes: u32,
        share: u32,
        disposition: u32,
        options: u32,
        ea: *mut c_void,
        ea_length: u32,
    ) -> i32;
    pub(super) fn RtlNtStatusToDosError(status: i32) -> u32;
}
pub(super) fn raw(f: &File) -> RawFileHandle {
    f.as_raw_handle()
}
pub(super) fn duplicate(handle: RawFileHandle) -> io::Result<File> {
    if handle.is_null()
        || handle == INVALID
        || !matches!(unsafe { GetFileType(handle) } & !0x8000, 1 | 2 | 3)
    {
        return Err(io::Error::from_raw_os_error(6));
    }
    let process = unsafe { GetCurrentProcess() };
    let mut output = ptr::null_mut();
    // Kernel validates input; only the new non-inheritable handle transfers to File.
    if unsafe { DuplicateHandle(process, handle, process, &mut output, 0, 0, 2) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_handle(output) })
}
fn info<T>(file: &File, class: u32) -> io::Result<T> {
    let mut value = MaybeUninit::<T>::uninit();
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            class,
            value.as_mut_ptr().cast(),
            mem::size_of::<T>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { value.assume_init() })
}
pub(super) fn identity(file: &File) -> io::Result<FileIdentity> {
    let id: IdInfo = info(file, 18)?;
    let standard: StandardInfo = info(file, 1)?;
    let basic: BasicInfo = info(file, 0)?;
    Ok(FileIdentity {
        volume: id.volume,
        file_id: u128::from_le_bytes(id.id),
        length: u64::try_from(standard.end).map_err(|_| io::ErrorKind::InvalidData)?,
        allocated_bytes: u64::try_from(standard.allocated)
            .map_err(|_| io::ErrorKind::InvalidData)?,
        links: standard.links as u64,
        modified: (basic.write as i128) * 100,
        changed: (basic.change as i128) * 100,
        directory: standard.directory != 0,
        regular: standard.directory == 0 && unsafe { GetFileType(file.as_raw_handle()) } == 1,
        reparse: basic.attributes & REPARSE != 0,
    })
}
fn wide(path: &OsStr) -> io::Result<Vec<u16>> {
    let mut value: Vec<u16> = path.encode_wide().collect();
    if value.contains(&0) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    value.push(0);
    Ok(value)
}
pub(super) fn open_directory(path: &Path) -> io::Result<File> {
    let name = wide(path.as_os_str())?;
    let h = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ,
            SHARE,
            ptr::null_mut(),
            3,
            BACKUP | OPEN_REPARSE,
            ptr::null_mut(),
        )
    };
    if h == INVALID {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_handle(h) };
    let id = identity(&file)?;
    if !id.directory || id.reparse {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(file)
}
pub(super) fn open_relative(parent: &File, name: &OsStr, kind: RelativeKind) -> io::Result<File> {
    open_relative_options(parent, name, kind, false, false, false)
}
pub(super) fn open_relative_options(
    parent: &File,
    name: &OsStr,
    kind: RelativeKind,
    writable: bool,
    create: bool,
    exclusive: bool,
) -> io::Result<File> {
    if exclusive && !create {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let parent_id = identity(parent)?;
    if !parent_id.directory || parent_id.reparse {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut name = wide(name)?;
    name.pop();
    if name.is_empty()
        || name.iter().any(|u| matches!(*u, 0 | 47 | 92 | 58))
        || name == [46]
        || name == [46, 46]
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let bytes = u16::try_from(
        name.len()
            .checked_mul(2)
            .ok_or(io::ErrorKind::InvalidInput)?,
    )
    .map_err(|_| io::ErrorKind::InvalidInput)?;
    let mut unicode = UnicodeString {
        length: bytes,
        maximum: bytes,
        buffer: name.as_mut_ptr(),
    };
    let descriptor = if create {
        Some(Descriptor::private(
            true,
            matches!(kind, RelativeKind::Directory),
        )?)
    } else {
        None
    };
    let mut attributes = ObjectAttributes {
        length: mem::size_of::<ObjectAttributes>() as u32,
        root: parent.as_raw_handle(),
        name: &mut unicode,
        attributes: 0x40 | 0x1000,
        security: descriptor.as_ref().map_or(ptr::null_mut(), |d| d.0),
        quality: ptr::null_mut(),
    };
    let mut status = IoStatus {
        status: 0,
        information: 0,
    };
    let mut handle = ptr::null_mut();
    let options = 0x200000
        | 0x4000
        | 0x20
        | if matches!(kind, RelativeKind::Directory) {
            1
        } else {
            0x40
        };
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            if matches!(kind, RelativeKind::Directory) {
                // NtCreateFile directories require directory-specific rights;
                // unlike Win32 CreateFile, generic file-data rights are invalid.
                0x1 | 0x20 | 0x80 | 0x100000
            } else {
                GENERIC_READ | if writable { GENERIC_WRITE | 0x10000 } else { 0 } | 0x100000
            },
            &mut attributes,
            &mut status,
            ptr::null_mut(),
            0,
            SHARE,
            if exclusive {
                2
            } else if create {
                3
            } else {
                1
            },
            options,
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
    if result == 0x103 {
        // The synchronous-open option normally completes inside NtCreateFile;
        // if a filesystem pends it, keep the request/name/status alive until
        // kernel completion before inspecting the owned handle.
        if unsafe { WaitForSingleObject(handle, u32::MAX) } != 0 {
            // No safe unwind can release a still-pending kernel output buffer.
            // This owned, valid synchronous file handle cannot normally fail
            // its wait; fail-stop if the kernel violates that invariant.
            std::process::abort();
        }
        let result = status.status as i32;
        if result < 0 {
            return Err(io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(result) } as i32,
            ));
        }
    }
    let id = identity(&file)?;
    if id.reparse || id.directory != matches!(kind, RelativeKind::Directory) {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(file)
}
fn overlapped(offset: u64, event: Handle) -> io::Result<Overlapped> {
    if offset > i64::MAX as u64 {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(Overlapped {
        internal: 0,
        internal_high: 0,
        offset: offset as u32,
        offset_high: (offset >> 32) as u32,
        event,
    })
}
fn event() -> io::Result<OwnedHandle> {
    let h = unsafe { CreateEventW(ptr::null_mut(), 1, 0, ptr::null()) };
    if h.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(h) })
    }
}
fn positioned(
    file: &File,
    buffer: *mut c_void,
    length: usize,
    offset: u64,
    write: bool,
) -> io::Result<usize> {
    if length == 0 {
        return Ok(0);
    }
    let access = if write { GENERIC_WRITE } else { GENERIC_READ };
    let handle = unsafe { ReOpenFile(file.as_raw_handle(), access, SHARE, OVERLAPPED_FLAG) };
    if handle == INVALID {
        return Err(io::Error::last_os_error());
    }
    let independent = unsafe { File::from_raw_handle(handle) };
    let completion = event()?;
    let mut request = overlapped(offset, completion.as_raw_handle())?;
    let n = length.min(1 << 20) as u32;
    let rc = unsafe {
        if write {
            WriteFile(
                independent.as_raw_handle(),
                buffer.cast_const(),
                n,
                ptr::null_mut(),
                &mut request,
            )
        } else {
            ReadFile(
                independent.as_raw_handle(),
                buffer,
                n,
                ptr::null_mut(),
                &mut request,
            )
        }
    };
    if rc == 0 {
        let e = io::Error::last_os_error();
        if !write && e.raw_os_error() == Some(ERROR_EOF) {
            return Ok(0);
        }
        if e.raw_os_error() != Some(ERROR_PENDING) {
            return Err(e);
        }
    }
    let mut transferred = 0;
    if unsafe {
        GetOverlappedResult(
            independent.as_raw_handle(),
            &mut request,
            &mut transferred,
            1,
        )
    } == 0
    {
        let e = io::Error::last_os_error();
        if !write && e.raw_os_error() == Some(ERROR_EOF) {
            return Ok(0);
        }
        return Err(e);
    }
    Ok(transferred as usize)
}
pub(super) fn read_at(f: &File, b: &mut [u8], at: u64) -> io::Result<usize> {
    positioned(f, b.as_mut_ptr().cast(), b.len(), at, false)
}
pub(super) fn write_at(f: &File, b: &[u8], at: u64) -> io::Result<usize> {
    positioned(f, b.as_ptr().cast_mut().cast(), b.len(), at, true)
}
pub(super) fn lock_range(
    f: &File,
    mode: LockMode,
    at: u64,
    len: u64,
    wait: bool,
) -> io::Result<()> {
    let event = event()?;
    let mut request = overlapped(at, event.as_raw_handle())?;
    let flags = if wait { 0 } else { 1 }
        | if matches!(mode, LockMode::Exclusive) {
            2
        } else {
            0
        };
    let rc = unsafe {
        LockFileEx(
            f.as_raw_handle(),
            flags,
            0,
            len as u32,
            (len >> 32) as u32,
            &mut request,
        )
    };
    if rc != 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    if e.raw_os_error() == Some(ERROR_PENDING) {
        let mut done = 0;
        if unsafe { GetOverlappedResult(f.as_raw_handle(), &mut request, &mut done, 1) } != 0 {
            return Ok(());
        }
        return Err(io::Error::last_os_error());
    }
    if e.raw_os_error() == Some(33) {
        return Err(io::ErrorKind::WouldBlock.into());
    }
    Err(e)
}
pub(super) fn unlock_range(f: &File, at: u64, len: u64) -> io::Result<()> {
    let mut request = overlapped(at, ptr::null_mut())?;
    if unsafe {
        UnlockFileEx(
            f.as_raw_handle(),
            0,
            len as u32,
            (len >> 32) as u32,
            &mut request,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
pub(super) fn page_size() -> io::Result<usize> {
    let mut info = MaybeUninit::<SystemInfo>::uninit();
    unsafe { GetSystemInfo(info.as_mut_ptr()) };
    let n = unsafe { info.assume_init() }.page as usize;
    if n.is_power_of_two() {
        Ok(n)
    } else {
        Err(io::ErrorKind::InvalidData.into())
    }
}
pub(super) unsafe fn flush_view(p: *mut u8, n: usize) -> io::Result<()> {
    if unsafe { FlushViewOfFile(p.cast(), n) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
pub(super) unsafe fn lock_memory(p: *const u8, n: usize) -> io::Result<()> {
    if unsafe { VirtualLock(p.cast(), n) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
pub(super) unsafe fn unlock_memory(p: *const u8, n: usize) -> io::Result<()> {
    if unsafe { VirtualUnlock(p.cast(), n) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

// The target ABI is checked even on a cross-check without a Windows linker.
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(mem::size_of::<Overlapped>() == 32);
    assert!(mem::size_of::<ObjectAttributes>() == 48);
    assert!(mem::size_of::<IoStatus>() == 16);
    assert!(mem::size_of::<IdInfo>() == 24);
    assert!(mem::size_of::<StandardInfo>() == 24);
    assert!(mem::size_of::<BasicInfo>() == 40);
    assert!(mem::size_of::<SystemInfo>() == 48);
};
pub(super) fn mark_sparse(file: &File) -> io::Result<()> {
    let mut transferred = 0;
    if unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            0x000900c4,
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            0,
            &mut transferred,
            ptr::null_mut(),
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[repr(C)]
struct SecurityAttributes {
    length: u32,
    descriptor: *mut c_void,
    inherit: i32,
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        sddl: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        size: *mut u32,
    ) -> i32;
    fn SetKernelObjectSecurity(handle: Handle, information: u32, descriptor: *mut c_void) -> i32;
}
struct Descriptor(*mut c_void);
impl Descriptor {
    fn private(writable: bool, directory: bool) -> io::Result<Self> {
        // OW is the real Owner Rights SID; a protected DACL denies inherited
        // group/everyone access. The owner is assigned by the creation token.
        let sddl = if directory {
            "D:P(A;OICI;FA;;;OW)"
        } else if writable {
            "D:P(A;;FA;;;OW)"
        } else {
            "D:P(A;;0x0016008d;;;OW)"
        };
        let sddl = wide(OsStr::new(sddl))?;
        let mut output = ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut output,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if output.is_null() {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(Self(output))
    }
    fn attributes(&self) -> SecurityAttributes {
        SecurityAttributes {
            length: mem::size_of::<SecurityAttributes>() as u32,
            descriptor: self.0,
            inherit: 0,
        }
    }
}
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
pub(super) fn open_readonly(path: &Path) -> io::Result<File> {
    let name = wide(path.as_os_str())?;
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ,
            SHARE,
            ptr::null_mut(),
            3,
            OPEN_REPARSE,
            ptr::null_mut(),
        )
    };
    if handle == INVALID {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_handle(handle) };
    let identity = identity(&file)?;
    if !identity.regular || identity.reparse {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(file)
}
pub(super) fn create_private_file(path: &Path, read: bool) -> io::Result<File> {
    let descriptor = Descriptor::private(true, false)?;
    let mut attributes = descriptor.attributes();
    let name = wide(path.as_os_str())?;
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE | 0x40000 | 0x80 | if read { GENERIC_READ } else { 0 },
            SHARE,
            (&mut attributes as *mut SecurityAttributes).cast(),
            1,
            OPEN_REPARSE,
            ptr::null_mut(),
        )
    };
    if handle == INVALID {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_handle(handle) };
    let id = identity(&file)?;
    if !id.regular || id.reparse {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(file)
}
pub(super) fn create_private_directory(path: &Path) -> io::Result<()> {
    let descriptor = Descriptor::private(true, true)?;
    let mut attributes = descriptor.attributes();
    let path = wide(path.as_os_str())?;
    if unsafe { CreateDirectoryW(path.as_ptr(), &mut attributes) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
pub(super) fn set_private_permissions(file: &File, writable: bool) -> io::Result<()> {
    let descriptor = Descriptor::private(writable, false)?;
    // Sealed owners keep read+APPEND_DATA (for flush only)+WRITE_DAC (the
    // Windows equivalent of an owner being able to chmod a POSIX0400 file).
    // WRITE_DATA is absent: ordinary write/truncate opens remain denied.
    if unsafe { SetKernelObjectSecurity(file.as_raw_handle(), 4 | 0x80000000, descriptor.0) } != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(5) {
        return Err(error);
    }
    // std-created writable Files need not have requested WRITE_DAC. ReOpenFile
    // grants a separate control-only handle to that exact object, not a path.
    let handle = unsafe { ReOpenFile(file.as_raw_handle(), 0x40000 | 0x20000 | 0x80, SHARE, 0) };
    if handle == INVALID {
        return Err(io::Error::last_os_error());
    }
    let owner = unsafe { File::from_raw_handle(handle) };
    if unsafe { SetKernelObjectSecurity(owner.as_raw_handle(), 4 | 0x80000000, descriptor.0) } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(super) fn path_identity(path: &Path) -> io::Result<FileIdentity> {
    let name = wide(path.as_os_str())?;
    // Metadata-only access does not open a pipe for data or follow reparse
    // targets. BACKUP_SEMANTICS admits real directories on the same path.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            0x80,
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
    let file = unsafe { File::from_raw_handle(handle) };
    identity(&file)
}

pub(super) fn module_path(address: *const c_void) -> io::Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    if address.is_null() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut module = ptr::null_mut();
    // FROM_ADDRESS|UNCHANGED_REFCOUNT: inspect the caller's already-live module.
    if unsafe { GetModuleHandleExW(4 | 2, address.cast(), &mut module) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut length = 256;
    loop {
        let mut buffer = vec![0u16; length];
        let used = unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), length as u32) };
        if used == 0 {
            return Err(io::Error::last_os_error());
        }
        if (used as usize) < length {
            return Ok(std::ffi::OsString::from_wide(&buffer[..used as usize]).into());
        }
        length = length
            .checked_mul(2)
            .filter(|n| *n <= 65536)
            .ok_or(io::ErrorKind::InvalidData)?;
    }
}

pub(super) fn path_from_utf8(bytes: &[u8]) -> io::Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    if bytes.is_empty() {
        return Ok(std::path::PathBuf::new());
    }
    let length = i32::try_from(bytes.len()).map_err(|_| io::Error::from_raw_os_error(206))?;
    let needed =
        unsafe { MultiByteToWideChar(65001, 0, bytes.as_ptr(), length, ptr::null_mut(), 0) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    if needed > 32767 {
        return Err(io::Error::from_raw_os_error(206));
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(needed as usize)
        .map_err(|_| io::ErrorKind::OutOfMemory)?;
    output.resize(needed as usize, 0);
    let used = unsafe {
        MultiByteToWideChar(
            65001,
            0,
            bytes.as_ptr(),
            length,
            output.as_mut_ptr(),
            needed,
        )
    };
    if used != needed {
        return Err(io::Error::last_os_error());
    }
    Ok(std::ffi::OsString::from_wide(&output).into())
}

#[repr(C)]
struct ZeroData {
    first: i64,
    end: i64,
}
pub(super) fn initialize_zeroed_file(file: &File, length: u64) -> io::Result<()> {
    // A synchronous independent object keeps DeviceIoControl's stack output
    // valid to completion even if an arbitrary caller supplied an async File.
    let handle =
        unsafe { ReOpenFile(file.as_raw_handle(), GENERIC_READ | GENERIC_WRITE, SHARE, 0) };
    if handle == INVALID {
        return Err(io::Error::last_os_error());
    }
    let owner = unsafe { File::from_raw_handle(handle) };
    mark_sparse(&owner)?;
    owner.set_len(length)?;
    if length == 0 {
        return Ok(());
    }
    let mut zero = ZeroData {
        first: 0,
        end: length as i64,
    };
    let mut transferred = 0;
    // SetEndOfFile alone leaves newly extended bytes undefined by contract.
    // FSCTL_SET_ZERO_DATA guarantees their value; sparse backing avoids eager
    // allocation or userspace touching of multi-GiB private capacities.
    if unsafe {
        DeviceIoControl(
            owner.as_raw_handle(),
            0x000980c8,
            (&mut zero as *mut ZeroData).cast(),
            mem::size_of::<ZeroData>() as u32,
            ptr::null_mut(),
            0,
            &mut transferred,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn sync_file(file: &File) -> io::Result<()> {
    durability::sync_file(file)
}
pub(super) fn sync_directory_file(file: &File) -> io::Result<()> {
    durability::sync_directory_file(file)
}
pub(super) fn sync_directory(path: &Path) -> io::Result<()> {
    durability::sync_directory_file(&open_directory(path)?)
}
pub(super) fn rename_replace(from: &Path, to: &Path) -> io::Result<()> {
    durability::rename_replace(from, to)
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThreadId() -> u32;
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtSetInformationFile(
        file: Handle,
        status: *mut IoStatus,
        information: *mut c_void,
        length: u32,
        class: u32,
    ) -> i32;
}
pub(super) fn current_thread_id() -> u32 {
    unsafe { GetCurrentThreadId() }
}
pub(super) fn remove_owned_file(file: &File) -> io::Result<()> {
    if !identity(file)?.is_file() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    // This handle was opened with DELETE. Select that exact object, never a
    // second pathname lookup that could remove a replacement lease.
    let mut status = IoStatus {
        status: 0,
        information: 0,
    };
    let mut disposition = 3u32; // DELETE | POSIX_SEMANTICS
    let result = unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut status,
            (&mut disposition as *mut u32).cast(),
            4,
            64,
        )
    };
    if result == 0x103 {
        if unsafe { WaitForSingleObject(file.as_raw_handle(), u32::MAX) } != 0 {
            std::process::abort();
        }
    }
    let result = if result == 0x103 {
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
