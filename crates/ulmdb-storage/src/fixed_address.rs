//! Owned-address reservations. Unix replaces only its own PROT_NONE window;
//! Windows replaces owned placeholders without ever releasing address space.
//! This layer grants no database, namespace, file-immutability or graph proof.
#![allow(unsafe_code)]
use crate::{Error, Result, creator_owned::CreatorOwned};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
mod posix;
#[cfg(unix)]
use posix as sys;
#[cfg(any(target_os = "macos", windows))]
mod decoded;
#[cfg(windows)]
mod windows;
use std::{
    collections::BTreeMap,
    fs::File,
    ptr::NonNull,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
#[cfg(windows)]
use windows as sys;

pub struct Reservation {
    base: NonNull<u8>,
    length: usize,
    page: usize,
    backend: sys::Window,
    creator: u32,
    slots: CreatorOwned<Mutex<BTreeMap<usize, Record>>>,
    serial: AtomicU64,
    poisoned: AtomicBool,
    #[cfg(test)]
    drop_pause:
        CreatorOwned<Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>>,
}
#[derive(Default)]
struct SlotResources {
    #[cfg(test)]
    drop_probe: Mutex<Option<Box<dyn Send + Sync>>>,
    charges: Mutex<BTreeMap<std::num::NonZeroU64, crate::residency::FixedCharge>>,
    decoded: Mutex<Option<Arc<dyn DecodedBacking>>>,
    #[cfg(any(target_os = "macos", windows))]
    decoded_object: Option<Arc<crate::transform::ReadOnlyDecodedObject>>,
    // V2 layout exclusion survives until actual PROT_NONE restoration, including
    // poisoned teardown retained by the reservation's strong registry record.
    layout: Mutex<Option<ulmdb_platform::RangeLock>>,
}
/// Retain the real immutable source/context owner without coupling the address
/// allocator to one platform's decoded-file implementation.
pub(crate) trait DecodedBacking: Send + Sync {
    #[cfg(target_os = "linux")]
    fn file(&self) -> Arc<File>;
    #[cfg(any(target_os = "macos", windows))]
    fn object(&self) -> Arc<crate::transform::ReadOnlyDecodedObject>;
}
struct LockCounts {
    count: usize,
    healthy: bool,
}
struct Record {
    id: u64,
    end: usize,
    file: Identity,
    file_offset: u64,
    length: usize,
    live: Weak<Mapping>,
    writable: bool,
    // The registry retains charges/backing if reprotection fails on last Drop.
    _resources: Arc<SlotResources>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Identity {
    File {
        device: u64,
        inode: u128,
    },
    #[cfg(any(target_os = "macos", windows))]
    Decoded(usize),
}
struct Mapping {
    reservation: Arc<Reservation>,
    file_offset: u64,
    pointer: NonNull<u8>,
    read_ahead: crate::ReadAhead,
    locks: Mutex<LockCounts>,
    identity: Identity,
    resources: CreatorOwned<Arc<SlotResources>>,
    offset: usize,
    length: usize,
    rounded: usize,
    id: u64,
}
/// Cloning keeps the same existing immutable mapping; it creates no new branch.
#[derive(Clone)]
pub struct ByteLease(Arc<Mapping>);
// SAFETY: the window address never moves. Slot changes hold the registry and
// cannot overlap any live immutable lease. Source-file immutability is an
// explicit unsafe precondition of map_file, just as with memmap2::map.
unsafe impl Send for Reservation {}
unsafe impl Sync for Reservation {}
// SAFETY: these pointers address immutable mappings owned by their reservation;
// live leases exclude replacement, and lock/advice bookkeeping is synchronized.
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

fn file_identity(file: &File) -> Result<(Identity, u64, bool)> {
    #[cfg(unix)]
    {
        let m = file.metadata()?;
        Ok((
            Identity::File {
                device: m.dev(),
                inode: m.ino() as u128,
            },
            m.len(),
            m.is_file(),
        ))
    }
    #[cfg(windows)]
    {
        let m = ulmdb_platform::file_identity(file)?;
        Ok((
            Identity::File {
                device: m.volume,
                inode: m.file_id,
            },
            m.length,
            m.regular && !m.reparse,
        ))
    }
}
fn invalid(message: &'static str) -> Error {
    Error::InvalidInput(message)
}
fn rounded(length: usize, page: usize) -> Result<usize> {
    if length == 0 || length > isize::MAX as usize {
        return Err(Error::LimitExceeded);
    }
    length
        .checked_add(page - 1)
        .map(|n| n / page * page)
        .filter(|n| *n <= isize::MAX as usize)
        .ok_or(Error::LimitExceeded)
}
#[cfg(all(
    any(target_os = "linux", target_os = "macos", windows),
    target_pointer_width = "64"
))]
fn os_page() -> Result<usize> {
    sys::page().map_err(Into::into)
}

impl Reservation {
    pub fn reserve(requested: Option<usize>, length: usize) -> Result<Arc<Self>> {
        #[cfg(not(all(
            any(target_os = "linux", target_os = "macos", windows),
            target_pointer_width = "64"
        )))]
        {
            let _ = (requested, length);
            return Err(invalid("fixed mappings require64-bit Linux/macOS/Windows"));
        }
        #[cfg(all(
            any(target_os = "linux", target_os = "macos", windows),
            target_pointer_width = "64"
        ))]
        {
            let page = os_page()?;
            let length = rounded(length, page)?;
            if let Some(base) = requested {
                if base == 0 || !base.is_multiple_of(page) || base.checked_add(length).is_none() {
                    return Err(invalid("unaligned or overflowing fixed address"));
                }
            }
            let backend = sys::Window::reserve(requested, length).map_err(|e| {
                if e.kind() == std::io::ErrorKind::AddrInUse {
                    Error::Busy
                } else {
                    e.into()
                }
            })?;
            Ok(Arc::new(Self {
                base: backend.address(),
                length,
                page,
                backend,
                creator: std::process::id(),
                slots: CreatorOwned::new(Mutex::new(BTreeMap::new())),
                serial: AtomicU64::new(1),
                poisoned: AtomicBool::new(false),
                #[cfg(test)]
                drop_pause: CreatorOwned::new(Mutex::new(None)),
            }))
        }
    }
    pub fn address(&self) -> usize {
        self.base.as_ptr() as usize
    }
    pub fn len(&self) -> usize {
        self.length
    }
    pub fn is_empty(&self) -> bool {
        false
    }
    pub fn check_creator(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(invalid("inherited fixed address reservation"));
        }
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        Ok(())
    }
    pub(crate) fn range_is_live(&self, offset: usize, length: usize) -> Result<bool> {
        self.check_creator()?;
        let end = offset.checked_add(length).ok_or(Error::LimitExceeded)?;
        let slots = self.slots.lock().map_err(|_| Error::OutcomeUnknown)?;
        Ok(slots
            .range(..end)
            .next_back()
            .is_some_and(|(_, record)| record.end > offset))
    }
    /// Map a regular, protected immutable file into a disjoint owned slot.
    ///
    /// # Safety
    /// The engine must prevent writes/truncation of this file while any lease
    /// exists. Regular-file/EOF checks cannot establish that external lifetime
    /// promise. No mutable mapping or data pointer may alias the requested slot.
    pub unsafe fn map_file(
        self: &Arc<Self>,
        file: Arc<File>,
        file_offset: u64,
        region_offset: usize,
        length: usize,
    ) -> Result<ByteLease> {
        unsafe {
            self.map_file_with_read_ahead(
                file,
                file_offset,
                region_offset,
                length,
                crate::ReadAhead::Normal,
            )
        }
    }
    /// Same immutable-file safety contract as map_file; advice is fixed before
    /// any lease is published. A live slot with different advice cannot share.
    pub unsafe fn map_file_with_read_ahead(
        self: &Arc<Self>,
        file: Arc<File>,
        file_offset: u64,
        region_offset: usize,
        length: usize,
        read_ahead: crate::ReadAhead,
    ) -> Result<ByteLease> {
        read_ahead.validate()?;
        self.check_creator()?; // Before the mutex, file IO, or any mapping change.
        #[cfg(not(all(
            any(target_os = "linux", target_os = "macos", windows),
            target_pointer_width = "64"
        )))]
        {
            let _ = (file, file_offset, region_offset, length);
            Err(invalid("fixed mappings require64-bit Linux/macOS/Windows"))
        }
        #[cfg(all(
            any(target_os = "linux", target_os = "macos", windows),
            target_pointer_width = "64"
        ))]
        {
            if !region_offset.is_multiple_of(self.page)
                || !file_offset.is_multiple_of(self.page as u64)
            {
                return Err(invalid("unaligned fixed file mapping"));
            }
            let map_length = rounded(length, self.page)?;
            let end = region_offset
                .checked_add(map_length)
                .filter(|end| *end <= self.length)
                .ok_or(Error::LimitExceeded)?;
            let (identity, file_length, regular) = file_identity(&file)?;
            let file_end = file_offset
                .checked_add(length as u64)
                .ok_or(Error::LimitExceeded)?;
            if !regular || file_end > file_length {
                return Err(invalid("fixed mapping exceeds regular file"));
            }
            if file_offset > i64::MAX as u64 {
                return Err(Error::LimitExceeded);
            }
            let mut registry = self.slots.lock().map_err(|_| Error::OutcomeUnknown)?;
            self.check_creator()?;
            if let Some((&start, entry)) = registry.range(..end).next_back() {
                if entry.end > region_offset {
                    if !entry.writable
                        && start == region_offset
                        && entry.end == end
                        && entry.file == identity
                        && entry.file_offset == file_offset
                        && entry.length == length
                    {
                        if let Some(existing) = entry.live.upgrade() {
                            if existing.read_ahead == read_ahead {
                                return Ok(ByteLease(existing));
                            }
                        }
                    }
                    // A zero strong count can mean Drop is waiting for this mutex.
                    // Keep its authority until that destructor removes the record.
                    return Err(Error::Busy);
                }
            }
            let id = self
                .serial
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .map_err(|_| Error::LimitExceeded)?;
            let address = self.base.as_ptr().wrapping_add(region_offset);
            let pointer = self
                .backend
                .map(&file, file_offset, address, map_length, false)?;
            if pointer != address {
                self.poisoned.store(true, Ordering::Release);
                return Err(Error::OutcomeUnknown);
            }
            if read_ahead == crate::ReadAhead::Random {
                if let Err(error) = sys::random(pointer, map_length) {
                    if self.backend.restore(address, map_length).is_err() {
                        self.poisoned.store(true, Ordering::Release);
                        return Err(Error::OutcomeUnknown);
                    }
                    return Err(error.into());
                }
            }
            let resources = Arc::new(SlotResources::default());
            let mapping = Arc::new(Mapping {
                reservation: self.clone(),
                file_offset,
                pointer: NonNull::new(pointer.cast()).ok_or(Error::OutcomeUnknown)?,
                read_ahead,
                locks: Mutex::new(LockCounts {
                    count: 0,
                    healthy: true,
                }),
                identity,
                resources: CreatorOwned::new(resources.clone()),
                offset: region_offset,
                length,
                rounded: map_length,
                id,
            });
            registry.insert(
                region_offset,
                Record {
                    id,
                    end,
                    file: identity,
                    file_offset,
                    length,
                    live: Arc::downgrade(&mapping),
                    writable: false,
                    _resources: resources,
                },
            );
            Ok(ByteLease(mapping))
        }
    }
}
/// A unique writable mapping of an unpublished, exclusively owned staging
/// inode. It cannot be cloned or shared as an immutable ByteLease. Rust loans
/// follow this handle; callers must end every loan before consuming/dropping it.
pub struct WritableByteLease {
    reservation: Arc<Reservation>,
    pointer: NonNull<u8>,
    offset: usize,
    length: usize,
    rounded: usize,
    id: u64,
    #[cfg(windows)]
    flush_file: File,
}
// SAFETY: address ownership is stable; only &mut self exposes mutable bytes.
// The unsafe constructor's exclusive-inode contract forbids other writers.
unsafe impl Send for WritableByteLease {}
unsafe impl Sync for WritableByteLease {}
impl Reservation {
    /// Map one unique unpublished inode writable inside the reserved window.
    ///
    /// # Safety
    /// The caller exclusively owns the staging inode and forbids every other
    /// mapping/write/truncate for the entire lease and all its slice loans.
    /// No immutable bytes may be exposed until this lease has been dropped.
    pub unsafe fn map_file_mut(
        self: &Arc<Self>,
        file: &File,
        region_offset: usize,
        length: usize,
    ) -> Result<WritableByteLease> {
        self.check_creator()?;
        #[cfg(not(all(
            any(target_os = "linux", target_os = "macos", windows),
            target_pointer_width = "64"
        )))]
        {
            let _ = (file, region_offset, length);
            return Err(invalid("fixed mappings require64-bit Linux/macOS/Windows"));
        }
        #[cfg(all(
            any(target_os = "linux", target_os = "macos", windows),
            target_pointer_width = "64"
        ))]
        {
            if !region_offset.is_multiple_of(self.page) {
                return Err(invalid("unaligned fixed writer"));
            }
            let map_length = rounded(length, self.page)?;
            let end = region_offset
                .checked_add(map_length)
                .filter(|n| *n <= self.length)
                .ok_or(Error::LimitExceeded)?;
            let (identity, file_length, regular) = file_identity(file)?;
            if !regular || file_length < length as u64 {
                return Err(invalid("fixed writer exceeds staging inode"));
            }
            #[cfg(windows)]
            let flush_file = file.try_clone()?;
            let mut registry = self.slots.lock().map_err(|_| Error::OutcomeUnknown)?;
            self.check_creator()?;
            if registry
                .range(..end)
                .next_back()
                .is_some_and(|(_, entry)| entry.end > region_offset)
            {
                return Err(Error::Busy);
            }
            let id = self
                .serial
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .map_err(|_| Error::LimitExceeded)?;
            let address = self.base.as_ptr().wrapping_add(region_offset);
            let pointer = self.backend.map(file, 0, address, map_length, true)?;
            if pointer != address {
                self.poisoned.store(true, Ordering::Release);
                return Err(Error::OutcomeUnknown);
            }
            registry.insert(
                region_offset,
                Record {
                    id,
                    end,
                    file: identity,
                    file_offset: 0,
                    length,
                    live: Weak::new(),
                    writable: true,
                    _resources: Arc::new(SlotResources::default()),
                },
            );
            Ok(WritableByteLease {
                reservation: self.clone(),
                pointer: NonNull::new(pointer.cast()).ok_or(Error::OutcomeUnknown)?,
                offset: region_offset,
                length,
                rounded: map_length,
                id,
                #[cfg(windows)]
                flush_file,
            })
        }
    }
}
impl WritableByteLease {
    pub fn region_offset(&self) -> usize {
        self.offset
    }
    pub fn len(&self) -> usize {
        self.length
    }
    pub fn is_empty(&self) -> bool {
        false
    }
    pub fn as_ptr(&self) -> *const u8 {
        self.pointer.as_ptr()
    }
    pub fn bytes(&self) -> Result<&[u8]> {
        self.reservation.check_creator()?;
        Ok(unsafe { std::slice::from_raw_parts(self.pointer.as_ptr(), self.length) })
    }
    pub(crate) fn retain_layout_lock(&self, lock: ulmdb_platform::RangeLock) -> Result<()> {
        self.reservation.check_creator()?;
        let registry = match self.reservation.slots.lock() {
            Ok(r) => r,
            Err(_) => {
                std::mem::forget(lock);
                self.reservation.poisoned.store(true, Ordering::Release);
                return Err(Error::OutcomeUnknown);
            }
        };
        let Some(record) = registry
            .get(&self.offset)
            .filter(|r| r.id == self.id && r.writable)
        else {
            std::mem::forget(lock);
            self.reservation.poisoned.store(true, Ordering::Release);
            return Err(Error::OutcomeUnknown);
        };
        let mut retained = record
            ._resources
            .layout
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if retained.is_none() {
            *retained = Some(lock);
        }
        Ok(())
    }
    pub fn bytes_mut(&mut self) -> Result<&mut [u8]> {
        self.reservation.check_creator()?;
        Ok(unsafe { std::slice::from_raw_parts_mut(self.pointer.as_ptr(), self.length) })
    }
    pub fn flush_range(&self, length: usize, asynchronous: bool) -> std::io::Result<()> {
        self.reservation
            .check_creator()
            .map_err(std::io::Error::other)?;
        if length == 0 || length > self.length {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "fixed writer flush extent",
            ));
        }
        #[cfg(windows)]
        let file = Some(&self.flush_file);
        #[cfg(not(windows))]
        let file = None;
        sys::flush(self.pointer.as_ptr(), length, asynchronous, file)
    }
}
impl Drop for WritableByteLease {
    fn drop(&mut self) {
        if self.reservation.creator != std::process::id() {
            return;
        }
        let Ok(mut registry) = self.reservation.slots.lock() else {
            self.reservation.poisoned.store(true, Ordering::Release);
            return;
        };
        if registry
            .get(&self.offset)
            .is_none_or(|r| r.id != self.id || !r.writable)
        {
            self.reservation.poisoned.store(true, Ordering::Release);
            return;
        }
        let address = self.pointer.as_ptr();
        if self
            .reservation
            .backend
            .restore(address, self.rounded)
            .is_err()
        {
            self.reservation.poisoned.store(true, Ordering::Release);
            return;
        }
        registry.remove(&self.offset);
    }
}
impl ByteLease {
    pub(crate) fn retain_layout_lock(&self, lock: ulmdb_platform::RangeLock) -> Result<()> {
        self.check_creator()?;
        // Only an Option is changed, with no callbacks/partial object mutation;
        // recovering this small guard after poison preserves the actual lease.
        let mut retained = self
            .0
            .resources
            .layout
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if retained.is_none() {
            *retained = Some(lock);
        }
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.0.length
    }
    pub fn is_empty(&self) -> bool {
        false
    }
    pub fn region_offset(&self) -> usize {
        self.0.offset
    }
    pub fn file_offset(&self) -> u64 {
        self.0.file_offset
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.0.pointer.as_ptr()
    }
    pub fn check_creator(&self) -> Result<()> {
        self.0.reservation.check_creator()
    }
    pub fn advise_random(&self) -> std::io::Result<()> {
        self.check_creator().map_err(std::io::Error::other)?;
        if self.0.read_ahead != crate::ReadAhead::Random {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "fixed mapping advice is immutable",
            ));
        }
        sys::random(self.0.pointer.as_ptr(), self.0.rounded)?;
        Ok(())
    }
    pub(crate) fn required_charges(
        &self,
    ) -> &Mutex<BTreeMap<std::num::NonZeroU64, crate::residency::FixedCharge>> {
        &self.0.resources.charges
    }
    #[cfg(test)]
    pub(crate) fn poison_registry_for_test(&self) {
        let reservation = self.0.reservation.clone();
        let _ = std::panic::catch_unwind(move || {
            let _guard = reservation.slots.lock().unwrap();
            panic!("intentional fixed registry poison");
        });
    }
    pub(crate) fn rounded_len(&self) -> usize {
        self.0.rounded
    }
    pub(crate) fn retain_decoded(&self, backing: Arc<dyn DecodedBacking>) -> Result<()> {
        self.check_creator()?;
        #[cfg(target_os = "linux")]
        {
            let (identity, length, regular) = file_identity(&backing.file())?;
            if identity != self.0.identity || !regular || length != self.0.length as u64 {
                return Err(Error::Corrupt("fixed decoded lifetime identity"));
            }
        }
        #[cfg(any(target_os = "macos", windows))]
        {
            let object = backing.object();
            object.check_creator()?;
            if self.0.identity != Identity::Decoded(Arc::as_ptr(&object) as usize)
                || object.len() != self.0.length
                || !self
                    .0
                    .resources
                    .decoded_object
                    .as_ref()
                    .is_some_and(|retained| Arc::ptr_eq(retained, &object))
            {
                return Err(Error::Corrupt("fixed decoded lifetime identity"));
            }
        }
        let mut resource = self
            .0
            .resources
            .decoded
            .lock()
            .map_err(|_| Error::OutcomeUnknown)?;
        if resource.is_none() {
            *resource = Some(backing);
        }
        Ok(())
    }
    pub fn lock(&self) -> std::io::Result<()> {
        self.lock_inner(false)
    }
    pub(crate) fn lock_required(&self) -> std::io::Result<()> {
        self.lock_inner(true)
    }
    fn lock_inner(&self, retain_partial: bool) -> std::io::Result<()> {
        self.check_creator().map_err(std::io::Error::other)?;
        let mut locks = self
            .0
            .locks
            .lock()
            .map_err(|_| std::io::Error::other("fixed lock count poisoned"))?;
        let next = locks
            .count
            .checked_add(1)
            .ok_or(std::io::Error::other("fixed lock count overflow"))?;
        if (locks.count == 0 || !locks.healthy)
            && let Err(error) = sys::lock(self.0.pointer.as_ptr(), self.0.rounded)
        {
            if retain_partial {
                locks.count = next;
                locks.healthy = false;
            }
            return Err(error);
        }
        locks.count = next;
        locks.healthy = true;
        Ok(())
    }
    pub fn unlock(&self) -> std::io::Result<()> {
        self.check_creator().map_err(std::io::Error::other)?;
        let mut locks = self
            .0
            .locks
            .lock()
            .map_err(|_| std::io::Error::other("fixed lock count poisoned"))?;
        if locks.count == 0 {
            return Err(std::io::Error::other("unbalanced fixed unlock"));
        }
        if locks.count == 1
            && let Err(error) = sys::unlock(self.0.pointer.as_ptr(), self.0.rounded)
        {
            return Err(error);
        }
        locks.count -= 1;
        if locks.count == 0 {
            locks.healthy = true;
        }
        Ok(())
    }
    pub fn bytes(&self) -> Result<&[u8]> {
        self.check_creator()?; // Lease keeps its immutable, exclusive slot live.
        Ok(unsafe { std::slice::from_raw_parts(self.as_ptr(), self.0.length) })
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        #[cfg(all(
            any(target_os = "linux", target_os = "macos", windows),
            target_pointer_width = "64"
        ))]
        {
            // A forked child owns a separate address space. It never changes
            // parent VMAs; skip inherited Rust mutexes entirely in that child.
            if self.reservation.creator != std::process::id() {
                return;
            }
            #[cfg(test)]
            if let Some((started, resume)) = self.reservation.drop_pause.lock().unwrap().take() {
                let _ = started.send(());
                let _ = resume.recv_timeout(std::time::Duration::from_secs(3));
            }
            let Ok(mut registry) = self.reservation.slots.lock() else {
                self.reservation.poisoned.store(true, Ordering::Release);
                return;
            };
            if registry.get(&self.offset).is_none_or(|r| r.id != self.id) {
                self.reservation.poisoned.store(true, Ordering::Release);
                return;
            }
            let address = self.pointer.as_ptr();
            // Retain ownership of the entire window between leases. Releasing
            // this hole would permit a foreign mmap before our next slot map.
            if self
                .reservation
                .backend
                .restore(address, self.rounded)
                .is_err()
            {
                self.reservation.poisoned.store(true, Ordering::Release);
                return;
            }
            registry.remove(&self.offset);
        }
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        // No live lease can outlast its reservation. If final OS cleanup fails,
        // retain every controller/decoded/kernel-lease resource with that VMA
        // instead of announcing a successful last unmap by field destruction.
        if self.backend.release().is_err() {
            let resources = std::mem::replace(
                &mut self.slots,
                CreatorOwned::new(Mutex::new(BTreeMap::new())),
            );
            std::mem::forget(resources);
        }
    }
}

#[cfg(all(test, target_os = "linux", target_pointer_width = "64"))]
mod tests {
    use super::*;
    use std::{fs, io::Write, path::PathBuf, time::Duration};
    pub(super) struct Source {
        path: PathBuf,
        pub(super) file: Arc<File>,
    }
    impl Source {
        pub(super) fn new(size: usize, byte: u8) -> Self {
            let path = std::env::temp_dir().join(format!(
                "fixed-address-{}-{}",
                std::process::id(),
                crate::new_identity().unwrap()
            ));
            let mut f = File::create(&path).unwrap();
            f.write_all(&vec![byte; size]).unwrap();
            drop(f);
            Self {
                file: Arc::new(File::open(&path).unwrap()),
                path,
            }
        }
    }
    impl Drop for Source {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }
    #[test]
    fn actual_addresses_disjoint_slots_sharing_and_last_drop_keep_window_reserved() {
        let p = os_page().unwrap();
        let r = Reservation::reserve(None, 4 * p).unwrap();
        let a = Source::new(2 * p, b'A');
        let b = Source::new(p, b'B');
        let first = unsafe { r.map_file(a.file.clone(), 0, 0, p) }.unwrap();
        assert_eq!(first.as_ptr() as usize, r.address());
        assert!(first.bytes().unwrap().iter().all(|v| *v == b'A'));
        let second = unsafe { r.map_file(a.file.clone(), p as u64, 2 * p, 17) }.unwrap();
        assert_eq!(second.as_ptr() as usize, r.address() + 2 * p);
        assert_eq!(second.bytes().unwrap(), &[b'A'; 17]);
        let same = unsafe { r.map_file(a.file.clone(), 0, 0, p) }.unwrap();
        assert!(Arc::ptr_eq(&first.0, &same.0));
        assert!(matches!(
            unsafe { r.map_file(b.file.clone(), 0, 0, p) },
            Err(Error::Busy)
        ));
        drop(first);
        assert!(matches!(
            unsafe { r.map_file(b.file.clone(), 0, 0, p) },
            Err(Error::Busy)
        ));
        drop(same);
        assert!(matches!(
            Reservation::reserve(Some(r.address()), p),
            Err(Error::Busy)
        ));
        let b = unsafe { r.map_file(b.file.clone(), 0, 0, p) }.unwrap();
        assert!(b.bytes().unwrap().iter().all(|v| *v == b'B'));
    }
    #[test]
    fn unique_writable_slot_excludes_all_other_leases_until_drop_and_then_remaps_readonly() {
        let p = os_page().unwrap();
        let r = Reservation::reserve(None, 4 * p).unwrap();
        let a = Source::new(2 * p, b'A');
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&a.path)
            .unwrap();
        let mut w = unsafe { r.map_file_mut(&file, p, 2 * p) }.unwrap();
        assert_eq!(w.as_ptr() as usize, r.address() + p);
        w.bytes_mut().unwrap()[..p].fill(b'W');
        assert!(matches!(
            unsafe { r.map_file(a.file.clone(), 0, p, 2 * p) },
            Err(Error::Busy)
        ));
        assert!(matches!(
            unsafe { r.map_file_mut(&file, p, 2 * p) },
            Err(Error::Busy)
        ));
        assert_eq!(&w.bytes().unwrap()[..p], vec![b'W'; p]);
        w.flush_range(p, false).unwrap();
        drop(w);
        let read = unsafe { r.map_file(a.file.clone(), 0, p, 2 * p) }.unwrap();
        assert_eq!(&read.bytes().unwrap()[..p], vec![b'W'; p]);
        assert!(matches!(
            unsafe { r.map_file_mut(&file, p, p) },
            Err(Error::Busy)
        ));
        drop(read);
        let mut abort = unsafe { r.map_file_mut(&file, 0, p) }.unwrap();
        abort.bytes_mut().unwrap().fill(b'Z');
        drop(abort);
        let next = unsafe { r.map_file_mut(&file, 0, p) }.unwrap();
        assert_eq!(next.as_ptr() as usize, r.address());
    }
    #[test]
    fn exact_reservation_never_clobbers_a_foreign_vma() {
        let p = os_page().unwrap();
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                p,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        assert_ne!(ptr, libc::MAP_FAILED);
        unsafe { std::ptr::write_bytes(ptr, 0x5a, p) };
        assert!(matches!(
            Reservation::reserve(Some(ptr as usize), p),
            Err(Error::Busy)
        ));
        assert!(
            unsafe { std::slice::from_raw_parts(ptr as *const u8, p) }
                .iter()
                .all(|v| *v == 0x5a)
        );
        assert_eq!(unsafe { libc::munmap(ptr, p) }, 0);
        let r = Reservation::reserve(Some(ptr as usize), p).unwrap();
        assert_eq!(r.address(), ptr as usize);
    }
    #[test]
    fn invalid_extents_fail_before_replacing_live_bytes() {
        let p = os_page().unwrap();
        let r = Reservation::reserve(None, 2 * p).unwrap();
        let a = Source::new(p, b'A');
        let x = unsafe { r.map_file(a.file.clone(), 0, 0, p) }.unwrap();
        for (off, pos, len) in [
            (1, p, p),
            (0, 1, p),
            (0, p, p + 1),
            (p as u64, p, p),
            (0, p, 0),
        ] {
            assert!(unsafe { r.map_file(a.file.clone(), off, pos, len) }.is_err());
        }
        assert!(x.bytes().unwrap().iter().all(|v| *v == b'A'));
        assert!(Reservation::reserve(Some(1), p).is_err());
        assert!(Reservation::reserve(None, usize::MAX).is_err());
    }
    #[test]
    fn maps_keep_no_extra_file_descriptor_and_counter_exhaustion_precedes_mapping() {
        let p = os_page().unwrap();
        let r = Reservation::reserve(None, 2 * p).unwrap();
        let source = Source::new(p, b'A');
        let input = Arc::new(File::open(&source.path).unwrap());
        let weak = Arc::downgrade(&input);
        let lease = unsafe { r.map_file(input.clone(), 0, 0, p) }.unwrap();
        drop(input);
        assert!(weak.upgrade().is_none());
        assert_eq!(lease.bytes().unwrap(), vec![b'A'; p]);
        r.serial.store(u64::MAX, Ordering::Relaxed);
        assert!(matches!(
            unsafe { r.map_file(source.file.clone(), 0, p, p) },
            Err(Error::LimitExceeded)
        ));
        assert_eq!(lease.file_offset(), 0);
        assert_eq!(lease.bytes().unwrap(), vec![b'A'; p]);
    }

    #[test]
    fn read_ahead_is_fixed_at_mint_and_never_mutates_a_different_live_policy() {
        let p = os_page().unwrap();
        let r = Reservation::reserve(None, p).unwrap();
        let source = Source::new(p, b'A');
        let normal = unsafe { r.map_file(source.file.clone(), 0, 0, p) }.unwrap();
        assert!(normal.advise_random().is_err());
        assert!(matches!(
            unsafe {
                r.map_file_with_read_ahead(source.file.clone(), 0, 0, p, crate::ReadAhead::Random)
            },
            Err(Error::Busy)
        ));
        drop(normal);
        let random = unsafe {
            r.map_file_with_read_ahead(source.file.clone(), 0, 0, p, crate::ReadAhead::Random)
        }
        .unwrap();
        random.advise_random().unwrap();
        assert!(matches!(
            unsafe { r.map_file(source.file.clone(), 0, 0, p) },
            Err(Error::Busy)
        ));
        let shared = unsafe {
            r.map_file_with_read_ahead(source.file.clone(), 0, 0, p, crate::ReadAhead::Random)
        }
        .unwrap();
        assert!(Arc::ptr_eq(&random.0, &shared.0));
    }

    #[test]
    fn pending_last_drop_cannot_reprotect_a_replacement() {
        let p = os_page().unwrap();
        let r = Reservation::reserve(None, p).unwrap();
        let a = Source::new(p, b'A');
        let b = Source::new(p, b'B');
        let x = unsafe { r.map_file(a.file.clone(), 0, 0, p) }.unwrap();
        let (started, ready) = std::sync::mpsc::channel();
        let (resume, continue_drop) = std::sync::mpsc::channel();
        *r.drop_pause.lock().unwrap() = Some((started, continue_drop));
        let worker = std::thread::spawn(move || drop(x));
        ready.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(matches!(
            unsafe { r.map_file(b.file.clone(), 0, 0, p) },
            Err(Error::Busy)
        ));
        resume.send(()).unwrap();
        worker.join().unwrap();
        let replacement = unsafe { r.map_file(b.file.clone(), 0, 0, p) }.unwrap();
        assert_eq!(replacement.bytes().unwrap(), vec![b'B'; p]);
    }
}

#[cfg(all(test, target_os = "linux", target_pointer_width = "64"))]
mod fork_drop_tests {
    use super::*;
    use crate::creator_owned::tests::{DropProbe, HeldMutex, fork_drop};

    #[test]
    fn child_drops_orphaned_reservation_without_walking_held_registry() {
        const NAME: &str = "fixed_address::fork_drop_tests::child_drops_orphaned_reservation_without_walking_held_registry";
        if crate::history::tests::run_isolated(NAME) {
            return;
        }
        let p = os_page().unwrap();
        let reservation = Reservation::reserve(None, p).unwrap();
        let source = super::tests::Source::new(p, b'A');
        let lease = unsafe { reservation.map_file(source.file.clone(), 0, 0, p) }.unwrap();
        let (probe, mut reader) = DropProbe::new();
        *lease.0.resources.drop_probe.lock().unwrap() = Some(Box::new(probe));
        let (charge, held_ledger) = crate::residency::tests::fixed_charge_with_held_ledger();
        lease
            .0
            .resources
            .charges
            .lock()
            .unwrap()
            .insert(std::num::NonZeroU64::new(1).unwrap(), charge);
        // A failed registry operation retains the resource bag until the whole
        // reservation unmaps. This is the real quarantine path, not a fake Arc.
        lease.poison_registry_for_test();
        drop(lease);
        assert_eq!(Arc::strong_count(&reservation), 1);
        let held = unsafe { HeldMutex::new(&reservation.slots) };
        let reservation = fork_drop(reservation);
        drop(held);
        drop(held_ledger);
        reader.absent();
        drop(reservation);
        reader.once();
        crate::history::tests::worker_completed(NAME);
    }

    #[test]
    fn child_mapping_early_return_also_skips_automatic_resource_drop() {
        const NAME: &str = "fixed_address::fork_drop_tests::child_mapping_early_return_also_skips_automatic_resource_drop";
        if crate::history::tests::run_isolated(NAME) {
            return;
        }
        let p = os_page().unwrap();
        let reservation = Reservation::reserve(None, p).unwrap();
        let source = super::tests::Source::new(p, b'A');
        let lease = unsafe { reservation.map_file(source.file.clone(), 0, 0, p) }.unwrap();
        let (probe, mut reader) = DropProbe::new();
        *lease.0.resources.drop_probe.lock().unwrap() = Some(Box::new(probe));
        // Integrity fault: make the mapping itself the last resource owner.
        // Its PID early-return alone must not run resource field destructors.
        reservation.slots.lock().unwrap().remove(&0);
        assert_eq!(Arc::strong_count(&lease.0.resources), 1);
        let held = unsafe { HeldMutex::new(&lease.0.resources.charges) };
        let lease = fork_drop(lease);
        drop(held);
        reader.absent();
        assert_eq!(lease.bytes().unwrap()[0], b'A');
        drop(lease);
        reader.once();
        drop(reservation);
        crate::history::tests::worker_completed(NAME);
    }
}
