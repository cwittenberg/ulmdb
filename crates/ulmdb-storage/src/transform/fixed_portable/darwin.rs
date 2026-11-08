//! Mach READ memory entries, minted only after dropping all writable authority.
#![allow(unsafe_code)]
use crate::{Error, Result};
use std::ptr::NonNull;

type Port = u32;
unsafe extern "C" {
    fn mach_vm_allocate(task: Port, address: *mut u64, size: u64, flags: i32) -> i32;
    fn mach_vm_deallocate(task: Port, address: u64, size: u64) -> i32;
    fn mach_vm_protect(task: Port, address: u64, size: u64, maximum: u32, protection: i32) -> i32;
    fn mach_vm_inherit(task: Port, address: u64, size: u64, inheritance: u32) -> i32;
    fn mach_make_memory_entry_64(
        task: Port,
        size: *mut u64,
        offset: u64,
        permission: i32,
        object: *mut Port,
        parent: Port,
    ) -> i32;
    fn mach_port_deallocate(task: Port, name: Port) -> i32;
}
fn task() -> Port {
    unsafe { libc::mach_task_self() }
}
fn checked(code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("Mach decoded memory operation: {code}")).into())
    }
}
fn rounded(length: usize) -> Result<usize> {
    let page = ulmdb_platform::page_size()?;
    length
        .checked_add(page - 1)
        .map(|n| n / page * page)
        .filter(|n| *n > 0)
        .ok_or(Error::LimitExceeded)
}
fn creator(pid: u32) -> Result<()> {
    if pid == std::process::id() {
        Ok(())
    } else {
        Err(Error::InvalidInput("inherited Mach decoded object"))
    }
}
struct Region {
    pointer: NonNull<u8>,
    span: usize,
    creator: u32,
    charge: super::Charge,
}
impl Drop for Region {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            if unsafe { mach_vm_deallocate(task(), self.pointer.as_ptr() as u64, self.span as u64) }
                != 0
            {
                // A failed unmap retains its physical-memory charge forever;
                // releasing it while the VMA survives would underreport usage.
                std::mem::forget(self.charge.clone());
            }
        }
    }
}
struct MemoryEntry {
    port: Port,
    creator: u32,
}
impl Drop for MemoryEntry {
    fn drop(&mut self) {
        if self.creator == std::process::id() && self.port != 0 {
            unsafe {
                mach_port_deallocate(task(), self.port);
            }
        }
    }
}
pub(super) struct Writer {
    region: Region,
    length: usize,
}
impl Writer {
    pub(super) fn new(length: usize, charge: super::Charge) -> Result<Self> {
        let span = rounded(length)?;
        let mut address = 0;
        checked(unsafe {
            mach_vm_allocate(task(), &mut address, span as u64, libc::VM_FLAGS_ANYWHERE)
        })?;
        let pointer = NonNull::new(address as *mut u8).ok_or(Error::LimitExceeded)?;
        let region = Region {
            pointer,
            span,
            creator: std::process::id(),
            charge,
        };
        // Private staging is never inherited as a writable alias by fork.
        checked(unsafe {
            mach_vm_inherit(
                task(),
                address,
                span as u64,
                libc::VM_INHERIT_NONE as libc::vm_inherit_t,
            )
        })?;
        Ok(Self { region, length })
    }
    pub(super) fn write_at(&mut self, start: usize, bytes: &[u8]) -> Result<()> {
        creator(self.region.creator)?;
        if start
            .checked_add(bytes.len())
            .is_none_or(|end| end > self.length)
        {
            return Err(Error::InvalidInput("decoded staging write extent"));
        }
        // The only writable mapping is private to this unique builder.
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
        creator(self.region.creator)?;
        let address = self.region.pointer.as_ptr() as u64;
        let span = self.region.span;
        // Reduce maximum protection as well as current protection; WRITE cannot
        // be restored even through the original staging address.
        checked(unsafe { mach_vm_protect(task(), address, span as u64, 1, libc::VM_PROT_READ) })?;
        let refused = unsafe {
            mach_vm_protect(
                task(),
                address,
                span as u64,
                0,
                libc::VM_PROT_READ | libc::VM_PROT_WRITE,
            )
        };
        if refused == 0 {
            return Err(Error::Corrupt("Mach maximum READ protection not enforced"));
        }
        if refused != libc::KERN_PROTECTION_FAILURE {
            checked(refused)?;
        }
        let mut size = span as u64;
        let mut port = 0;
        checked(unsafe {
            mach_make_memory_entry_64(task(), &mut size, address, libc::VM_PROT_READ, &mut port, 0)
        })?;
        let entry = MemoryEntry {
            port,
            creator: self.region.creator,
        };
        if port == 0 || size != span as u64 {
            return Err(Error::Corrupt("Mach decoded entry extent"));
        }
        // READ entry cannot issue an RW alias. Refuse the capability if that
        // kernel invariant fails; there is no permission-only fallback.
        let mut writable = 0;
        let result = unsafe {
            libc::mach_vm_map(
                task(),
                &mut writable,
                span as u64,
                0,
                libc::VM_FLAGS_ANYWHERE,
                port,
                0,
                0,
                libc::VM_PROT_READ | libc::VM_PROT_WRITE,
                libc::VM_PROT_READ | libc::VM_PROT_WRITE,
                libc::VM_INHERIT_NONE as libc::vm_inherit_t,
            )
        };
        if result == 0 {
            unsafe {
                mach_vm_deallocate(task(), writable, span as u64);
            }
            return Err(Error::Corrupt("Mach READ entry permitted writable mapping"));
        }
        // XNU vm_map_enter_mem_object returns KERN_INVALID_RIGHT when an
        // entry's READ permission excludes requested current/maximum WRITE.
        if !matches!(
            result,
            libc::KERN_PROTECTION_FAILURE | libc::KERN_INVALID_RIGHT
        ) {
            checked(result)?;
        }
        // Explicitly remove staging before exposing the immutable token.
        let charge = self.region.charge.clone();
        drop(self.region);
        Ok(Object {
            entry,
            span,
            length: self.length,
            charge,
        })
    }
}
pub(super) struct Object {
    entry: MemoryEntry,
    span: usize,
    length: usize,
    charge: super::Charge,
}
impl Object {
    fn map_raw(&self, address: &mut u64, flags: i32) -> Result<()> {
        creator(self.entry.creator)?;
        checked(unsafe {
            libc::mach_vm_map(
                task(),
                address,
                self.span as u64,
                0,
                flags,
                self.entry.port,
                0,
                0,
                libc::VM_PROT_READ,
                libc::VM_PROT_READ,
                libc::VM_INHERIT_NONE as libc::vm_inherit_t,
            )
        })
    }
    pub(super) fn map(&self) -> Result<View> {
        let mut address = 0;
        self.map_raw(&mut address, libc::VM_FLAGS_ANYWHERE)?;
        let pointer = NonNull::new(address as *mut u8).ok_or(Error::LimitExceeded)?;
        Ok(View {
            region: Region {
                pointer,
                span: self.span,
                creator: self.entry.creator,
                charge: self.charge.clone(),
            },
            length: self.length,
        })
    }
    pub(super) unsafe fn map_at(&self, address: usize) -> Result<()> {
        creator(self.entry.creator)?;
        if address == 0
            || !address.is_multiple_of(ulmdb_platform::page_size()?)
            || address.checked_add(self.span).is_none()
        {
            return Err(Error::InvalidInput("decoded fixed destination"));
        }
        let mut actual = address as u64;
        self.map_raw(&mut actual, libc::VM_FLAGS_FIXED | libc::VM_FLAGS_OVERWRITE)?;
        if actual != address as u64 {
            unsafe {
                mach_vm_deallocate(task(), actual, self.span as u64);
            }
            return Err(Error::Corrupt("Mach fixed decoded address changed"));
        }
        Ok(())
    }
}
pub(super) struct View {
    region: Region,
    length: usize,
}
// SAFETY: sealed READ-only mappings never mutate and the unique view owns unmap.
unsafe impl Send for View {}
unsafe impl Sync for View {}
impl View {
    pub(super) fn bytes(&self) -> &[u8] {
        assert_eq!(
            self.region.creator,
            std::process::id(),
            "inherited Mach decoded view"
        );
        unsafe { std::slice::from_raw_parts(self.region.pointer.as_ptr(), self.length) }
    }
}
