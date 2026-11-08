//! Bounded, epoch-pinned values behind stable generational heads.
//!
//! Preparation allocates privately before journal I/O. Ready exposes numeric
//! heads, never a future View. Publishing installs every link before returning
//! the new View. Readers borrow immutable values without locks.
//!
//! Fixed OnceLock directories own lazy, permanently addressed slot chunks.
//! Chunks and optional 64-byte value sidecars remain until the final Shared
//! owner drops; epoch-safe GC recycles their slots. Requested directory, chunk,
//! queue, workspace, guard-control and blob capacities share one byte limit.
//! Caller-owned input/output vectors and allocator usable size/RSS are separate.
//!
//! Head validation loads end before a packed generation/latest-slot link. A
//! View predating retirement prevents reuse. A later View cannot observe the
//! pre-retirement FOREVER (publication happens-before obtaining that View); if
//! it observes a reused slot's FOREVER reset, that acquire also orders the new
//! generation link reset. Its following link load therefore rejects old IDs.
//! After those gates every traversed record is epoch-pinned: future records
//! have end > birth > View epoch, and the selected record has end > View epoch.
//! Traversal stops there, before older, potentially reclaimed predecessors.

use crate::{Error, Result};
use std::{
    cell::UnsafeCell,
    collections::VecDeque,
    mem::size_of,
    num::NonZeroU32,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
};

const INLINE: usize = 16;
const MEDIUM: usize = 64;
const HEAD_CHUNK: usize = 256;
const VERSION_CHUNK: usize = 128;
const FREE: u8 = 0;
const PRIVATE: u8 = 1;
const PUBLISHED: u8 = 2;
const FOREVER: u64 = u64::MAX;
const ARC_HEADER: usize = 2 * size_of::<usize>();

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_heads: usize,
    pub max_versions: usize,
    pub max_views: usize,
}
impl Limits {
    pub fn for_bytes(max_bytes: usize) -> Result<Self> {
        if max_bytes < 32 << 10 {
            return Err(Error::LimitExceeded);
        }
        let target = (max_bytes / 512).min(1 << 20);
        let heads = 1usize << target.ilog2();
        let limits = Self {
            max_bytes,
            max_heads: heads,
            max_versions: heads.checked_mul(2).ok_or(Error::LimitExceeded)?,
            max_views: heads,
        };
        limits.validate()?;
        Ok(limits)
    }
    fn validate(self) -> Result<()> {
        if self.max_heads == 0
            || self.max_versions == 0
            || self.max_views == 0
            || self.max_heads >= u32::MAX as usize
            || self.max_versions >= u32::MAX as usize
            || base_bytes(self)? > self.max_bytes
        {
            return Err(Error::LimitExceeded);
        }
        Ok(())
    }
}
impl Default for Limits {
    fn default() -> Self {
        Self::for_bytes(32 << 20).expect("fixed valid version-pool limits")
    }
}

/// Generation in the high word, head slot+1 in the low word; zero is invalid.
/// Pool-local IDs are not global/source authorities. Pair heads with that same
/// pool's Views; a foreign pool can independently use equal numeric IDs.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct Head {
    // Declaration order keeps ordering identical to the encoded u64: compare
    // the high generation word before the low, one-based slot word.
    generation: NonZeroU32,
    slot: NonZeroU32,
}
impl std::fmt::Debug for Head {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Head").field(&self.to_u64()).finish()
    }
}
impl std::hash::Hash for Head {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.to_u64(), state);
    }
}
impl Head {
    pub fn to_u64(self) -> u64 {
        (u64::from(self.generation.get()) << 32) | u64::from(self.slot.get())
    }
    pub fn from_u64(value: u64) -> Option<Self> {
        Some(Self {
            generation: NonZeroU32::new((value >> 32) as u32)?,
            slot: NonZeroU32::new(value as u32)?,
        })
    }
    pub fn to_le_bytes(self) -> [u8; 8] {
        self.to_u64().to_le_bytes()
    }
    pub fn from_le_bytes(bytes: [u8; 8]) -> Option<Self> {
        Self::from_u64(u64::from_le_bytes(bytes))
    }
    fn parts(self) -> (usize, u32) {
        // Construction validates both nonzero words once. This removes only
        // redundant representation checks; a View still checks pool bounds,
        // retirement, generation, publication and visible birth on every get.
        ((self.slot.get() - 1) as usize, self.generation.get())
    }
}
fn identity(index: usize, generation: u32) -> u64 {
    (u64::from(generation) << 32) | (index as u64 + 1)
}
fn slot_id(value: u64) -> Option<(usize, u32)> {
    let low = value as u32;
    let generation = (value >> 32) as u32;
    (low != 0 && generation != 0).then_some((low.wrapping_sub(1) as usize, generation))
}
#[derive(Clone, Copy, Debug)]
pub struct Mutation<'a> {
    pub head: Option<Head>,
    pub value: Option<&'a [u8]>,
}

struct Budget {
    limit: usize,
    used: AtomicUsize,
    peak: AtomicUsize,
    large: AtomicUsize,
}
struct TemporaryCharge {
    budget: Arc<Budget>,
    bytes: usize,
}
impl Drop for TemporaryCharge {
    fn drop(&mut self) {
        self.budget.release(self.bytes);
    }
}
impl Budget {
    fn reserve(&self, bytes: usize) -> Result<()> {
        let before = self
            .used
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                n.checked_add(bytes).filter(|n| *n <= self.limit)
            })
            .map_err(|_| Error::LimitExceeded)?;
        self.peak.fetch_max(before + bytes, Ordering::Relaxed);
        Ok(())
    }
    fn release(&self, bytes: usize) {
        self.used.fetch_sub(bytes, Ordering::Relaxed);
    }
}
struct Blob {
    bytes: Box<[u8]>,
    budget: Arc<Budget>,
}
impl Drop for Blob {
    fn drop(&mut self) {
        self.budget
            .large
            .fetch_sub(self.bytes.len(), Ordering::Relaxed);
        self.budget.release(self.bytes.len());
    }
}
enum Value {
    Inline { length: u8, bytes: [u8; INLINE] },
    Medium { length: u8 },
    Large(Blob),
}
impl Value {
    fn new(bytes: &[u8], budget: &Arc<Budget>) -> Result<Self> {
        if bytes.len() <= INLINE {
            let mut inline = [0; INLINE];
            inline[..bytes.len()].copy_from_slice(bytes);
            return Ok(Self::Inline {
                length: bytes.len() as u8,
                bytes: inline,
            });
        }
        if bytes.len() <= MEDIUM {
            return Ok(Self::Medium {
                length: bytes.len() as u8,
            });
        }
        budget.reserve(bytes.len())?;
        let mut copy = Vec::new();
        if copy.try_reserve_exact(bytes.len()).is_err() {
            budget.release(bytes.len());
            return Err(Error::LimitExceeded);
        }
        copy.extend_from_slice(bytes);
        budget.large.fetch_add(bytes.len(), Ordering::Relaxed);
        Ok(Self::Large(Blob {
            bytes: copy.into_boxed_slice(),
            budget: budget.clone(),
        }))
    }
}
struct Record {
    birth: u64,
    previous: u32,
    value: Value,
}
struct HeadSlot {
    // High word = head generation, low word = latest version slot+1.
    // A private head has low word zero and cannot expose a record.
    link: AtomicU64,
    end: AtomicU64,
}
impl HeadSlot {
    fn empty() -> Self {
        Self {
            link: AtomicU64::new(0),
            end: AtomicU64::new(0),
        }
    }
}
struct VersionSlot {
    // Only the writer inspects these retirement/allocation fields. Readers
    // enter through published head/previous edges and their View's epoch pin.
    generation: AtomicU32,
    state: AtomicU8,
    end: AtomicU64,
    record: UnsafeCell<Option<Record>>,
}
impl VersionSlot {
    fn empty() -> Self {
        Self {
            generation: AtomicU32::new(0),
            state: AtomicU8::new(FREE),
            end: AtomicU64::new(FOREVER),
            record: UnsafeCell::new(None),
        }
    }
}
#[repr(align(64))]
struct MediumCell(UnsafeCell<[u8; MEDIUM]>);
struct VersionChunk {
    slots: Box<[VersionSlot]>,
    medium: OnceLock<Box<[MediumCell]>>,
}

// All raw record/sidecar accesses are private to this module. The only reader
// is View::get after the packed head gates; the only mutations are unlinked
// initialization or exclusive-writer epoch-safe reclaim/reuse.
#[allow(unsafe_code)]
mod cells {
    use super::*;
    // SAFETY: a published record and its matching medium cell are immutable.
    // Each View that can reach either prevents reuse until its epoch is at or
    // beyond their retirement. Chunks never move/unload while any View exists.
    // PRIVATE slots have no published incoming edge; FREE slots have no reader.
    unsafe impl Sync for VersionSlot {}
    // SAFETY: exactly the same slot lifetime protects the medium sidecar body.
    unsafe impl Sync for MediumCell {}
    pub(super) fn initialize(slot: &VersionSlot, record: Record) {
        debug_assert_eq!(slot.state.load(Ordering::Relaxed), PRIVATE);
        // SAFETY: this private slot is exclusively owned and unlinked.
        unsafe {
            *slot.record.get() = Some(record);
        }
    }
    pub(super) fn write_medium(slot: &VersionSlot, cell: &MediumCell, bytes: &[u8]) {
        debug_assert_eq!(slot.state.load(Ordering::Relaxed), PRIVATE);
        // SAFETY: the corresponding version is private; old users have passed
        // its retirement floor. The full array was initialized on allocation.
        unsafe {
            (&mut *cell.0.get())[..bytes.len()].copy_from_slice(bytes);
        }
    }
    pub(super) fn reclaim(slot: &VersionSlot) {
        debug_assert_eq!(slot.state.load(Ordering::Relaxed), FREE);
        // SAFETY: unpublished abort, or every live View is at/after end.
        unsafe {
            (&mut *slot.record.get()).take();
        }
    }
    pub(super) fn borrow(slot: &VersionSlot) -> Option<&Record> {
        // SAFETY: get has a matching head generation, end > View epoch, and
        // follows only initialized published edges. Every traversed record is
        // pinned by that View, including records born after the View's epoch.
        unsafe { (&*slot.record.get()).as_ref() }
    }
    pub(super) fn medium(cell: &MediumCell, length: usize) -> &[u8] {
        // SAFETY: the borrowed Record pins this exact sidecar slot. Length was
        // bounded on creation; the sidecar is never changed until safe reuse.
        unsafe { &(&*cell.0.get())[..length] }
    }
}

struct Shared {
    heads: Box<[OnceLock<Box<[HeadSlot]>>]>,
    versions: Box<[OnceLock<VersionChunk>]>,
    budget: Arc<Budget>,
}
impl Shared {
    #[inline]
    fn head(&self, index: usize) -> Option<&HeadSlot> {
        self.heads
            .get(index / HEAD_CHUNK)?
            .get()?
            .get(index % HEAD_CHUNK)
    }
    #[inline]
    fn version(&self, index: usize) -> Option<(&VersionChunk, &VersionSlot)> {
        let chunk = self.versions.get(index / VERSION_CHUNK)?.get()?;
        Some((chunk, chunk.slots.get(index % VERSION_CHUNK)?))
    }
}
struct ViewGuard {
    epoch: u64,
    budget: Arc<Budget>,
}
const GUARD_BYTES: usize = size_of::<ViewGuard>() + ARC_HEADER;
impl Drop for ViewGuard {
    fn drop(&mut self) {
        // The writer's private queue owns one strong Arc until collection.
        // Its final drop runs synchronously before this writer can allocate
        // again; a paused reader destructor cannot hide retained control bytes.
        self.budget.release(GUARD_BYTES);
    }
}
#[derive(Clone)]
pub struct View {
    shared: Arc<Shared>,
    guard: Arc<ViewGuard>,
}
impl View {
    pub fn epoch(&self) -> u64 {
        self.guard.epoch
    }
    /// Allocation-free streaming lookup state. Only positive, permanently
    /// addressed chunk/sidecar references are cached; all Head gates remain.
    pub fn read_cache(&self) -> ReadCache<'_> {
        ReadCache {
            view: self,
            epoch: self.epoch(),
            heads: None,
            versions: None,
            medium: None,
        }
    }
    #[inline]
    pub fn get(&self, head: Head) -> Option<&[u8]> {
        let (index, generation) = head.parts();
        let head = self.shared.head(index)?;
        let epoch = self.epoch();
        // This ordering is essential: a reused end reset synchronizes its
        // preceding generation/link reset with the following packed load.
        if epoch >= head.end.load(Ordering::Acquire) {
            return None;
        }
        let link = head.link.load(Ordering::Acquire);
        if (link >> 32) as u32 != generation {
            return None;
        }
        let mut current = link as u32;
        while current != 0 {
            let index = current as usize - 1;
            let (chunk, version) = self.shared.version(index)?;
            let record = cells::borrow(version)?;
            if record.birth <= epoch {
                return match &record.value {
                    Value::Inline { length, bytes } => Some(&bytes[..usize::from(*length)]),
                    Value::Medium { length } => Some(cells::medium(
                        chunk.medium.get()?.get(index % VERSION_CHUNK)?,
                        usize::from(*length),
                    )),
                    Value::Large(blob) => Some(&blob.bytes),
                };
            }
            // Never follow predecessors after finding this View's value: older
            // links can name reclaimed slots, while every traversed future
            // record has end > birth > epoch and is therefore still pinned.
            current = record.previous;
        }
        None
    }
}

/// A View-borrowed streaming cache. Cached references remain valid even when
/// the writer initializes other chunks or recycles unrelated slots. Returned
/// slices borrow the View, so advancing/dropping this cache cannot invalidate
/// a previously returned value. No allocation, Arc clone, or reader lock.
pub struct ReadCache<'view> {
    view: &'view View,
    epoch: u64,
    heads: Option<(usize, &'view [HeadSlot])>,
    versions: Option<(usize, &'view VersionChunk)>,
    medium: Option<(usize, &'view [MediumCell])>,
}
impl<'view> ReadCache<'view> {
    #[inline]
    fn head(&mut self, index: usize) -> Option<&'view HeadSlot> {
        let chunk_id = index / HEAD_CHUNK;
        let slots = match self.heads {
            Some((cached, slots)) if cached == chunk_id => slots,
            _ => {
                let slots: &'view [HeadSlot] = self.view.shared.heads.get(chunk_id)?.get()?;
                self.heads = Some((chunk_id, slots));
                slots
            }
        };
        slots.get(index % HEAD_CHUNK)
    }
    #[inline]
    fn version(&mut self, index: usize) -> Option<(&'view VersionChunk, &'view VersionSlot)> {
        let chunk_id = index / VERSION_CHUNK;
        let chunk = match self.versions {
            Some((cached, chunk)) if cached == chunk_id => chunk,
            _ => {
                let chunk: &'view VersionChunk = self.view.shared.versions.get(chunk_id)?.get()?;
                self.versions = Some((chunk_id, chunk));
                chunk
            }
        };
        Some((chunk, chunk.slots.get(index % VERSION_CHUNK)?))
    }
    #[inline]
    fn medium(&mut self, index: usize, chunk: &'view VersionChunk) -> Option<&'view MediumCell> {
        let chunk_id = index / VERSION_CHUNK;
        let slots = match self.medium {
            Some((cached, slots)) if cached == chunk_id => slots,
            _ => {
                // Never remember absence: preparation can initialize this
                // sidecar later while this old View/cache continues to exist.
                let slots: &'view [MediumCell] = chunk.medium.get()?;
                self.medium = Some((chunk_id, slots));
                slots
            }
        };
        slots.get(index % VERSION_CHUNK)
    }
    #[inline]
    pub fn get(&mut self, head: Head) -> Option<&'view [u8]> {
        let (index, generation) = head.parts();
        let epoch = self.epoch;
        let head = self.head(index)?;
        // Identical validation/order to View::get; cached addresses grant no
        // authority to skip generation, retirement or visible-birth checks.
        if epoch >= head.end.load(Ordering::Acquire) {
            return None;
        }
        let link = head.link.load(Ordering::Acquire);
        if (link >> 32) as u32 != generation {
            return None;
        }
        let mut current = link as u32;
        while current != 0 {
            let index = current as usize - 1;
            let (chunk, slot) = self.version(index)?;
            // SAFETY argument is the same as View::get: published incoming
            // edges and the borrowed View pin all bodies actually traversed.
            let record = cells::borrow(slot)?;
            if record.birth <= epoch {
                return match &record.value {
                    Value::Inline { length, bytes } => Some(&bytes[..usize::from(*length)]),
                    Value::Medium { length } => Some(cells::medium(
                        self.medium(index, chunk)?,
                        usize::from(*length),
                    )),
                    Value::Large(blob) => Some(&blob.bytes),
                };
            }
            current = record.previous;
        }
        None
    }
}

#[derive(Clone, Copy)]
struct Retired {
    identity: u64,
    epoch: u64,
}
struct Tracked {
    epoch: u64,
    guard: Arc<ViewGuard>,
}
#[derive(Clone, Copy)]
struct Pending {
    head: Head,
    version: u64,
    old_version: u64,
    new_head: bool,
    delete: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub limit_bytes: usize,
    pub allocated_bytes: usize,
    pub peak_bytes: usize,
    pub large_bytes: usize,
    pub head_capacity: usize,
    pub version_capacity: usize,
    pub view_capacity: usize,
    pub active_heads: usize,
    pub allocated_versions: usize,
    pub retired_heads: usize,
    pub retired_versions: usize,
    pub tracked_views: usize,
    pub workspace_bytes: usize,
    pub reclaimed_heads: u64,
    pub reclaimed_versions: u64,
    pub gc_steps: u64,
    pub initialized_heads: usize,
    pub initialized_versions: usize,
    pub medium_capacity_bytes: usize,
    pub directory_bytes: usize,
}

pub struct PoolWriter {
    shared: Arc<Shared>,
    limits: Limits,
    epoch: u64,
    free_heads: Vec<usize>,
    free_versions: Vec<usize>,
    retired_heads: VecDeque<Retired>,
    retired_versions: VecDeque<Retired>,
    views: VecDeque<Tracked>,
    pending: Vec<Pending>,
    heads_out: Vec<Head>,
    seen: Vec<u64>,
    workspace_bytes: usize,
    initialized_heads: usize,
    initialized_versions: usize,
    medium_capacity_bytes: usize,
    active_heads: usize,
    allocated_versions: usize,
    reclaimed_heads: u64,
    reclaimed_versions: u64,
    gc_steps: u64,
}
fn power_capacity(n: usize) -> Result<usize> {
    n.checked_next_power_of_two().ok_or(Error::LimitExceeded)
}
fn directory_bytes(l: Limits) -> Result<usize> {
    l.max_heads
        .div_ceil(HEAD_CHUNK)
        .checked_mul(size_of::<OnceLock<Box<[HeadSlot]>>>())
        .and_then(|n| {
            l.max_versions
                .div_ceil(VERSION_CHUNK)
                .checked_mul(size_of::<OnceLock<VersionChunk>>())
                .and_then(|m| n.checked_add(m))
        })
        .ok_or(Error::LimitExceeded)
}
fn base_bytes(l: Limits) -> Result<usize> {
    (size_of::<PoolWriter>() + size_of::<Shared>() + size_of::<Budget>() + 2 * ARC_HEADER)
        .checked_add(directory_bytes(l)?)
        .ok_or(Error::LimitExceeded)
}
fn filled<T>(n: usize, mut make: impl FnMut(usize) -> T) -> Result<Vec<T>> {
    let mut out = Vec::new();
    out.try_reserve_exact(n).map_err(|_| Error::LimitExceeded)?;
    for index in 0..n {
        out.push(make(index));
    }
    Ok(out)
}
fn reserve_vec<T>(values: &mut Vec<T>, n: usize, budget: &Budget) -> Result<usize> {
    if n <= values.capacity() {
        return Ok(0);
    }
    let capacity = power_capacity(n)?;
    let additional = (capacity - values.capacity())
        .checked_mul(size_of::<T>())
        .ok_or(Error::LimitExceeded)?;
    budget.reserve(additional)?;
    if values.try_reserve_exact(capacity - values.len()).is_err() {
        budget.release(additional);
        return Err(Error::LimitExceeded);
    }
    Ok(additional)
}
fn reserve_queue<T>(values: &mut VecDeque<T>, n: usize, budget: &Budget) -> Result<()> {
    if n <= values.capacity() {
        return Ok(());
    }
    let capacity = power_capacity(n)?;
    let additional = (capacity - values.capacity())
        .checked_mul(size_of::<T>())
        .ok_or(Error::LimitExceeded)?;
    budget.reserve(additional)?;
    if values.try_reserve_exact(capacity - values.len()).is_err() {
        budget.release(additional);
        return Err(Error::LimitExceeded);
    }
    Ok(())
}

impl PoolWriter {
    fn empty(epoch: u64, limits: Limits) -> Result<Self> {
        limits.validate()?;
        if epoch == FOREVER {
            return Err(Error::LimitExceeded);
        }
        let base = base_bytes(limits)?;
        let budget = Arc::new(Budget {
            limit: limits.max_bytes,
            used: AtomicUsize::new(base),
            peak: AtomicUsize::new(base),
            large: AtomicUsize::new(0),
        });
        let shared = Arc::new(Shared {
            heads: filled(limits.max_heads.div_ceil(HEAD_CHUNK), |_| OnceLock::new())?
                .into_boxed_slice(),
            versions: filled(limits.max_versions.div_ceil(VERSION_CHUNK), |_| {
                OnceLock::new()
            })?
            .into_boxed_slice(),
            budget,
        });
        Ok(Self {
            shared,
            limits,
            epoch,
            free_heads: Vec::new(),
            free_versions: Vec::new(),
            retired_heads: VecDeque::new(),
            retired_versions: VecDeque::new(),
            views: VecDeque::new(),
            pending: Vec::new(),
            heads_out: Vec::new(),
            seen: Vec::new(),
            workspace_bytes: 0,
            initialized_heads: 0,
            initialized_versions: 0,
            medium_capacity_bytes: 0,
            active_heads: 0,
            allocated_versions: 0,
            reclaimed_heads: 0,
            reclaimed_versions: 0,
            gc_steps: 0,
        })
    }
    /// Seed exactly the already-selected recovery epoch. Returned heads follow
    /// input order. Caller-owned input/output vector storage is separate.
    pub fn seed(epoch: u64, values: &[&[u8]], limits: Limits) -> Result<(Self, View, Vec<Head>)> {
        let mut pool = Self::empty(epoch, limits)?;
        if values.len() > limits.max_heads {
            return Err(Error::LimitExceeded);
        }
        let bytes = values
            .len()
            .checked_mul(size_of::<Mutation<'_>>())
            .ok_or(Error::LimitExceeded)?;
        pool.shared.budget.reserve(bytes)?;
        let _charge = TemporaryCharge {
            budget: pool.shared.budget.clone(),
            bytes,
        };
        let mut edits = Vec::new();
        edits
            .try_reserve_exact(values.len())
            .map_err(|_| Error::LimitExceeded)?;
        for value in values {
            edits.push(Mutation {
                head: None,
                value: Some(*value),
            });
        }
        let ready = pool.prepare_inner(epoch, &edits)?;
        let heads = copy_heads(ready.heads())?;
        let view = ready.publish();
        Ok((pool, view, heads))
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    /// Conservative minimum of all tracked published Views. Dead front owners
    /// remain protective until collect removes them; no older epoch can be
    /// newly published. Suitable for retiring coherent auxiliary index nodes.
    pub fn oldest_view_epoch(&self) -> u64 {
        self.views.front().map_or(self.epoch, |view| view.epoch)
    }
    pub fn prepare<'a>(&'a mut self, epoch: u64, edits: &[Mutation<'_>]) -> Result<Ready<'a>> {
        if self.epoch.checked_add(1) != Some(epoch) || epoch == FOREVER {
            return Err(Error::LimitExceeded);
        }
        self.prepare_inner(epoch, edits)
    }
    fn ensure_heads(&mut self, count: usize) -> Result<()> {
        while self.free_heads.len() < count {
            let start = self.initialized_heads;
            if start >= self.limits.max_heads {
                return Err(Error::LimitExceeded);
            }
            let n = HEAD_CHUNK.min(self.limits.max_heads - start);
            // Free capacity covers ALL initialized slots, so GC/abort never
            // allocates even after every currently live head is deleted.
            reserve_vec(&mut self.free_heads, start + n, &self.shared.budget)?;
            let bytes = n
                .checked_mul(size_of::<HeadSlot>())
                .ok_or(Error::LimitExceeded)?;
            self.shared.budget.reserve(bytes)?;
            let slots = match filled(n, |_| HeadSlot::empty()) {
                Ok(slots) => slots.into_boxed_slice(),
                Err(error) => {
                    self.shared.budget.release(bytes);
                    return Err(error);
                }
            };
            assert!(self.shared.heads[start / HEAD_CHUNK].set(slots).is_ok());
            self.initialized_heads += n;
            for index in (start..start + n).rev() {
                self.free_heads.push(index);
            }
        }
        Ok(())
    }
    fn ensure_versions(&mut self, count: usize) -> Result<()> {
        while self.free_versions.len() < count {
            let start = self.initialized_versions;
            if start >= self.limits.max_versions {
                return Err(Error::LimitExceeded);
            }
            let n = VERSION_CHUNK.min(self.limits.max_versions - start);
            reserve_vec(&mut self.free_versions, start + n, &self.shared.budget)?;
            let bytes = n
                .checked_mul(size_of::<VersionSlot>())
                .ok_or(Error::LimitExceeded)?;
            self.shared.budget.reserve(bytes)?;
            let slots = match filled(n, |_| VersionSlot::empty()) {
                Ok(slots) => slots.into_boxed_slice(),
                Err(error) => {
                    self.shared.budget.release(bytes);
                    return Err(error);
                }
            };
            assert!(
                self.shared.versions[start / VERSION_CHUNK]
                    .set(VersionChunk {
                        slots,
                        medium: OnceLock::new()
                    })
                    .is_ok()
            );
            self.initialized_versions += n;
            for index in (start..start + n).rev() {
                self.free_versions.push(index);
            }
        }
        Ok(())
    }
    fn prepare_inner<'a>(&'a mut self, epoch: u64, edits: &[Mutation<'_>]) -> Result<Ready<'a>> {
        if edits.len() > self.limits.max_heads || self.views.len() >= self.limits.max_views {
            return Err(Error::LimitExceeded);
        }
        debug_assert!(self.pending.is_empty());
        self.heads_out.clear();
        self.seen.clear();
        self.workspace_bytes += reserve_vec(&mut self.pending, edits.len(), &self.shared.budget)?;
        self.workspace_bytes += reserve_vec(&mut self.heads_out, edits.len(), &self.shared.budget)?;
        self.workspace_bytes += reserve_vec(&mut self.seen, edits.len(), &self.shared.budget)?;
        let (mut heads, mut records, mut retire_heads, mut retire_records) =
            (0usize, 0usize, 0usize, 0usize);
        for edit in edits {
            match (edit.head, edit.value) {
                (None, None) => return Err(Error::InvalidInput("version pool empty mutation")),
                (None, Some(_)) => {
                    heads += 1;
                    records += 1;
                }
                (Some(head), value) => {
                    self.current_head(head)?;
                    self.seen.push(head.to_u64());
                    retire_records += 1;
                    if value.is_some() {
                        records += 1;
                    } else {
                        retire_heads += 1;
                    }
                }
            }
        }
        self.seen.sort_unstable();
        if self.seen.windows(2).any(|p| p[0] == p[1]) {
            return Err(Error::InvalidInput(
                "version pool duplicate head; coalesce before prepare",
            ));
        }
        if self.retired_heads.len() + retire_heads > self.limits.max_heads
            || self.retired_versions.len() + retire_records > self.limits.max_versions
        {
            return Err(Error::LimitExceeded);
        }
        self.ensure_heads(heads)?;
        self.ensure_versions(records)?;
        let head_retirements = self.retired_heads.len() + retire_heads;
        let version_retirements = self.retired_versions.len() + retire_records;
        let tracked_views = self.views.len() + 1;
        reserve_queue(
            &mut self.retired_heads,
            head_retirements,
            &self.shared.budget,
        )?;
        reserve_queue(
            &mut self.retired_versions,
            version_retirements,
            &self.shared.budget,
        )?;
        reserve_queue(&mut self.views, tracked_views, &self.shared.budget)?;
        self.shared.budget.reserve(GUARD_BYTES)?;
        let guard = Arc::new(ViewGuard {
            epoch,
            budget: self.shared.budget.clone(),
        });
        for edit in edits {
            match self.prepare_one(epoch, *edit) {
                Ok(pending) => {
                    self.heads_out.push(pending.head);
                    self.pending.push(pending);
                }
                Err(error) => {
                    self.abort_pending();
                    return Err(error);
                }
            }
        }
        Ok(Ready {
            pool: self,
            epoch,
            guard: Some(guard),
            published: false,
        })
    }
    fn current_head(&self, head: Head) -> Result<u64> {
        let (index, generation) = head.parts();
        let slot = self.shared.head(index).ok_or(Error::StaleHandle)?;
        let link = slot.link.load(Ordering::Acquire);
        if (link >> 32) as u32 != generation
            || link as u32 == 0
            || slot.end.load(Ordering::Acquire) != FOREVER
        {
            return Err(Error::StaleHandle);
        }
        let index = link as u32 as usize - 1;
        let (_, version) = self.shared.version(index).ok_or(Error::StaleHandle)?;
        Ok(identity(index, version.generation.load(Ordering::Relaxed)))
    }
    fn allocate_head(&mut self) -> Result<Head> {
        while let Some(index) = self.free_heads.pop() {
            let slot = self.shared.head(index).expect("initialized free head");
            let Some(generation) =
                ((slot.link.load(Ordering::Relaxed) >> 32) as u32).checked_add(1)
            else {
                continue;
            };
            // One atomic word prevents a new generation from exposing an old
            // generation's latest edge. Reset end LAST, essential for get's ABA proof.
            slot.link
                .store(u64::from(generation) << 32, Ordering::Release);
            slot.end.store(FOREVER, Ordering::Release);
            return Ok(Head::from_u64(identity(index, generation)).expect("allocated valid head"));
        }
        Err(Error::LimitExceeded)
    }
    fn allocate_version(&mut self, epoch: u64, previous: u64, bytes: &[u8]) -> Result<u64> {
        while let Some(index) = self.free_versions.pop() {
            let (chunk, slot) = self
                .shared
                .version(index)
                .expect("initialized free version");
            let Some(generation) = slot.generation.load(Ordering::Relaxed).checked_add(1) else {
                continue;
            };
            if bytes.len() > INLINE && bytes.len() <= MEDIUM && chunk.medium.get().is_none() {
                let allocation = chunk
                    .slots
                    .len()
                    .checked_mul(size_of::<MediumCell>())
                    .ok_or(Error::LimitExceeded)?;
                if let Err(error) = self.shared.budget.reserve(allocation) {
                    self.free_versions.push(index);
                    return Err(error);
                }
                let cells = match filled(chunk.slots.len(), |_| {
                    MediumCell(UnsafeCell::new([0; MEDIUM]))
                }) {
                    Ok(cells) => cells.into_boxed_slice(),
                    Err(error) => {
                        self.shared.budget.release(allocation);
                        self.free_versions.push(index);
                        return Err(error);
                    }
                };
                assert!(chunk.medium.set(cells).is_ok());
                self.medium_capacity_bytes += allocation;
            }
            let value = match Value::new(bytes, &self.shared.budget) {
                Ok(value) => value,
                Err(error) => {
                    self.free_versions.push(index);
                    return Err(error);
                }
            };
            slot.generation.store(generation, Ordering::Relaxed);
            slot.state.store(PRIVATE, Ordering::Relaxed);
            slot.end.store(FOREVER, Ordering::Relaxed);
            if bytes.len() > INLINE && bytes.len() <= MEDIUM {
                cells::write_medium(
                    slot,
                    &chunk.medium.get().expect("prepared sidecar")[index % VERSION_CHUNK],
                    bytes,
                );
            }
            cells::initialize(
                slot,
                Record {
                    birth: epoch,
                    previous: previous as u32,
                    value,
                },
            );
            self.allocated_versions += 1;
            return Ok(identity(index, generation));
        }
        Err(Error::LimitExceeded)
    }
    fn prepare_one(&mut self, epoch: u64, edit: Mutation<'_>) -> Result<Pending> {
        let (head, new_head, old_version) = match edit.head {
            Some(head) => (head, false, self.current_head(head)?),
            None => (self.allocate_head()?, true, 0),
        };
        let version = match edit.value {
            Some(value) => match self.allocate_version(epoch, old_version, value) {
                Ok(version) => version,
                Err(error) => {
                    if new_head {
                        self.abort_head(head);
                    }
                    return Err(error);
                }
            },
            None => 0,
        };
        Ok(Pending {
            head,
            version,
            old_version,
            new_head,
            delete: edit.value.is_none(),
        })
    }
    fn abort_head(&mut self, head: Head) {
        let (index, _) = head.parts();
        self.shared
            .head(index)
            .expect("initialized head")
            .end
            .store(0, Ordering::Release);
        self.free_heads.push(index);
    }
    fn abort_version(&mut self, version: u64) {
        let (index, _) = slot_id(version).expect("private version");
        let (_, slot) = self.shared.version(index).expect("initialized version");
        slot.state.store(FREE, Ordering::Relaxed);
        cells::reclaim(slot);
        self.allocated_versions -= 1;
        self.free_versions.push(index);
    }
    fn abort_pending(&mut self) {
        while let Some(pending) = self.pending.pop() {
            if pending.version != 0 {
                self.abort_version(pending.version);
            }
            if pending.new_head {
                self.abort_head(pending.head);
            }
        }
        self.heads_out.clear();
        self.seen.clear();
    }
    /// Bounded writer work. Every live epoch, including one earlier than a
    /// future record's birth, conservatively protects records ending after it.
    pub fn collect(&mut self, work: usize) -> usize {
        let mut done = 0;
        while done < work {
            if self
                .views
                .front()
                .is_some_and(|v| Arc::strong_count(&v.guard) == 1)
            {
                // Only the writer's private queue Arc remains. No public path
                // can clone it or resurrect a View at this epoch. Acquire the
                // last reader's Release decrement before advancing the floor
                // or mutating any formerly borrowed bytes. strong_count itself
                // is Relaxed. Dropping this final Arc synchronously destroys the
                // guard/control allocation and releases its capacity ticket.
                std::sync::atomic::fence(Ordering::Acquire);
                self.views.pop_front();
                done += 1;
                continue;
            }
            let floor = self.views.front().map_or(self.epoch, |v| v.epoch);
            if self
                .retired_versions
                .front()
                .is_some_and(|r| r.epoch <= floor)
            {
                let retired = self
                    .retired_versions
                    .pop_front()
                    .expect("checked retirement");
                let (index, generation) = slot_id(retired.identity).expect("retired version");
                let (_, slot) = self
                    .shared
                    .version(index)
                    .expect("initialized retired version");
                if slot.generation.load(Ordering::Relaxed) == generation
                    && slot.state.load(Ordering::Relaxed) == PUBLISHED
                    && slot.end.load(Ordering::Relaxed) == retired.epoch
                {
                    slot.state.store(FREE, Ordering::Relaxed);
                    cells::reclaim(slot);
                    self.free_versions.push(index);
                    self.allocated_versions -= 1;
                    self.reclaimed_versions = self.reclaimed_versions.saturating_add(1);
                }
                done += 1;
                continue;
            }
            if self.retired_heads.front().is_some_and(|r| r.epoch <= floor) {
                let retired = self.retired_heads.pop_front().expect("checked retirement");
                let (index, generation) = slot_id(retired.identity).expect("retired head");
                let slot = self.shared.head(index).expect("initialized retired head");
                if (slot.link.load(Ordering::Relaxed) >> 32) as u32 == generation
                    && slot.end.load(Ordering::Relaxed) == retired.epoch
                {
                    slot.end.store(0, Ordering::Release);
                    self.free_heads.push(index);
                    self.reclaimed_heads = self.reclaimed_heads.saturating_add(1);
                }
                done += 1;
                continue;
            }
            break;
        }
        self.gc_steps = self.gc_steps.saturating_add(done as u64);
        done
    }
    pub fn stats(&self) -> Stats {
        Stats {
            limit_bytes: self.shared.budget.limit,
            allocated_bytes: self.shared.budget.used.load(Ordering::Relaxed),
            peak_bytes: self.shared.budget.peak.load(Ordering::Relaxed),
            large_bytes: self.shared.budget.large.load(Ordering::Relaxed),
            head_capacity: self.limits.max_heads,
            version_capacity: self.limits.max_versions,
            view_capacity: self.limits.max_views,
            active_heads: self.active_heads,
            allocated_versions: self.allocated_versions,
            retired_heads: self.retired_heads.len(),
            retired_versions: self.retired_versions.len(),
            tracked_views: self.views.len(),
            workspace_bytes: self.workspace_bytes,
            reclaimed_heads: self.reclaimed_heads,
            reclaimed_versions: self.reclaimed_versions,
            gc_steps: self.gc_steps,
            initialized_heads: self.initialized_heads,
            initialized_versions: self.initialized_versions,
            medium_capacity_bytes: self.medium_capacity_bytes,
            directory_bytes: directory_bytes(self.limits).expect("validated limits"),
        }
    }
}
/// No future View accessor. Drop aborts privately; publish allocates nothing.
pub struct Ready<'a> {
    pool: &'a mut PoolWriter,
    epoch: u64,
    guard: Option<Arc<ViewGuard>>,
    published: bool,
}
impl Ready<'_> {
    pub fn heads(&self) -> &[Head] {
        &self.pool.heads_out
    }
    /// Bind preparation proofs to this exact pool and selected predecessor.
    /// Equal numeric heads or epochs in another pool grant no such authority.
    pub fn is_successor_of(&self, view: &View) -> bool {
        Arc::ptr_eq(&self.pool.shared, &view.shared)
            && self.pool.epoch == view.epoch()
            && view.epoch().checked_add(1) == Some(self.epoch)
    }
    pub fn publish(mut self) -> View {
        for pending in &self.pool.pending {
            let (head_index, generation) = pending.head.parts();
            let head = self.pool.shared.head(head_index).expect("initialized head");
            if pending.delete {
                head.end.store(self.epoch, Ordering::Release);
                self.pool.retired_heads.push_back(Retired {
                    identity: pending.head.to_u64(),
                    epoch: self.epoch,
                });
                self.pool.active_heads -= 1;
            } else {
                let (index, _) = slot_id(pending.version).expect("prepared version");
                self.pool
                    .shared
                    .version(index)
                    .expect("initialized version")
                    .1
                    .state
                    .store(PUBLISHED, Ordering::Relaxed);
                head.link.store(
                    (u64::from(generation) << 32) | (pending.version as u32 as u64),
                    Ordering::Release,
                );
                if pending.new_head {
                    self.pool.active_heads += 1;
                }
            }
            if pending.old_version != 0 {
                let (index, _) = slot_id(pending.old_version).expect("previous version");
                self.pool
                    .shared
                    .version(index)
                    .expect("initialized version")
                    .1
                    .end
                    .store(self.epoch, Ordering::Relaxed);
                self.pool.retired_versions.push_back(Retired {
                    identity: pending.old_version,
                    epoch: self.epoch,
                });
            }
        }
        self.pool.epoch = self.epoch;
        let guard = self.guard.take().expect("private unpublished guard");
        self.pool.views.push_back(Tracked {
            epoch: self.epoch,
            guard: guard.clone(),
        });
        self.pool.pending.clear();
        self.pool.heads_out.clear();
        self.pool.seen.clear();
        self.published = true;
        View {
            shared: self.pool.shared.clone(),
            guard,
        }
    }
}
impl Drop for Ready<'_> {
    fn drop(&mut self) {
        if !self.published {
            self.pool.abort_pending();
        }
    }
}
fn copy_heads(heads: &[Head]) -> Result<Vec<Head>> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(heads.len())
        .map_err(|_| Error::LimitExceeded)?;
    copy.extend_from_slice(heads);
    Ok(copy)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn head_preserves_numeric_wire_order_and_hash_with_both_nonzero_words() {
        use std::hash::{DefaultHasher, Hash, Hasher};
        assert_eq!(size_of::<Head>(), 8);
        assert_eq!(size_of::<Option<Head>>(), 8);
        for invalid in [0, 1, u64::from(u32::MAX), 1 << 32, u64::MAX << 32] {
            assert!(Head::from_u64(invalid).is_none());
            assert!(Head::from_le_bytes(invalid.to_le_bytes()).is_none());
        }
        let mut encoded = [
            u64::MAX,
            2 << 32 | 1,
            1 << 32 | u64::from(u32::MAX),
            1 << 32 | 1,
        ];
        let mut heads = encoded.map(|value| Head::from_u64(value).unwrap());
        for (head, value) in heads.iter().copied().zip(encoded) {
            assert_eq!(head.to_u64(), value);
            assert_eq!(head.to_le_bytes(), value.to_le_bytes());
            assert_eq!(Head::from_le_bytes(head.to_le_bytes()), Some(head));
            let mut expected = DefaultHasher::new();
            let mut actual = DefaultHasher::new();
            value.hash(&mut expected);
            head.hash(&mut actual);
            assert_eq!(expected.finish(), actual.finish());
        }
        encoded.sort_unstable();
        heads.sort_unstable();
        assert_eq!(heads.map(Head::to_u64), encoded);
    }
    fn limits() -> Limits {
        Limits {
            max_bytes: 64 << 10,
            max_heads: 8,
            max_versions: 16,
            max_views: 8,
        }
    }
    fn replace(head: Head, bytes: &[u8]) -> Mutation<'_> {
        Mutation {
            head: Some(head),
            value: Some(bytes),
        }
    }
    #[test]
    fn old_borrow_stays_stable_and_aborted_ready_never_publishes_future_view() {
        let (mut pool, old, heads) = PoolWriter::seed(0, &[b"original"], limits()).unwrap();
        let head = heads[0];
        let held = old.get(head).unwrap();
        let pointer = held.as_ptr();
        let ready = pool.prepare(1, &[replace(head, b"discarded")]).unwrap();
        assert_eq!(old.get(head), Some(b"original".as_slice()));
        drop(ready);
        assert_eq!(pool.epoch(), 0);
        let ready = pool.prepare(1, &[replace(head, b"new")]).unwrap();
        // Ready exposes no View, so no safe API can borrow future-epoch data.
        assert_eq!(old.get(head).unwrap().as_ptr(), pointer);
        let new = ready.publish();
        assert_eq!(new.get(head), Some(b"new".as_slice()));
        assert_eq!(held, b"original");
        assert_eq!(held.as_ptr(), pointer);
    }
    #[test]
    fn stale_head_is_none_after_delete_gc_and_same_slot_reuse() {
        let (mut pool, old, heads) = PoolWriter::seed(4, &[b"old"], limits()).unwrap();
        let stale = heads[0];
        let deleted = pool
            .prepare(
                5,
                &[Mutation {
                    head: Some(stale),
                    value: None,
                }],
            )
            .unwrap()
            .publish();
        assert!(deleted.get(stale).is_none());
        assert_eq!(old.get(stale), Some(b"old".as_slice()));
        drop(old);
        pool.collect(100);
        let ready = pool
            .prepare(
                6,
                &[Mutation {
                    head: None,
                    value: Some(b"replacement"),
                }],
            )
            .unwrap();
        let fresh = ready.heads()[0];
        assert_ne!(fresh, stale);
        let new = ready.publish();
        assert!(new.get(stale).is_none());
        assert_eq!(new.get(fresh), Some(b"replacement".as_slice()));
        assert!(deleted.get(fresh).is_none());
    }
    #[test]
    fn duplicate_head_refuses_before_allocation_and_bounded_guard_queue_recovers() {
        let mut l = limits();
        l.max_views = 3;
        let (mut pool, first, heads) = PoolWriter::seed(0, &[b"a"], l).unwrap();
        let before = pool.stats().allocated_versions;
        assert!(matches!(
            pool.prepare(1, &[replace(heads[0], b"b"), replace(heads[0], b"c")]),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(pool.stats().allocated_versions, before);
        let second = pool
            .prepare(1, &[replace(heads[0], b"b")])
            .unwrap()
            .publish();
        let third = pool
            .prepare(2, &[replace(heads[0], b"c")])
            .unwrap()
            .publish();
        assert!(matches!(
            pool.prepare(3, &[replace(heads[0], b"d")]),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(first.get(heads[0]), Some(b"a".as_slice()));
        drop(first);
        drop(second);
        pool.collect(100);
        let fourth = pool
            .prepare(3, &[replace(heads[0], b"d")])
            .unwrap()
            .publish();
        assert_eq!(fourth.get(heads[0]), Some(b"d".as_slice()));
        assert_eq!(third.get(heads[0]), Some(b"c".as_slice()));
    }
    #[test]
    fn large_values_release_charge_after_epoch_floor_and_survive_owner_drop() {
        let (mut pool, old, heads) = PoolWriter::seed(0, &[&[1; 1024]], limits()).unwrap();
        let first = pool.stats().large_bytes;
        let current = pool
            .prepare(1, &[replace(heads[0], &[2; 2048])])
            .unwrap()
            .publish();
        assert_eq!(pool.stats().large_bytes, first + 2048);
        drop(old);
        pool.collect(100);
        assert_eq!(pool.stats().large_bytes, 2048);
        let borrowed = current.get(heads[0]).unwrap();
        drop(pool);
        assert_eq!(borrowed, &[2; 2048]);
    }
    #[test]
    fn publication_keeps_every_old_head_consistent_and_ids_are_checked() {
        let (mut pool, old, heads) = PoolWriter::seed(10, &[b"a", b"b"], limits()).unwrap();
        let ready = pool
            .prepare(11, &[replace(heads[0], b"x"), replace(heads[1], b"y")])
            .unwrap();
        assert_eq!(old.get(heads[0]), Some(b"a".as_slice()));
        assert_eq!(old.get(heads[1]), Some(b"b".as_slice()));
        let new = ready.publish();
        assert_eq!(old.get(heads[0]), Some(b"a".as_slice()));
        assert_eq!(old.get(heads[1]), Some(b"b".as_slice()));
        assert_eq!(new.get(heads[0]), Some(b"x".as_slice()));
        assert_eq!(new.get(heads[1]), Some(b"y".as_slice()));
        assert!(Head::from_u64(0).is_none());
        assert!(Head::from_u64(1).is_none());
        assert!(new.get(Head::from_u64(u64::MAX).unwrap()).is_none());
    }

    #[test]
    fn held_blob_pressure_refuses_before_publish_then_collect_allows_retry() {
        let (mut pool, old, heads) = PoolWriter::seed(0, &[&[1; 8192]], limits()).unwrap();
        let current = pool
            .prepare(1, &[replace(heads[0], &[2; 16384])])
            .unwrap()
            .publish();
        let before = pool.stats().allocated_versions;
        assert!(matches!(
            pool.prepare(2, &[replace(heads[0], &[3; 40000])]),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(pool.epoch(), 1);
        assert_eq!(pool.stats().allocated_versions, before);
        assert_eq!(old.get(heads[0]), Some([1; 8192].as_slice()));
        assert_eq!(current.get(heads[0]), Some([2; 16384].as_slice()));
        drop(old);
        pool.collect(100);
        let next = pool
            .prepare(2, &[replace(heads[0], &[3; 40000])])
            .unwrap()
            .publish();
        assert_eq!(next.get(heads[0]), Some([3; 40000].as_slice()));
        assert_eq!(current.get(heads[0]), Some([2; 16384].as_slice()));
        assert!(pool.stats().allocated_bytes <= pool.stats().limit_bytes);
    }

    #[test]
    fn concurrent_borrow_and_stale_head_checks_survive_retirement_and_reuse() {
        use std::sync::{atomic::AtomicBool, mpsc};
        let (mut pool, old, heads) = PoolWriter::seed(0, &[&[7; 128]], limits()).unwrap();
        let head = heads[0];
        let current = std::thread::scope(|scope| {
            let (ready_tx, ready_rx) = mpsc::channel();
            let (go_tx, go_rx) = mpsc::channel();
            let reader = scope.spawn(move || {
                let borrowed = old.get(head).unwrap();
                let address = borrowed.as_ptr();
                ready_tx.send(()).unwrap();
                go_rx.recv().unwrap();
                assert_eq!(borrowed, &[7; 128]);
                assert_eq!(old.get(head).unwrap().as_ptr(), address);
            });
            ready_rx.recv().unwrap();
            let current = pool
                .prepare(1, &[replace(head, &[8; 128])])
                .unwrap()
                .publish();
            pool.collect(100);
            assert_eq!(pool.stats().reclaimed_versions, 0);
            go_tx.send(()).unwrap();
            reader.join().unwrap();
            current
        });
        pool.collect(100);
        assert_eq!(pool.stats().reclaimed_versions, 1);
        let deleted = pool
            .prepare(
                2,
                &[Mutation {
                    head: Some(head),
                    value: None,
                }],
            )
            .unwrap()
            .publish();
        drop(current);
        std::thread::scope(|scope| {
            let reader_view = deleted.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let reader_stop = stop.clone();
            let (ready_tx, ready_rx) = mpsc::channel();
            let (fresh_tx, fresh_rx) = mpsc::channel();
            let reader = scope.spawn(move || {
                assert!(reader_view.get(head).is_none());
                ready_tx.send(()).unwrap();
                while !reader_stop.load(Ordering::Acquire) {
                    assert!(reader_view.get(head).is_none());
                    std::hint::spin_loop();
                }
                assert!(reader_view.get(head).is_none());
                // A leaked new numeric head still cannot reveal a future view.
                assert!(reader_view.get(fresh_rx.recv().unwrap()).is_none());
            });
            ready_rx.recv().unwrap();
            pool.collect(100);
            assert_eq!(pool.stats().reclaimed_heads, 1);
            std::thread::yield_now();
            let ready = pool
                .prepare(
                    3,
                    &[Mutation {
                        head: None,
                        value: Some(b"reused-slot"),
                    }],
                )
                .unwrap();
            let fresh = ready.heads()[0];
            assert_eq!(fresh.to_u64() as u32, head.to_u64() as u32);
            assert_ne!(fresh, head);
            let newest = ready.publish();
            assert!(newest.get(head).is_none());
            assert_eq!(newest.get(fresh), Some(b"reused-slot".as_slice()));
            fresh_tx.send(fresh).unwrap();
            stop.store(true, Ordering::Release);
            reader.join().unwrap();
        });
    }
    #[test]
    fn empty_pool_reserves_directories_only_and_chunks_grow_under_old_borrows() {
        let limits = Limits::default();
        let (mut pool, empty, _) = PoolWriter::seed(0, &[], limits).unwrap();
        let first = pool.stats();
        assert_eq!(first.initialized_heads, 0);
        assert_eq!(first.initialized_versions, 0);
        assert_eq!(first.medium_capacity_bytes, 0);
        assert!(first.allocated_bytes < 128 << 10);
        let ready = pool
            .prepare(
                1,
                &[Mutation {
                    head: None,
                    value: Some(&[1; 64]),
                }],
            )
            .unwrap();
        let head = ready.heads()[0];
        let old = ready.publish();
        drop(empty);
        let borrowed = old.get(head).unwrap();
        let address = borrowed.as_ptr();
        let inputs = vec![
            Mutation {
                head: None,
                value: Some(&[2; 64])
            };
            HEAD_CHUNK + 1
        ];
        let ready = pool.prepare(2, &inputs).unwrap();
        let new_heads = ready.heads().to_vec();
        let current = ready.publish();
        assert!(pool.stats().initialized_heads > HEAD_CHUNK);
        assert!(pool.stats().initialized_versions > VERSION_CHUNK);
        assert_eq!(pool.stats().large_bytes, 0);
        assert!(pool.stats().medium_capacity_bytes > 0);
        pool.collect(10000);
        assert_eq!(borrowed, &[1; 64]);
        assert_eq!(old.get(head).unwrap().as_ptr(), address);
        for head in new_heads {
            assert!(old.get(head).is_none());
            assert_eq!(current.get(head), Some([2; 64].as_slice()));
        }
        assert!(pool.stats().allocated_bytes <= limits.max_bytes);
    }

    #[test]
    fn medium_sidecar_is_lazy_shared_capacity_and_safe_on_record_slot_reuse() {
        let mut l = limits();
        l.max_versions = 2;
        let (mut pool, old, heads) = PoolWriter::seed(0, &[&[1; 64]], l).unwrap();
        let head = heads[0];
        assert_eq!(pool.stats().large_bytes, 0);
        assert_eq!(pool.stats().medium_capacity_bytes, 2 * MEDIUM);
        let old_address = old.get(head).unwrap().as_ptr();
        let current = pool
            .prepare(1, &[replace(head, &[2; 64])])
            .unwrap()
            .publish();
        let current_borrow = current.get(head).unwrap();
        drop(old);
        pool.collect(100);
        let next = pool
            .prepare(2, &[replace(head, &[3; 31])])
            .unwrap()
            .publish();
        assert_eq!(current_borrow, &[2; 64]);
        assert_eq!(next.get(head), Some([3; 31].as_slice()));
        assert_eq!(next.get(head).unwrap().as_ptr(), old_address);
        assert_eq!(pool.stats().medium_capacity_bytes, 2 * MEDIUM);
        assert_eq!(pool.stats().large_bytes, 0);
    }

    #[test]
    fn sidecar_quota_failure_aborts_before_any_link_then_inline_retry_succeeds() {
        let mut l = limits();
        // Fit chunk slots/free lists/workspace, refuse the optional sidecar.
        l.max_bytes = base_bytes(l).unwrap()
            + l.max_heads * (size_of::<HeadSlot>() + size_of::<usize>())
            + l.max_versions * (size_of::<VersionSlot>() + size_of::<usize>())
            + 4 * (size_of::<Pending>() + size_of::<Head>() + size_of::<u64>())
            + 2 * GUARD_BYTES
            + 4 * size_of::<Tracked>();
        let (mut pool, old, _) = PoolWriter::seed(0, &[], l).unwrap();
        assert!(matches!(
            pool.prepare(
                1,
                &[Mutation {
                    head: None,
                    value: Some(&[9; 64])
                }]
            ),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(pool.epoch(), 0);
        assert_eq!(pool.stats().active_heads, 0);
        assert_eq!(pool.stats().allocated_versions, 0);
        assert_eq!(pool.stats().medium_capacity_bytes, 0);
        assert!(pool.stats().allocated_bytes <= l.max_bytes);
        let ready = pool
            .prepare(
                1,
                &[Mutation {
                    head: None,
                    value: Some(b"inline"),
                }],
            )
            .unwrap();
        let head = ready.heads()[0];
        assert!(old.get(head).is_none());
        let current = ready.publish();
        assert_eq!(current.get(head), Some(b"inline".as_slice()));
    }

    #[test]
    fn guessed_future_and_stale_heads_never_escape_end_then_link_gates_during_reuse() {
        use std::sync::{atomic::AtomicBool, mpsc};
        let mut l = limits();
        l.max_heads = 1;
        l.max_versions = 4;
        l.max_views = 8;
        let (mut pool, first, heads) = PoolWriter::seed(0, &[&[1; 64]], l).unwrap();
        let mut stale = heads[0];
        let mut deleted = pool
            .prepare(
                1,
                &[Mutation {
                    head: Some(stale),
                    value: None,
                }],
            )
            .unwrap()
            .publish();
        drop(first);
        for epoch in (2..130).step_by(2) {
            let guessed = Head::from_u64(stale.to_u64() + (1u64 << 32)).unwrap();
            let fresh_deleted = std::thread::scope(|scope| {
                let reader_view = deleted.clone();
                let stop = Arc::new(AtomicBool::new(false));
                let reader_stop = stop.clone();
                let (tx, rx) = mpsc::channel();
                let reader = scope.spawn(move || {
                    let mut cache = reader_view.read_cache();
                    tx.send(()).unwrap();
                    while !reader_stop.load(Ordering::Acquire) {
                        assert!(reader_view.get(stale).is_none());
                        assert!(reader_view.get(guessed).is_none());
                        assert!(cache.get(stale).is_none());
                        assert!(cache.get(guessed).is_none());
                        std::hint::spin_loop();
                    }
                    assert!(reader_view.get(stale).is_none());
                    assert!(reader_view.get(guessed).is_none());
                });
                rx.recv().unwrap();
                pool.collect(100);
                let ready = pool
                    .prepare(
                        epoch,
                        &[Mutation {
                            head: None,
                            value: Some(&[8; 64]),
                        }],
                    )
                    .unwrap();
                assert_eq!(ready.heads()[0], guessed);
                let born = ready.publish();
                assert!(deleted.get(guessed).is_none());
                assert_eq!(born.get(guessed), Some([8; 64].as_slice()));
                let next_deleted = pool
                    .prepare(
                        epoch + 1,
                        &[Mutation {
                            head: Some(guessed),
                            value: None,
                        }],
                    )
                    .unwrap()
                    .publish();
                drop(born);
                stop.store(true, Ordering::Release);
                reader.join().unwrap();
                next_deleted
            });
            deleted = fresh_deleted;
            stale = guessed;
        }
        pool.collect(100);
        assert_eq!(pool.stats().active_heads, 0);
        assert!(pool.stats().allocated_bytes <= l.max_bytes);
    }

    #[test]
    fn epoch_and_head_generation_exhaustion_refuse_instead_of_wrapping() {
        let (mut pool, old, heads) = PoolWriter::seed(FOREVER - 1, &[b"end"], limits()).unwrap();
        assert!(matches!(
            pool.prepare(FOREVER, &[replace(heads[0], b"no")]),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(old.get(heads[0]), Some(b"end".as_slice()));
        let mut l = limits();
        l.max_heads = 1;
        let (mut pool, old, _) = PoolWriter::seed(0, &[], l).unwrap();
        pool.ensure_heads(1).unwrap();
        pool.shared
            .head(0)
            .unwrap()
            .link
            .store(u64::from(u32::MAX) << 32, Ordering::Relaxed);
        assert!(matches!(
            pool.prepare(
                1,
                &[Mutation {
                    head: None,
                    value: Some(b"no")
                }]
            ),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(pool.epoch(), 0);
        assert!(
            old.get(Head::from_u64((u64::from(u32::MAX) << 32) | 1).unwrap())
                .is_none()
        );
    }
    #[test]
    fn read_cache_borrows_survive_retargeting_lazy_growth_and_cache_drop() {
        let l = Limits {
            max_bytes: 8 << 20,
            max_heads: 1024,
            max_versions: 2048,
            max_views: 32,
        };
        let values: Vec<Vec<u8>> = (0..HEAD_CHUNK + 1)
            .map(|i| {
                let lengths = [64, 0, 8, 17, 129];
                vec![i as u8; lengths[i % lengths.len()]]
            })
            .collect();
        let refs: Vec<_> = values.iter().map(Vec::as_slice).collect();
        let (mut pool, old, heads) = PoolWriter::seed(0, &refs, l).unwrap();
        let charged = pool.stats().allocated_bytes;
        let mut cache = old.read_cache();
        assert_eq!(pool.stats().allocated_bytes, charged);
        let borrowed = cache.get(heads[0]).unwrap();
        let address = borrowed.as_ptr();
        for i in (0..heads.len()).rev() {
            assert_eq!(cache.get(heads[i]), Some(values[i].as_slice()));
        }
        let edits = vec![
            Mutation {
                head: None,
                value: Some(&[77; 64])
            };
            HEAD_CHUNK + 1
        ];
        let ready = pool.prepare(1, &edits).unwrap();
        let new_heads = ready.heads().to_vec();
        let current = ready.publish();
        assert!(pool.stats().initialized_heads > 2 * HEAD_CHUNK);
        assert!(pool.stats().initialized_versions > 4 * VERSION_CHUNK);
        pool.collect(10000);
        for head in &new_heads {
            assert!(cache.get(*head).is_none());
        }
        let changed = pool
            .prepare(2, &[replace(heads[0], &[88; 64])])
            .unwrap()
            .publish();
        assert_eq!(cache.get(heads[0]), Some(values[0].as_slice()));
        let mut latest = changed.read_cache();
        assert_eq!(latest.get(heads[0]), Some([88; 64].as_slice()));
        for head in &new_heads {
            assert_eq!(latest.get(*head), Some([77; 64].as_slice()));
        }
        drop(current);
        drop(cache);
        assert_eq!(borrowed, values[0]);
        assert_eq!(borrowed.as_ptr(), address);
        assert!(pool.stats().allocated_bytes <= l.max_bytes);
    }

    #[test]
    fn read_cache_never_caches_missing_chunks_or_medium_sidecars() {
        let l = Limits {
            max_bytes: 2 << 20,
            max_heads: 1024,
            max_versions: 2048,
            max_views: 32,
        };
        let (mut pool, old, heads) = PoolWriter::seed(0, &[b"inline"], l).unwrap();
        let head = heads[0];
        let mut cache = old.read_cache();
        let (head_index, _) = head.parts();
        let packed = pool
            .shared
            .head(head_index)
            .unwrap()
            .link
            .load(Ordering::Acquire);
        let version_index = packed as u32 as usize - 1;
        let (chunk, _) = cache.version(version_index).unwrap();
        assert!(cache.medium(version_index, chunk).is_none());
        assert!(cache.medium.is_none());
        assert!(cache.head(HEAD_CHUNK + 1).is_none());
        let changed = pool
            .prepare(1, &[replace(head, &[99; 64])])
            .unwrap()
            .publish();
        assert!(cache.medium(version_index, chunk).is_some());
        assert_eq!(cache.get(head), Some(b"inline".as_slice()));
        assert_eq!(changed.read_cache().get(head), Some([99; 64].as_slice()));
        let edits = vec![
            Mutation {
                head: None,
                value: Some(b"new")
            };
            HEAD_CHUNK + 1
        ];
        let new = pool.prepare(2, &edits).unwrap().publish();
        assert!(cache.head(HEAD_CHUNK + 1).is_some());
        assert_eq!(cache.get(head), Some(b"inline".as_slice()));
        assert_eq!(new.read_cache().get(head), Some([99; 64].as_slice()));
    }

    #[test]
    fn guard_control_remains_charged_until_private_queue_final_drop() {
        let (mut pool, first, heads) = PoolWriter::seed(0, &[b"one"], limits()).unwrap();
        let second = pool
            .prepare(1, &[replace(heads[0], b"two")])
            .unwrap()
            .publish();
        let before = pool.stats().allocated_bytes;
        drop(first);
        // Last public View drop cannot destroy the queue-owned control.
        assert_eq!(pool.stats().allocated_bytes, before);
        assert_eq!(pool.collect(1), 1);
        assert_eq!(pool.stats().allocated_bytes, before - GUARD_BYTES);
        assert_eq!(pool.oldest_view_epoch(), second.epoch());
        let budget = pool.shared.budget.clone();
        let before_owner_drop = budget.used.load(Ordering::Relaxed);
        let clone = second.clone();
        drop(pool);
        // Removing the queue owner does not release a live View's charge.
        assert_eq!(budget.used.load(Ordering::Relaxed), before_owner_drop);
        drop(second);
        assert_eq!(budget.used.load(Ordering::Relaxed), before_owner_drop);
        assert_eq!(clone.get(heads[0]), Some(b"two".as_slice()));
        drop(clone);
        assert_eq!(
            budget.used.load(Ordering::Relaxed),
            before_owner_drop - GUARD_BYTES
        );
    }
    #[test]
    fn successor_proof_rejects_foreign_pool_and_historical_view() {
        let (mut pool, old, heads) = PoolWriter::seed(0, &[b"a"], limits()).unwrap();
        let (_foreign, foreign, foreign_heads) = PoolWriter::seed(0, &[b"a"], limits()).unwrap();
        assert_eq!(heads, foreign_heads);
        let ready = pool.prepare(1, &[replace(heads[0], b"b")]).unwrap();
        assert!(ready.is_successor_of(&old));
        assert!(!ready.is_successor_of(&foreign));
        let current = ready.publish();
        let ready = pool.prepare(2, &[replace(heads[0], b"c")]).unwrap();
        assert!(ready.is_successor_of(&current));
        assert!(!ready.is_successor_of(&old));
        assert!(!ready.is_successor_of(&foreign));
    }
}
