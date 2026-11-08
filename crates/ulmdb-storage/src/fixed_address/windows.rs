//! Windows10 1803+ placeholders retain address ownership across view changes.
//! No release/reserve fallback exists. Each split/replacement/restoration is
//! serialized and recorded; failed teardown retains the outer VMA resources.
#![allow(unsafe_code)]
use crate::creator_owned::CreatorOwned;
use std::{
    collections::BTreeMap,
    ffi::c_void,
    fs::File,
    io,
    os::windows::io::{AsRawHandle, BorrowedHandle},
    ptr::{self, NonNull},
    sync::{Mutex, OnceLock},
};
type Handle = *mut c_void;
const RESERVE: u32 = 0x2000;
const RELEASE: u32 = 0x8000;
const RESERVE_PLACEHOLDER: u32 = 0x40000;
const REPLACE_PLACEHOLDER: u32 = 0x4000;
const PRESERVE_PLACEHOLDER: u32 = 2;
const COALESCE_PLACEHOLDERS: u32 = 1;
const NOACCESS: u32 = 1;
const READONLY: u32 = 2;
const READWRITE: u32 = 4;
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
type Alloc = unsafe extern "system" fn(
    Handle,
    *mut c_void,
    usize,
    u32,
    u32,
    *mut c_void,
    u32,
) -> *mut c_void;
type Map = unsafe extern "system" fn(
    Handle,
    Handle,
    *mut c_void,
    u64,
    usize,
    u32,
    u32,
    *mut c_void,
    u32,
) -> *mut c_void;
type Unmap = unsafe extern "system" fn(Handle, *mut c_void, u32) -> i32;
#[derive(Clone, Copy)]
struct Api {
    alloc: Alloc,
    map: Map,
    unmap: Unmap,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Handle;
    fn GetProcAddress(module: Handle, name: *const u8) -> *mut c_void;
    fn GetCurrentProcess() -> Handle;
    fn GetSystemInfo(info: *mut SystemInfo);
    fn VirtualFree(address: *mut c_void, size: usize, kind: u32) -> i32;
    fn VirtualLock(address: *const c_void, size: usize) -> i32;
    fn VirtualUnlock(address: *const c_void, size: usize) -> i32;
    fn CreateFileMappingW(
        file: Handle,
        security: *mut c_void,
        protection: u32,
        high: u32,
        low: u32,
        name: *const u16,
    ) -> Handle;
    fn CloseHandle(handle: Handle) -> i32;
    fn FlushViewOfFile(address: *const c_void, length: usize) -> i32;
}
fn api() -> io::Result<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        for name in ["KernelBase.dll", "kernel32.dll"] {
            let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
            let module = GetModuleHandleW(name.as_ptr());
            if module.is_null() {
                continue;
            }
            let alloc = GetProcAddress(module, c"VirtualAlloc2".as_ptr().cast());
            let map = GetProcAddress(module, c"MapViewOfFile3".as_ptr().cast());
            let unmap = GetProcAddress(module, c"UnmapViewOfFile2".as_ptr().cast());
            if !alloc.is_null() && !map.is_null() && !unmap.is_null() {
                return Some(Api {
                    alloc: std::mem::transmute::<*mut c_void, Alloc>(alloc),
                    map: std::mem::transmute::<*mut c_void, Map>(map),
                    unmap: std::mem::transmute::<*mut c_void, Unmap>(unmap),
                });
            }
        }
        None
    })
    .as_ref()
    .ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows fixed mapping requires placeholder APIs (Windows10 1803+)",
        )
    })
}
pub(super) fn page() -> io::Result<usize> {
    let mut info = std::mem::MaybeUninit::<SystemInfo>::uninit();
    unsafe {
        GetSystemInfo(info.as_mut_ptr());
    }
    let n = unsafe { info.assume_init() }.page as usize;
    if n.is_power_of_two() {
        Ok(n)
    } else {
        Err(io::ErrorKind::Unsupported.into())
    }
}
#[derive(Clone, Copy)]
struct Part {
    end: usize,
    mapped: bool,
}
pub(super) struct Window {
    base: NonNull<u8>,
    length: usize,
    creator: u32,
    parts: CreatorOwned<Mutex<BTreeMap<usize, Part>>>,
}
struct Section(Handle);
impl Drop for Section {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
impl Window {
    pub(super) fn reserve(requested: Option<usize>, length: usize) -> io::Result<Self> {
        let api = api()?;
        let p = unsafe {
            (api.alloc)(
                ptr::null_mut(),
                requested.unwrap_or(0) as *mut _,
                length,
                RESERVE | RESERVE_PLACEHOLDER,
                NOACCESS,
                ptr::null_mut(),
                0,
            )
        };
        if p.is_null() {
            let e = io::Error::last_os_error();
            return Err(
                if requested.is_some() && matches!(e.raw_os_error(), Some(487 | 183)) {
                    io::ErrorKind::AddrInUse.into()
                } else {
                    e
                },
            );
        }
        if requested.is_some_and(|n| n != p as usize) || (p as usize).checked_add(length).is_none()
        {
            unsafe { VirtualFree(p, 0, RELEASE) };
            return Err(io::ErrorKind::AddrInUse.into());
        }
        Ok(Self {
            base: NonNull::new(p.cast()).ok_or(io::ErrorKind::AddrInUse)?,
            length,
            creator: std::process::id(),
            parts: CreatorOwned::new(Mutex::new(BTreeMap::from([(
                0,
                Part {
                    end: length,
                    mapped: false,
                },
            )]))),
        })
    }
    pub(super) fn address(&self) -> NonNull<u8> {
        self.base
    }
    fn check(&self) -> io::Result<()> {
        if self.creator != std::process::id() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "inherited Windows fixed window",
            ));
        }
        Ok(())
    }
    fn extent(&self, address: *mut u8, length: usize) -> io::Result<(usize, usize)> {
        self.check()?;
        let at = (address as usize)
            .checked_sub(self.base.as_ptr() as usize)
            .ok_or(io::ErrorKind::InvalidInput)?;
        let end = at
            .checked_add(length)
            .filter(|n| *n <= self.length)
            .ok_or(io::ErrorKind::InvalidInput)?;
        Ok((at, end))
    }
    fn isolate(&self, parts: &mut BTreeMap<usize, Part>, at: usize, end: usize) -> io::Result<()> {
        let (&start, first) = parts
            .range(..=at)
            .next_back()
            .ok_or(io::ErrorKind::InvalidData)?;
        if first.mapped {
            return Err(io::ErrorKind::AddrInUse.into());
        }
        let mut stop = first.end;
        // Coalesce adjacent owned placeholders if a previous split/restoration
        // left more than one. Never cross any mapped view or release a hole.
        let mut count = 1;
        while stop < end {
            let next = parts
                .get(&stop)
                .filter(|n| !n.mapped)
                .ok_or(io::ErrorKind::AddrInUse)?;
            stop = next.end;
            count += 1;
        }
        if count > 1 {
            if unsafe {
                VirtualFree(
                    self.base.as_ptr().add(start).cast(),
                    stop - start,
                    RELEASE | COALESCE_PLACEHOLDERS,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            let keys: Vec<_> = parts.range(start..stop).map(|(k, _)| *k).collect();
            for k in keys {
                parts.remove(&k);
            }
            parts.insert(
                start,
                Part {
                    end: stop,
                    mapped: false,
                },
            );
        }
        if start < at {
            if unsafe {
                VirtualFree(
                    self.base.as_ptr().add(start).cast(),
                    at - start,
                    RELEASE | PRESERVE_PLACEHOLDER,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            parts.insert(
                start,
                Part {
                    end: at,
                    mapped: false,
                },
            );
            parts.insert(
                at,
                Part {
                    end: stop,
                    mapped: false,
                },
            );
        }
        if end < stop {
            if unsafe {
                VirtualFree(
                    self.base.as_ptr().add(at).cast(),
                    end - at,
                    RELEASE | PRESERVE_PLACEHOLDER,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            parts.insert(at, Part { end, mapped: false });
            parts.insert(
                end,
                Part {
                    end: stop,
                    mapped: false,
                },
            );
        }
        Ok(())
    }
    pub(super) fn map(
        &self,
        file: &File,
        offset: u64,
        address: *mut u8,
        length: usize,
        writable: bool,
    ) -> io::Result<*mut u8> {
        self.extent(address, length)?;
        let protection = if writable { READWRITE } else { READONLY };
        let handle = unsafe {
            CreateFileMappingW(
                file.as_raw_handle(),
                ptr::null_mut(),
                protection,
                0,
                0,
                ptr::null(),
            )
        };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let _section = Section(handle);
        self.map_handle(handle, offset, address, length, protection)
    }
    pub(super) fn map_section(
        &self,
        section: BorrowedHandle<'_>,
        address: *mut u8,
        length: usize,
    ) -> io::Result<*mut u8> {
        self.map_handle(section.as_raw_handle(), 0, address, length, READONLY)
    }
    fn map_handle(
        &self,
        handle: Handle,
        offset: u64,
        address: *mut u8,
        length: usize,
        protection: u32,
    ) -> io::Result<*mut u8> {
        let (at, end) = self.extent(address, length)?;
        let mut parts = self
            .parts
            .lock()
            .map_err(|_| io::Error::other("fixed placeholder state poisoned"))?;
        self.isolate(&mut parts, at, end)?;
        let p = unsafe {
            (api()?.map)(
                handle,
                GetCurrentProcess(),
                address.cast(),
                offset,
                length,
                REPLACE_PLACEHOLDER,
                protection,
                ptr::null_mut(),
                0,
            )
        };
        if p.is_null() {
            return Err(io::Error::last_os_error());
        }
        if p != address.cast() {
            unsafe { (api()?.unmap)(GetCurrentProcess(), p, 0) };
            return Err(io::Error::other("placeholder replacement changed address"));
        }
        parts.insert(at, Part { end, mapped: true });
        Ok(p.cast())
    }
    pub(super) fn restore(&self, address: *mut u8, length: usize) -> io::Result<()> {
        let (at, end) = self.extent(address, length)?;
        let mut parts = self
            .parts
            .lock()
            .map_err(|_| io::Error::other("fixed placeholder state poisoned"))?;
        if !parts.get(&at).is_some_and(|p| p.end == end && p.mapped) {
            return Err(io::ErrorKind::InvalidData.into());
        }
        if unsafe { (api()?.unmap)(GetCurrentProcess(), address.cast(), PRESERVE_PLACEHOLDER) } == 0
        {
            return Err(io::Error::last_os_error());
        }
        parts.insert(at, Part { end, mapped: false });
        Ok(())
    }
    pub(super) fn release(&self) -> io::Result<()> {
        self.check()?;
        let mut parts = self
            .parts
            .lock()
            .map_err(|_| io::Error::other("fixed placeholder state poisoned"))?;
        while let Some((&at, part)) = parts.first_key_value() {
            let pointer = unsafe { self.base.as_ptr().add(at) };
            let rc = if part.mapped {
                unsafe { (api()?.unmap)(GetCurrentProcess(), pointer.cast(), 0) }
            } else {
                unsafe { VirtualFree(pointer.cast(), 0, RELEASE) }
            };
            if rc == 0 {
                return Err(io::Error::last_os_error());
            }
            parts.remove(&at);
        }
        Ok(())
    }
}
pub(super) fn random(_pointer: *mut u8, _length: usize) -> io::Result<()> {
    // LMDB accepts NORDAHEAD on Windows and applies madvise only on Unix.
    Ok(())
}
pub(super) fn lock(pointer: *mut u8, length: usize) -> io::Result<()> {
    if unsafe { VirtualLock(pointer.cast(), length) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
pub(super) fn unlock(pointer: *mut u8, length: usize) -> io::Result<()> {
    if unsafe { VirtualUnlock(pointer.cast(), length) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
pub(super) fn flush(
    pointer: *mut u8,
    length: usize,
    asynchronous: bool,
    file: Option<&File>,
) -> io::Result<()> {
    if unsafe { FlushViewOfFile(pointer.cast(), length) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if !asynchronous {
        ulmdb_platform::sync_file(
            file.ok_or_else(|| io::Error::other("fixed writer flush handle absent"))?,
        )?;
    }
    Ok(())
}
