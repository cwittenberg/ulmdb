//! Unnamed pagefile sections; no writable capability survives publication.
//! SEC_NO_CHANGE is required and its actual denial behavior is checked before
//! the immutable token is minted. Unsupported kernels fail closed.
#![allow(unsafe_code)]
use crate::{Error, Result};
use std::{
    ffi::c_void,
    os::windows::io::{BorrowedHandle, RawHandle},
    ptr::NonNull,
};
type HandleValue = *mut c_void;
const READ: u32 = 2;
const WRITE: u32 = 4;
const SECTION_QUERY: u32 = 1;
const SECTION_MAP_WRITE: u32 = 2;
const SECTION_MAP_READ: u32 = 4;
const WRITE_DAC: u32 = 0x0004_0000;
const SEC_COMMIT: u32 = 0x0800_0000;
const SEC_NO_CHANGE: u32 = 0x0040_0000;
const ACCESS_DENIED: i32 = 0xc000_0022u32 as i32;
const INVALID_PAGE_PROTECTION: i32 = 0xc000_0045u32 as i32;
const SECTION_PROTECTION: i32 = 0xc000_004eu32 as i32;
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateSection(
        section: *mut HandleValue,
        access: u32,
        attributes: *const c_void,
        maximum: *const i64,
        protection: u32,
        allocation: u32,
        file: HandleValue,
    ) -> i32;
    fn NtMapViewOfSection(
        section: HandleValue,
        process: HandleValue,
        address: *mut HandleValue,
        zero_bits: usize,
        commit: usize,
        offset: *const i64,
        size: *mut usize,
        inherit: u32,
        allocation: u32,
        protection: u32,
    ) -> i32;
    fn NtUnmapViewOfSection(process: HandleValue, address: HandleValue) -> i32;
    fn NtProtectVirtualMemory(
        process: HandleValue,
        address: *mut HandleValue,
        size: *mut usize,
        protection: u32,
        old: *mut u32,
    ) -> i32;
    fn NtDuplicateObject(
        source_process: HandleValue,
        source: HandleValue,
        target_process: HandleValue,
        target: *mut HandleValue,
        access: u32,
        attributes: u32,
        options: u32,
    ) -> i32;
    fn NtClose(handle: HandleValue) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        text: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        bytes: *mut u32,
    ) -> i32;
    fn GetSecurityDescriptorDacl(
        descriptor: *const c_void,
        present: *mut i32,
        acl: *mut *mut c_void,
        defaulted: *mut i32,
    ) -> i32;
    fn SetSecurityInfo(
        handle: HandleValue,
        kind: i32,
        information: u32,
        owner: *mut c_void,
        group: *mut c_void,
        dacl: *mut c_void,
        sacl: *mut c_void,
    ) -> u32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}

fn seal_access(handle: &Handle) -> Result<()> {
    creator(handle.creator)?;
    // Explicit OWNER_RIGHTS disables the owner's implicit WRITE_DAC. Deny
    // WRITE/EXTEND/EXECUTE and security edits, including handle re-duplication
    // with greater rights; keep READ|QUERY and READ_CONTROL only.
    const SDDL: &[u8] = b"D:P(D;;0x000D003A;;;WD)(A;;0x00020005;;;OW)(A;;0x00020005;;;WD)\0";
    let mut wide = [0u16; SDDL.len()];
    for (out, byte) in wide.iter_mut().zip(SDDL) {
        *out = u16::from(*byte);
    }
    let mut raw = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            1,
            &mut raw,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    struct Descriptor(*mut c_void);
    impl Drop for Descriptor {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    let descriptor = Descriptor(raw);
    let mut acl = std::ptr::null_mut();
    let mut present = 0;
    let mut defaulted = 0;
    if unsafe { GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut acl, &mut defaulted) }
        == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    if present == 0 || acl.is_null() {
        return Err(Error::Corrupt("decoded section DACL absent"));
    }
    // SE_KERNEL_OBJECT; protected DACL only, no global/process ACL changes.
    let error = unsafe {
        SetSecurityInfo(
            handle.raw,
            6,
            0x8000_0004,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            acl,
            std::ptr::null_mut(),
        )
    };
    if error != 0 {
        return Err(std::io::Error::from_raw_os_error(error as i32).into());
    }
    Ok(())
}
fn deny_duplicate(handle: &Handle, access: u32) -> Result<()> {
    creator(handle.creator)?;
    let mut duplicate = std::ptr::null_mut();
    let status = unsafe {
        NtDuplicateObject(
            process(),
            handle.raw,
            process(),
            &mut duplicate,
            access,
            0,
            0,
        )
    };
    if status >= 0 {
        if !duplicate.is_null() {
            drop(Handle {
                raw: duplicate,
                creator: handle.creator,
            });
        }
        return Err(Error::Corrupt(
            "decoded section handle rights could be increased",
        ));
    }
    if status != ACCESS_DENIED {
        checked(status)?;
    }
    Ok(())
}
fn process() -> HandleValue {
    -1isize as HandleValue
}
fn checked(code: i32) -> Result<()> {
    if code >= 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(code) } as i32).into())
    }
}
fn creator(pid: u32) -> Result<()> {
    if pid == std::process::id() {
        Ok(())
    } else {
        Err(Error::InvalidInput("inherited decoded section"))
    }
}
struct Handle {
    raw: HandleValue,
    creator: u32,
}
// SAFETY: only a READ|QUERY handle is placed in the immutable Object; handles
// have no thread affinity and all operations reject a different process first.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        if self.creator == std::process::id() && !self.raw.is_null() {
            unsafe {
                NtClose(self.raw);
            }
        }
    }
}
struct Region {
    pointer: NonNull<u8>,
    span: usize,
    creator: u32,
    charge: super::Charge,
    mapped: bool,
}
impl Region {
    fn unmap(&mut self) -> Result<()> {
        creator(self.creator)?;
        if self.mapped {
            checked(unsafe { NtUnmapViewOfSection(process(), self.pointer.as_ptr().cast()) })?;
            self.mapped = false;
        }
        Ok(())
    }
}
impl Drop for Region {
    fn drop(&mut self) {
        if self.creator == std::process::id() && self.unmap().is_err() {
            // Keep the decoded charge if the kernel retains the actual VMA.
            std::mem::forget(self.charge.clone());
        }
    }
}
fn map(handle: &Handle, span: usize, protection: u32, charge: super::Charge) -> Result<Region> {
    creator(handle.creator)?;
    let mut address = std::ptr::null_mut();
    let mut size = span;
    checked(unsafe {
        NtMapViewOfSection(
            handle.raw,
            process(),
            &mut address,
            0,
            0,
            std::ptr::null(),
            &mut size,
            2,
            0,
            protection,
        )
    })?;
    let pointer = NonNull::new(address.cast::<u8>()).ok_or(Error::Corrupt("null section view"))?;
    let region = Region {
        pointer,
        span: size,
        creator: handle.creator,
        charge,
        mapped: true,
    };
    if size != span {
        return Err(Error::Corrupt("decoded section view extent"));
    }
    Ok(region)
}
pub(super) struct Writer {
    handle: Handle,
    region: Region,
    length: usize,
}
impl Writer {
    pub(super) fn new(length: usize, charge: super::Charge) -> Result<Self> {
        let page = ulmdb_platform::page_size()?;
        let span = length
            .checked_add(page - 1)
            .map(|n| n / page * page)
            .filter(|n| *n > 0)
            .ok_or(Error::LimitExceeded)?;
        let maximum = i64::try_from(span).map_err(|_| Error::LimitExceeded)?;
        let mut raw = std::ptr::null_mut();
        checked(unsafe {
            NtCreateSection(
                &mut raw,
                SECTION_QUERY | SECTION_MAP_READ | SECTION_MAP_WRITE | WRITE_DAC,
                std::ptr::null(),
                &maximum,
                WRITE,
                SEC_COMMIT | SEC_NO_CHANGE,
                std::ptr::null_mut(),
            )
        })?;
        if raw.is_null() {
            return Err(Error::Corrupt("null decoded section"));
        }
        let handle = Handle {
            raw,
            creator: std::process::id(),
        };
        let region = map(&handle, span, WRITE, charge)?;
        Ok(Self {
            handle,
            region,
            length,
        })
    }
    pub(super) fn write_at(&mut self, start: usize, bytes: &[u8]) -> Result<()> {
        creator(self.handle.creator)?;
        if start
            .checked_add(bytes.len())
            .is_none_or(|end| end > self.length)
        {
            return Err(Error::InvalidInput("decoded section staging extent"));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.region.pointer.as_ptr().add(start),
                bytes.len(),
            );
        }
        Ok(())
    }
    pub(super) fn seal(self) -> Result<Object> {
        creator(self.handle.creator)?;
        let mut raw = std::ptr::null_mut();
        checked(unsafe {
            NtDuplicateObject(
                process(),
                self.handle.raw,
                process(),
                &mut raw,
                SECTION_QUERY | SECTION_MAP_READ,
                0,
                0,
            )
        })?;
        if raw.is_null() {
            return Err(Error::Corrupt("null read-only decoded section"));
        }
        let readonly = Handle {
            raw,
            creator: self.handle.creator,
        };
        seal_access(&self.handle)?;
        let span = self.region.span;
        let charge = self.region.charge.clone();
        // No WRITE handle or staging view survives beyond these drops.
        let mut staging = self.region;
        // A failed writable unmap must not issue an immutable capability.
        staging.unmap()?;
        drop(staging);
        drop(self.handle);
        deny_duplicate(&readonly, SECTION_MAP_WRITE)?;
        deny_duplicate(&readonly, WRITE_DAC)?;
        let probe = map(&readonly, span, READ, charge.clone())?;
        let mut address = probe.pointer.as_ptr().cast();
        let mut size = span;
        let mut old = 0;
        let denied =
            unsafe { NtProtectVirtualMemory(process(), &mut address, &mut size, WRITE, &mut old) };
        if denied >= 0 {
            return Err(Error::Corrupt(
                "SEC_NO_CHANGE did not prohibit writable upgrade",
            ));
        }
        if !matches!(
            denied,
            INVALID_PAGE_PROTECTION | SECTION_PROTECTION | ACCESS_DENIED
        ) {
            checked(denied)?;
        }
        let mut writable = std::ptr::null_mut();
        let mut writable_size = span;
        let denied = unsafe {
            NtMapViewOfSection(
                readonly.raw,
                process(),
                &mut writable,
                0,
                0,
                std::ptr::null(),
                &mut writable_size,
                2,
                0,
                WRITE,
            )
        };
        if denied >= 0 {
            if !writable.is_null() {
                unsafe {
                    NtUnmapViewOfSection(process(), writable);
                }
            }
            return Err(Error::Corrupt(
                "READ section handle permitted writable mapping",
            ));
        }
        if !matches!(denied, ACCESS_DENIED | SECTION_PROTECTION) {
            checked(denied)?;
        }
        drop(probe);
        Ok(Object {
            handle: readonly,
            span,
            length: self.length,
            charge,
        })
    }
}
pub(super) struct Object {
    handle: Handle,
    span: usize,
    length: usize,
    charge: super::Charge,
}
impl Object {
    pub(super) fn section(&self) -> BorrowedHandle<'_> {
        // SAFETY: Object owns this non-inheritable READ|QUERY handle.
        unsafe { BorrowedHandle::borrow_raw(self.handle.raw as RawHandle) }
    }
    pub(super) fn map(&self) -> Result<View> {
        Ok(View {
            region: map(&self.handle, self.span, READ, self.charge.clone())?,
            length: self.length,
        })
    }
}
pub(super) struct View {
    region: Region,
    length: usize,
}
// SAFETY: immutable READ/SEC_NO_CHANGE views; unique ownership controls unmap.
unsafe impl Send for View {}
unsafe impl Sync for View {}
impl View {
    pub(super) fn bytes(&self) -> &[u8] {
        assert_eq!(
            self.region.creator,
            std::process::id(),
            "inherited decoded section view"
        );
        unsafe { std::slice::from_raw_parts(self.region.pointer.as_ptr(), self.length) }
    }
}
