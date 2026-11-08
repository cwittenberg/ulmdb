//! The only raw-pointer boundary. Frozen prefixes never become writable again.
use crate::{Error, Result};
use memmap2::{MmapOptions, MmapRaw};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io,
    path::Path,
    ptr::NonNull,
    sync::Arc,
};
use ulmdb_platform::FileExt;

pub const HEADER: usize = 4096;
/// Backing of the rebuildable cache only. WAL durability is independent.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ImageMode {
    #[default]
    File,
    Anonymous,
}

#[derive(Clone, Copy, Debug)]
pub struct ChunkLimits {
    pub capacity: usize,
    pub reserve_quantum: usize,
    pub max_burned_bytes: usize,
}
impl Default for ChunkLimits {
    fn default() -> Self {
        Self {
            capacity: 64 * 1024 * 1024,
            reserve_quantum: 1024 * 1024,
            max_burned_bytes: 16 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct ChunkStatus {
    pub image_mode: ImageMode,
    /// Physical reservation established with fallocate; zero for anonymous maps.
    pub file_reserved_bytes: usize,
    /// Reserved virtual address extent, not resident/locked physical memory.
    pub anonymous_virtual_bytes: usize,
    pub capacity: usize,
    /// Writable backing extent: file reservation or anonymous virtual capacity.
    pub reserved_bytes: usize,
    pub frozen_end: usize,
    pub published_end: usize,
    pub burned_bytes: usize,
    pub blocked: bool,
}
struct RawOwner {
    raw: MmapRaw,
    mode: ImageMode,
    chunk_id: u64,
    image_incarnation: u128,
}

/// Immutable prefix. Pointer and bound precede cold lifetime/identity ownership.
#[repr(C)]
pub struct ReadSpan {
    ptr: NonNull<u8>,
    bound: usize,
    owner: Arc<RawOwner>,
}
#[allow(unsafe_code)]
// SAFETY: this type exposes only a frozen prefix; unique writer authority can
// mutate only bytes beyond EVERY previously frozen bound. Arc pins its mapping.
unsafe impl Send for ReadSpan {}
#[allow(unsafe_code)]
// SAFETY: frozen bytes never change, and RawOwner cannot expose writes publicly.
unsafe impl Sync for ReadSpan {}
impl ReadSpan {
    pub fn image_mode(&self) -> ImageMode {
        self.owner.mode
    }
    pub fn chunk_id(&self) -> u64 {
        self.owner.chunk_id
    }
    pub fn image_incarnation(&self) -> u128 {
        self.owner.image_incarnation
    }
    pub fn end(&self) -> usize {
        self.bound
    }
    pub fn capacity(&self) -> usize {
        self.owner.raw.len()
    }
    pub fn read(&self, offset: usize, length: usize) -> Result<&[u8]> {
        let end = offset
            .checked_add(length)
            .ok_or(Error::Corrupt("image extent overflow"))?;
        if end > self.bound {
            return Err(Error::Corrupt("image extent exceeds frozen bound"));
        }
        #[allow(unsafe_code)]
        // SAFETY: nonnull mapping, admitted <=isize::MAX capacity, checked prefix
        // range, Arc owner pins it; no writer may modify any frozen prefix byte.
        Ok(unsafe { std::slice::from_raw_parts(self.ptr.as_ptr().add(offset), length) })
    }
    pub(crate) fn same_owner(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.owner, &other.owner)
    }
}

/// Unique control handle. Owned tail leases retain fencing; read spans do not.
pub struct ChunkWriter {
    control: Arc<WriterControl>,
}
enum Backing {
    File(crate::WriterFence),
    Anonymous,
}
impl Backing {
    fn mode(&self) -> ImageMode {
        match self {
            Self::File(_) => ImageMode::File,
            Self::Anonymous => ImageMode::Anonymous,
        }
    }
    fn reserve(&self, start: usize, length: usize) -> Result<()> {
        match self {
            Self::File(file) => reserve(file, start, length),
            Self::Anonymous => Ok(()),
        }
    }
}
struct WriterControl {
    owner: Arc<RawOwner>,
    backing: Backing,
    _environment_fence: Option<Arc<crate::WriterFence>>,
    limits: ChunkLimits,
    state: std::sync::Mutex<State>,
}
struct State {
    cursor: usize,
    published: usize,
    reserved: usize,
    burned: usize,
    blocked: bool,
    active: Option<u64>,
    next_nonce: u64,
}
impl WriterControl {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn view(&self, end: usize) -> Arc<ReadSpan> {
        // Mapping construction checked nonnull, capacity and isize geometry.
        Arc::new(ReadSpan {
            ptr: NonNull::new(self.owner.raw.as_mut_ptr()).expect("checked nonnull mapping"),
            bound: end,
            owner: Arc::clone(&self.owner),
        })
    }
}
/// Non-Clone mutation authority. Every write is checked against this owned tail.
pub(super) struct TailLease {
    control: Option<Arc<WriterControl>>,
    nonce: u64,
    pub start: usize,
    pub cursor: usize,
    reserved: usize,
    blocked: bool,
}
pub(super) struct FrozenPermit {
    control: Option<Arc<WriterControl>>,
    nonce: u64,
    start: usize,
    end: usize,
    pub view: Arc<ReadSpan>,
}
impl ChunkWriter {
    pub fn create(path: impl AsRef<Path>, chunk_id: u64, limits: ChunkLimits) -> Result<Self> {
        Self::create_inner(path.as_ref(), chunk_id, limits, None)
    }
    /// Retain an already acquired Environment fence through every owned lease.
    /// The caller binds that fence to its environment; this is no role certificate.
    pub fn create_with_fence(
        path: impl AsRef<Path>,
        chunk_id: u64,
        limits: ChunkLimits,
        fence: Arc<crate::WriterFence>,
    ) -> Result<Self> {
        Self::create_inner(path.as_ref(), chunk_id, limits, Some(fence))
    }
    fn create_inner(
        path: &Path,
        chunk_id: u64,
        limits: ChunkLimits,
        fence: Option<Arc<crate::WriterFence>>,
    ) -> Result<Self> {
        validate_limits(chunk_id, limits)?;
        let file = ulmdb_platform::create_private_file(path, true)?;
        ulmdb_platform::set_private_permissions(&file, true)?;
        let file = crate::WriterFence::try_lock(file).map_err(|e| match e {
            std::fs::TryLockError::WouldBlock => Error::Busy,
            std::fs::TryLockError::Error(e) => Error::Io(e),
        })?;
        file.set_len(limits.capacity as u64)?;
        let reserved = round_up(HEADER, limits.reserve_quantum)?.min(limits.capacity);
        reserve(&file, 0, reserved)?;
        let incarnation = crate::new_identity()?;
        let mut header = [0u8; HEADER];
        header[..8].copy_from_slice(b"ULMDBIM1");
        header[8..16].copy_from_slice(&chunk_id.to_le_bytes());
        header[16..24].copy_from_slice(&(limits.capacity as u64).to_le_bytes());
        header[24..40].copy_from_slice(&incarnation.to_le_bytes());
        let hash = Sha256::digest(&header[..HEADER - 32]);
        header[HEADER - 32..].copy_from_slice(&hash);
        file.write_all_at(&header, 0)?;
        let independent = OpenOptions::new().read(true).write(true).open(path)?;
        let a = ulmdb_platform::file_identity(&file)?;
        let b = ulmdb_platform::file_identity(&independent)?;
        if !a.same_object(&b) || a.len() != b.len() || !a.is_file() || !b.is_file() {
            return Err(Error::Corrupt("image independent OFD identity mismatch"));
        }
        let raw = MmapOptions::new()
            .len(limits.capacity)
            .map_raw(&independent)?;
        if raw.as_ptr().is_null() {
            return Err(Error::Corrupt("null image mapping"));
        }
        Self::from_mapping(
            raw,
            chunk_id,
            incarnation,
            limits,
            Backing::File(file),
            reserved,
            fence,
        )
    }
    /// Private anonymous read image. This creates no cache file or file lock;
    /// Environment callers must retain their separate durable-writer fence.
    pub fn create_anonymous(chunk_id: u64, limits: ChunkLimits) -> Result<Self> {
        Self::anonymous_inner(chunk_id, limits, None)
    }
    pub fn create_anonymous_with_fence(
        chunk_id: u64,
        limits: ChunkLimits,
        fence: Arc<crate::WriterFence>,
    ) -> Result<Self> {
        Self::anonymous_inner(chunk_id, limits, Some(fence))
    }
    fn anonymous_inner(
        chunk_id: u64,
        limits: ChunkLimits,
        fence: Option<Arc<crate::WriterFence>>,
    ) -> Result<Self> {
        validate_limits(chunk_id, limits)?;
        let incarnation = crate::new_identity()?;
        let mut header = [0u8; HEADER];
        header[..8].copy_from_slice(b"ULMDBIM1");
        header[8..16].copy_from_slice(&chunk_id.to_le_bytes());
        header[16..24].copy_from_slice(&(limits.capacity as u64).to_le_bytes());
        header[24..40].copy_from_slice(&incarnation.to_le_bytes());
        let hash = Sha256::digest(&header[..HEADER - 32]);
        header[HEADER - 32..].copy_from_slice(&hash);
        // This fresh map has no escaped references. Initialize only now, then
        // consume the typed map into MmapRaw before any immutable span exists.
        let mut mapping = MmapOptions::new().len(limits.capacity).map_anon()?;
        mapping[..HEADER].copy_from_slice(&header);
        let raw: MmapRaw = mapping.into();
        Self::from_mapping(
            raw,
            chunk_id,
            incarnation,
            limits,
            Backing::Anonymous,
            limits.capacity,
            fence,
        )
    }
    fn from_mapping(
        raw: MmapRaw,
        chunk_id: u64,
        incarnation: u128,
        limits: ChunkLimits,
        backing: Backing,
        reserved: usize,
        fence: Option<Arc<crate::WriterFence>>,
    ) -> Result<Self> {
        if raw.as_ptr().is_null() || raw.len() != limits.capacity {
            return Err(Error::Corrupt("image raw mapping geometry"));
        }
        let mode = backing.mode();
        let control = WriterControl {
            owner: Arc::new(RawOwner {
                raw,
                mode,
                chunk_id,
                image_incarnation: incarnation,
            }),
            backing,
            _environment_fence: fence,
            limits,
            state: std::sync::Mutex::new(State {
                cursor: HEADER,
                published: HEADER,
                reserved,
                burned: 0,
                blocked: false,
                active: None,
                next_nonce: 1,
            }),
        };
        Ok(Self {
            control: Arc::new(control),
        })
    }
    /// Fresh physical incarnation, preserving logical references. This copies
    /// an immutable source prefix, not a recovery-authority certificate.
    pub fn create_from(
        path: impl AsRef<Path>,
        limits: ChunkLimits,
        source: &ReadSpan,
    ) -> Result<Self> {
        if source.end() > limits.capacity {
            return Err(Error::LimitExceeded);
        }
        let writer = Self::create(path, source.chunk_id(), limits)?;
        let mut lease = writer.acquire()?;
        lease.ensure_reserved(source.end())?;
        lease.write(HEADER, source.read(HEADER, source.end() - HEADER)?)?;
        lease.cursor = source.end();
        lease.freeze()?.accept();
        Ok(writer)
    }
    pub fn chunk_id(&self) -> u64 {
        self.control.owner.chunk_id
    }
    pub fn image_incarnation(&self) -> u128 {
        self.control.owner.image_incarnation
    }
    pub fn image_mode(&self) -> ImageMode {
        self.control.owner.mode
    }
    pub fn status(&self) -> ChunkStatus {
        let s = self.control.state();
        ChunkStatus {
            image_mode: self.image_mode(),
            file_reserved_bytes: if self.image_mode() == ImageMode::File {
                s.reserved
            } else {
                0
            },
            anonymous_virtual_bytes: if self.image_mode() == ImageMode::Anonymous {
                self.control.limits.capacity
            } else {
                0
            },
            capacity: self.control.limits.capacity,
            reserved_bytes: s.reserved,
            frozen_end: s.cursor,
            published_end: s.published,
            burned_bytes: s.burned,
            blocked: s.blocked,
        }
    }
    pub fn read_view(&self) -> Arc<ReadSpan> {
        let end = self.control.state().published;
        self.control.view(end)
    }
    pub(super) fn acquire(&self) -> Result<TailLease> {
        let mut s = self.control.state();
        if s.active.is_some() {
            return Err(Error::Busy);
        }
        if s.blocked || s.cursor >= self.control.limits.capacity {
            return Err(Error::LimitExceeded);
        }
        let nonce = s.next_nonce;
        let next = nonce.checked_add(1).ok_or(Error::LimitExceeded)?;
        s.next_nonce = next;
        s.active = Some(nonce);
        Ok(TailLease {
            control: Some(Arc::clone(&self.control)),
            nonce,
            start: s.cursor,
            cursor: s.cursor,
            reserved: s.reserved,
            blocked: false,
        })
    }
}
impl TailLease {
    fn control(&self) -> &WriterControl {
        self.control.as_deref().expect("active tail authority")
    }
    pub fn chunk_id(&self) -> u64 {
        self.control().owner.chunk_id
    }
    pub fn capacity(&self) -> usize {
        self.control().limits.capacity
    }
    pub fn ensure_reserved(&mut self, end: usize) -> Result<()> {
        if self.blocked || end > self.capacity() {
            return Err(Error::LimitExceeded);
        }
        if end <= self.reserved {
            return Ok(());
        }
        let next = round_up(end, self.control().limits.reserve_quantum)?.min(self.capacity());
        if let Err(e) = self
            .control()
            .backing
            .reserve(self.reserved, next - self.reserved)
        {
            self.blocked = true;
            self.control().state().blocked = true;
            return Err(e);
        }
        self.reserved = next;
        self.control().state().reserved = next;
        Ok(())
    }
    pub fn read(&self, offset: usize, length: usize) -> Result<&[u8]> {
        let end = offset
            .checked_add(length)
            .ok_or(Error::Corrupt("private image extent overflow"))?;
        if offset < self.start || end > self.cursor {
            return Err(Error::Corrupt("private image read outside allocation tail"));
        }
        #[allow(unsafe_code)]
        // SAFETY: checked mapping bounds. This shared lease borrow prevents all
        // writes until the returned temporary read ends; no mutation capability is cloned.
        Ok(unsafe {
            std::slice::from_raw_parts(self.control().owner.raw.as_ptr().add(offset), length)
        })
    }
    pub fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<()> {
        let end = offset
            .checked_add(bytes.len())
            .ok_or(Error::LimitExceeded)?;
        if self.blocked || offset < self.start || end > self.reserved {
            return Err(Error::InvalidInput("image write outside private lease"));
        }
        #[allow(unsafe_code)]
        // SAFETY: this unique lease covers only bytes beyond every previously
        // frozen view. Its private read borrows exclude this mutable borrow.
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.control().owner.raw.as_mut_ptr().add(offset),
                bytes.len(),
            )
        };
        Ok(())
    }
    pub fn zero(&mut self, start: usize, end: usize) -> Result<()> {
        if self.blocked || start > end || start < self.start || end > self.reserved {
            return Err(Error::InvalidInput("image zero outside private lease"));
        }
        #[allow(unsafe_code)]
        // SAFETY: same exclusive disjoint tail invariant as write.
        unsafe {
            std::ptr::write_bytes(
                self.control().owner.raw.as_mut_ptr().add(start),
                0,
                end - start,
            )
        };
        Ok(())
    }
    pub fn freeze(mut self) -> Result<FrozenPermit> {
        if self.blocked {
            return Err(Error::LimitExceeded);
        }
        let control = self.control.take().expect("active tail authority");
        // Allocate the bounded immutable owner before a durable decision.
        let view = control.view(self.cursor);
        {
            let mut s = control.state();
            debug_assert_eq!(s.active, Some(self.nonce));
            s.cursor = self.cursor;
            s.reserved = self.reserved;
        }
        Ok(FrozenPermit {
            control: Some(control),
            nonce: self.nonce,
            start: self.start,
            end: self.cursor,
            view,
        })
    }
}
fn release(
    control: Arc<WriterControl>,
    nonce: u64,
    start: usize,
    end: usize,
    reserved: usize,
    blocked: bool,
    accept: bool,
) {
    let mut s = control.state();
    debug_assert_eq!(s.active, Some(nonce));
    s.cursor = s.cursor.max(end);
    s.reserved = s.reserved.max(reserved);
    s.blocked |= blocked;
    if accept {
        s.published = s.published.max(end);
    } else {
        s.burned = s.burned.saturating_add(end - start);
        s.blocked |= s.burned > control.limits.max_burned_bytes;
    }
    s.active = None;
}
impl Drop for TailLease {
    fn drop(&mut self) {
        if let Some(c) = self.control.take() {
            release(
                c,
                self.nonce,
                self.start,
                self.cursor,
                self.reserved,
                self.blocked,
                false,
            );
        }
    }
}
impl FrozenPermit {
    pub fn start(&self) -> usize {
        self.start
    }
    pub fn accept(mut self) -> Arc<ReadSpan> {
        if let Some(c) = self.control.take() {
            release(c, self.nonce, self.start, self.end, 0, false, true);
        }
        Arc::clone(&self.view)
    }
}
impl Drop for FrozenPermit {
    fn drop(&mut self) {
        if let Some(c) = self.control.take() {
            release(c, self.nonce, self.start, self.end, 0, false, false);
        }
    }
}
fn validate_limits(chunk_id: u64, limits: ChunkLimits) -> Result<()> {
    let page = crate::residency::os::page_size()?;
    if chunk_id == 0 || chunk_id == u64::MAX {
        return Err(Error::InvalidInput("invalid image chunk ID"));
    }
    if limits.capacity < HEADER
        || limits.capacity > 268_435_456
        || limits.capacity > isize::MAX as usize
        || !limits.capacity.is_multiple_of(page)
        || limits.reserve_quantum < page
        || !limits.reserve_quantum.is_multiple_of(page)
        || limits.reserve_quantum > limits.capacity
        || limits.max_burned_bytes > limits.capacity
    {
        return Err(Error::InvalidInput("invalid image chunk limits"));
    }
    Ok(())
}
fn round_up(n: usize, quantum: usize) -> Result<usize> {
    n.checked_add(quantum - 1)
        .map(|n| n / quantum * quantum)
        .ok_or(Error::LimitExceeded)
}
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn reserve(file: &File, start: usize, length: usize) -> Result<()> {
    use std::os::fd::AsRawFd;
    let start = libc::off64_t::try_from(start).map_err(|_| Error::LimitExceeded)?;
    let length = libc::off64_t::try_from(length).map_err(|_| Error::LimitExceeded)?;
    loop {
        // SAFETY: valid borrowed FD and checked nonnegative off64_t extent.
        if unsafe { libc::fallocate64(file.as_raw_fd(), 0, start, length) } == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e.into());
        }
    }
}
#[cfg(not(target_os = "linux"))]
fn reserve(_: &File, _: usize, _: usize) -> Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported).into())
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn serving_span_keeps_pointer_and_bound_before_cold_owner() {
        assert_eq!(std::mem::offset_of!(ReadSpan, ptr), 0);
        assert_eq!(
            std::mem::offset_of!(ReadSpan, bound),
            std::mem::size_of::<usize>()
        );
        assert_eq!(
            std::mem::size_of::<ReadSpan>(),
            3 * std::mem::size_of::<usize>()
        );
    }
}
