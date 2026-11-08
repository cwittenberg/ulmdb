//! Safe ownership-transfer baseline: no unrestricted pool slice or mutable map escapes.
use super::{
    directory::View,
    read_index::{ReadIndex, TINY_INDEX_BYTES},
};
use crate::{Error, Result};
use memmap2::{MmapMut, MmapOptions};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
pub const PAGE: usize = super::wal_codec::PAGE;
pub const MAX_GENERATION: u64 = super::wal_codec::MAX_GENERATION;
pub const MASK: u64 = super::wal_codec::PHYSICAL_MASK;
pub const MAX_SLOTS: usize = super::wal_codec::MAX_SLOTS;
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub kv_pages: usize,
    pub tiny_pages: usize,
    /// Fixed pool maps plus decoded overflow mappings. Overflow additionally
    /// caps this combined mapped allowance at256MiB; old borrows retain charge.
    pub max_reserved_bytes: u64,
    pub max_metadata_bytes: u64,
    pub max_views: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct VersionKey {
    pub slot: u32,
    pub generation: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct Stats {
    pub physical_incarnation: u128,
    pub page_size: usize,
    pub maps_created: usize,
    pub rounded_mapping_bytes: u64,
    pub free_pages: usize,
    pub unused_tiny_pages: usize,
    pub held_maps: usize,
    pub quarantined_maps: usize,
    pub exhausted_maps: usize,
    pub allocations: u64,
    pub reused_pages: u64,
    pub poisoned: bool,
    pub view_roots: usize,
    pub metadata_bound_bytes: u64,
    pub retired_tiny_maps: usize,
}
struct Free {
    map: MmapMut,
    #[cfg(feature = "resident-metadata-reuse")]
    ranges: Vec<(u16, u16)>,
    slot: u32,
    generation: u64,
}
struct State {
    free: Vec<Free>,
    tiny: Vec<Free>,
    quarantine: Vec<Free>,
    exhausted: Vec<Free>,
    retired_tiny: Vec<Free>,
    allocated: u64,
    reused: u64,
}
pub struct Pool {
    pub(super) identity: u128,
    creator: u32,
    config: Config,
    page_size: usize,
    rounded_bytes: u64,
    metadata_bound: u64,
    pub(super) extent_quota: Arc<super::private_quota::Quota>,
    pub(super) private_quota: Arc<super::private_quota::Quota>,
    views: AtomicUsize,
    poisoned: AtomicBool,
    tiny_reuse: AtomicBool,
    state: Mutex<State>,
    transform: Option<Arc<crate::transform::TransformContext>>,
    _decoded_lease: Option<crate::transform::DecodedLease>,
}
impl Pool {
    /// Compile-time selection only; no lookup or mutation instrumentation.
    pub const fn direct_page_patch_enabled() -> bool {
        cfg!(feature = "resident-direct-page-patch")
    }
    pub fn create(config: Config) -> Result<Arc<Self>> {
        Self::create_with_transform(config, None)
    }
    pub fn create_with_transform(
        config: Config,
        transform: Option<Arc<crate::transform::TransformContext>>,
    ) -> Result<Arc<Self>> {
        let count = config
            .kv_pages
            .checked_add(config.tiny_pages)
            .filter(|n| *n > 0 && *n <= MAX_SLOTS)
            .ok_or(Error::LimitExceeded)?;
        if config.kv_pages == 0 {
            return Err(Error::InvalidInput("resident pool needs reusable pages"));
        }
        if config.max_views < 2 || config.max_views > 4096 {
            return Err(Error::InvalidInput("resident view budget"));
        }
        // Conservative container bound, not a promise about allocator/kernel RSS.
        // Ranges cap256/page; queues capN; private page records capN; directory
        // roots/chunks capmax_views. No full-history registry is retained.
        let per_slot = std::mem::size_of::<Version>()
            + 256 * 4
            + 5 * std::mem::size_of::<Free>()
            + 4 * std::mem::size_of::<super::store::Private>()
            + 256;
        #[cfg(feature = "resident-metadata-reuse")]
        let per_slot = per_slot + std::mem::size_of::<(usize, super::wal_codec::SlotState)>();
        let directory_bound = super::directory::metadata_bound_bytes(count)
            .and_then(|n| n.checked_mul(config.max_views as u64))
            .ok_or(Error::LimitExceeded)?;
        let metadata_bound = (per_slot as u64)
            .checked_mul(count as u64)
            .and_then(|n| n.checked_add(directory_bound))
            .and_then(|n| n.checked_add(config.tiny_pages as u64 * TINY_INDEX_BYTES as u64))
            .and_then(|n| n.checked_add(private_quota_header_bytes()))
            .filter(|n| *n <= config.max_metadata_bytes)
            .ok_or(Error::LimitExceeded)?;

        let page_size = crate::residency::os::page_size()?;
        let rounded = PAGE
            .div_ceil(page_size)
            .checked_mul(page_size)
            .ok_or(Error::LimitExceeded)?;
        let rounded_bytes = (rounded as u64)
            .checked_mul(count as u64)
            .filter(|n| *n <= config.max_reserved_bytes)
            .ok_or(Error::LimitExceeded)?;
        let decoded_lease = transform
            .as_ref()
            .map(|context| context.reserve_resident_pool(rounded_bytes))
            .transpose()?;
        let mut free = Vec::new();
        let mut tiny = Vec::new();
        let mut quarantine = Vec::new();
        let mut exhausted = Vec::new();
        let mut retired_tiny = Vec::new();
        for v in [
            &mut free,
            &mut tiny,
            &mut quarantine,
            &mut exhausted,
            &mut retired_tiny,
        ] {
            v.try_reserve_exact(count)
                .map_err(|_| Error::LimitExceeded)?;
        }
        for slot in (0..count).rev() {
            let mut map = MmapOptions::new().len(PAGE).map_anon()?;
            map.fill(0);
            #[cfg(feature = "resident-metadata-reuse")]
            let ranges = {
                let mut ranges = Vec::new();
                if slot < config.kv_pages {
                    ranges
                        .try_reserve_exact(1)
                        .map_err(|_| Error::LimitExceeded)?;
                }
                ranges
            };
            let f = Free {
                map,
                #[cfg(feature = "resident-metadata-reuse")]
                ranges,
                slot: slot as u32,
                generation: 0,
            };
            if slot < config.kv_pages {
                free.push(f)
            } else {
                tiny.push(f)
            }
        }
        Ok(Arc::new(Self {
            identity: crate::new_identity()?,
            creator: std::process::id(),
            config,
            page_size,
            rounded_bytes,
            metadata_bound,
            extent_quota: super::private_quota::Quota::new(
                config
                    .max_reserved_bytes
                    .min(256 << 20)
                    .saturating_sub(rounded_bytes),
            ),
            private_quota: super::private_quota::Quota::new(
                config.max_metadata_bytes - metadata_bound,
            ),
            views: AtomicUsize::new(0),
            poisoned: AtomicBool::new(false),
            tiny_reuse: AtomicBool::new(false),
            state: Mutex::new(State {
                free,
                tiny,
                quarantine,
                exhausted,
                retired_tiny,
                allocated: 0,
                reused: 0,
            }),
            transform,
            _decoded_lease: decoded_lease,
        }))
    }
    pub(super) fn check_creator(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited Resident pool"));
        }
        Ok(())
    }
    pub(super) fn ensure_live(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited Resident pool"));
        }
        if self.state.is_poisoned() {
            self.poisoned.store(true, Ordering::Release);
        }
        if self.poisoned.load(Ordering::Acquire) {
            Err(Error::OutcomeUnknown)
        } else {
            Ok(())
        }
    }
    pub fn empty(self: &Arc<Self>) -> Result<View> {
        View::empty(Arc::clone(self))
    }
    pub fn stats(&self) -> Result<Stats> {
        let s = self.state.lock().map_err(|_| Error::OutcomeUnknown)?;
        let count = self.config.kv_pages + self.config.tiny_pages;
        Ok(Stats {
            physical_incarnation: self.identity,
            page_size: self.page_size,
            maps_created: count,
            rounded_mapping_bytes: self.rounded_bytes,
            free_pages: s.free.len(),
            unused_tiny_pages: s.tiny.len(),
            held_maps: count
                - s.free.len()
                - s.tiny.len()
                - s.quarantine.len()
                - s.exhausted.len()
                - s.retired_tiny.len(),
            quarantined_maps: s.quarantine.len(),
            exhausted_maps: s.exhausted.len(),
            allocations: s.allocated,
            reused_pages: s.reused,
            poisoned: self.poisoned.load(Ordering::Acquire),
            view_roots: self.views.load(Ordering::Acquire),
            metadata_bound_bytes: self.metadata_bound,
            retired_tiny_maps: s.retired_tiny.len(),
        })
    }
    /// Activate only after selecting or authenticating a checkpointed RW3/RW4
    /// owner. Old anonymous views keep their pages; only last-Arc release can
    /// return a Tiny page. Reuse zeros its bytes and advances its generation.
    pub fn enable_checkpoint_tiny_reuse(&self) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| Error::OutcomeUnknown)?;
        self.ensure_live()?;
        self.tiny_reuse.store(true, Ordering::Release);
        while let Some(page) = state.retired_tiny.pop() {
            state.tiny.push(page);
        }
        Ok(())
    }
    pub fn checkpoint_tiny_reuse_enabled(&self) -> bool {
        self.tiny_reuse.load(Ordering::Acquire)
    }
    pub fn quarantine(&self) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.poisoned.store(true, Ordering::Release);
        while let Some(f) = s.free.pop() {
            s.quarantine.push(f);
        }
        while let Some(f) = s.tiny.pop() {
            s.quarantine.push(f);
        }
    }
    pub(super) fn reserve(self: &Arc<Self>, tiny: bool) -> Result<Permit> {
        let mut s = self.state.lock().map_err(|_| Error::OutcomeUnknown)?;
        self.ensure_live()?;
        let allocated = s.allocated.checked_add(1).ok_or(Error::LimitExceeded)?;
        let reused = s.reused.checked_add(1).ok_or(Error::LimitExceeded)?;
        loop {
            let mut f = if tiny { s.tiny.pop() } else { s.free.pop() }.ok_or(Error::Busy)?;
            if f.generation == MAX_GENERATION {
                s.exhausted.push(f);
                continue;
            }
            s.allocated = allocated;
            if !tiny && f.generation > 0 {
                s.reused = reused;
            }
            if tiny && f.generation > 0 {
                f.map.fill(0);
            }
            let generation = f.generation + 1;
            return Ok(Permit {
                pool: Arc::clone(self),
                slot: f.slot,
                generation,
                map: Some(f.map),
                #[cfg(not(feature = "resident-metadata-reuse"))]
                ranges: Vec::new(),
                #[cfg(feature = "resident-metadata-reuse")]
                ranges: f.ranges,
                cursor: 0,
                tiny,
            });
        }
    }
    pub fn transform_context(&self) -> Option<&Arc<crate::transform::TransformContext>> {
        self.transform.as_ref()
    }
    pub fn geometry(&self) -> (usize, usize) {
        (self.config.kv_pages, self.config.tiny_pages)
    }
    pub(super) fn replay_permit(self: &Arc<Self>, key: VersionKey) -> Result<Permit> {
        self.ensure_live()?;
        if key.generation == 0
            || key.generation > MAX_GENERATION
            || key.slot as usize >= self.config.kv_pages + self.config.tiny_pages
        {
            return Err(Error::Corrupt("resident replay slot/generation"));
        }
        let tiny = key.slot as usize >= self.config.kv_pages;
        let mut state = self.state.lock().map_err(|_| Error::OutcomeUnknown)?;
        let queue = if tiny {
            &mut state.tiny
        } else {
            &mut state.free
        };
        let at = queue
            .iter()
            .position(|f| f.slot == key.slot)
            .ok_or(Error::Corrupt(
                "resident replay slot is not physically free",
            ))?;
        if queue[at].generation >= key.generation {
            return Err(Error::Corrupt("resident replay generation did not advance"));
        }
        let f = queue.swap_remove(at);
        Ok(Permit {
            pool: Arc::clone(self),
            slot: key.slot,
            generation: key.generation,
            map: Some(f.map),
            #[cfg(not(feature = "resident-metadata-reuse"))]
            ranges: Vec::new(),
            #[cfg(feature = "resident-metadata-reuse")]
            ranges: f.ranges,
            cursor: 0,
            tiny,
        })
    }
    pub(super) fn restore_retired(&self, slot: u32, generation: u64) -> Result<()> {
        if generation == 0 || generation > MAX_GENERATION {
            return Err(Error::Corrupt("resident checkpoint generation"));
        }
        let tiny = slot as usize >= self.config.kv_pages;
        let mut state = self.state.lock().map_err(|_| Error::OutcomeUnknown)?;
        let queue = if tiny {
            &mut state.tiny
        } else {
            &mut state.free
        };
        let index = queue
            .iter()
            .position(|f| f.slot == slot)
            .ok_or(Error::Corrupt("resident checkpoint retired slot"))?;
        if queue[index].generation != 0 {
            return Err(Error::Corrupt("resident checkpoint restored twice"));
        }
        queue[index].generation = generation;
        if tiny {
            let page = queue.swap_remove(index);
            state.retired_tiny.push(page);
        }
        Ok(())
    }
    pub(super) fn view_lease(self: &Arc<Self>) -> Result<ViewLease> {
        self.ensure_live()?;
        self.views
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < self.config.max_views).then_some(n + 1)
            })
            .map_err(|_| Error::Busy)?;
        Ok(ViewLease(Arc::clone(self)))
    }
    fn release(&self, f: Free, tiny: bool, published: bool) {
        if self.creator != std::process::id() {
            drop(f);
            return;
        }
        let mut s = match self.state.lock() {
            Ok(s) => s,
            Err(e) => {
                let s = e.into_inner();
                self.poisoned.store(true, Ordering::Release);
                s
            }
        };
        // Every queue reserved the pool's full map count before any publication.
        // No Arc/Pool cycle: queues own only maps, never versions or owners.
        if self.poisoned.load(Ordering::Acquire) {
            s.quarantine.push(f)
        } else if tiny && published && !self.tiny_reuse.load(Ordering::Acquire) {
            s.retired_tiny.push(f)
        } else if tiny {
            s.tiny.push(f)
        } else {
            s.free.push(f)
        }
    }
}
pub(super) struct ViewLease(Arc<Pool>);
impl Drop for ViewLease {
    fn drop(&mut self) {
        self.0.views.fetch_sub(1, Ordering::AcqRel);
    }
}
pub(super) struct Permit {
    pool: Arc<Pool>,
    pub slot: u32,
    pub generation: u64,
    map: Option<MmapMut>,
    pub ranges: Vec<(u16, u16)>,
    pub cursor: usize,
    tiny: bool,
}
impl Permit {
    pub(super) fn snapshot_bytes(&self) -> &[u8] {
        &self.map.as_ref().expect("live resident permit")[..]
    }
    pub fn read(&self, offset: usize, len: usize) -> Result<&[u8]> {
        check(&self.ranges, offset, len)?;
        self.map
            .as_ref()
            .unwrap()
            .get(offset..offset + len)
            .ok_or(Error::Corrupt("resident private map range"))
    }
    pub fn exact(&self, offset: usize, len: usize) -> bool {
        self.ranges
            .iter()
            .any(|&(a, n)| a as usize == offset && n as usize == len)
    }
    pub fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<()> {
        let end = offset
            .checked_add(bytes.len())
            .filter(|n| *n <= PAGE)
            .ok_or(Error::LimitExceeded)?;
        self.map.as_mut().unwrap()[offset..end].copy_from_slice(bytes);
        Ok(())
    }
    pub fn seal(mut self) -> Arc<Version> {
        let read_index = ReadIndex::new(self.tiny, &self.ranges);
        Arc::new(Version {
            read_index,
            pool: Arc::clone(&self.pool),
            slot: self.slot,
            generation: self.generation,
            map: self.map.take(),
            ranges: std::mem::take(&mut self.ranges),
            tiny: self.tiny,
            published: AtomicBool::new(false),
            extent: std::sync::OnceLock::new(),
            extent_lock: Mutex::new(()),
        })
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        if let Some(map) = self.map.take() {
            #[cfg(feature = "resident-metadata-reuse")]
            let ranges = {
                let mut ranges = std::mem::take(&mut self.ranges);
                ranges.clear();
                // All admitted objects have <=256 ranges. Never retain a larger
                // allocator capacity from an unusual cold/import caller.
                if ranges.capacity() > 256 {
                    ranges = Vec::new();
                }
                ranges
            };
            self.pool.release(
                Free {
                    map,
                    #[cfg(feature = "resident-metadata-reuse")]
                    ranges,
                    slot: self.slot,
                    generation: self.generation,
                },
                self.tiny,
                false,
            );
        }
    }
}
pub(super) struct Version {
    read_index: ReadIndex,
    pool: Arc<Pool>,
    pub slot: u32,
    pub generation: u64,
    map: Option<MmapMut>,
    ranges: Vec<(u16, u16)>,
    tiny: bool,
    published: AtomicBool,
    pub(super) extent: std::sync::OnceLock<Arc<super::extent::Extent>>,
    pub(super) extent_lock: Mutex<()>,
}
impl Version {
    pub fn key(&self) -> VersionKey {
        VersionKey {
            slot: self.slot,
            generation: self.generation,
        }
    }
    pub fn publish(&self) {
        self.published.store(true, Ordering::Release);
    }
    pub fn exact(&self, generation: u64, offset: usize, len: usize) -> bool {
        generation == self.generation
            && self
                .ranges
                .iter()
                .any(|&(a, n)| a as usize == offset && n as usize == len)
    }
    pub fn literal_bytes(&self) -> &[u8] {
        self.map.as_ref().unwrap()
    }
    pub fn literal_ranges(&self) -> &[(u16, u16)] {
        &self.ranges
    }
    pub fn belongs_to(&self, pool: &Arc<Pool>) -> bool {
        Arc::ptr_eq(&self.pool, pool)
    }
    pub fn read(&self, generation: u64, offset: usize, len: usize) -> Result<&[u8]> {
        if generation != self.generation {
            return Err(Error::Corrupt("resident stale generation"));
        }
        self.read_index.check(&self.ranges, offset, len)?;
        // MmapMut stays immutably owned here. No permit can regain its ownership
        // until this final Arc owner and every borrow derived from it have gone.
        self.map
            .as_ref()
            .unwrap()
            .get(offset..offset + len)
            .ok_or(Error::Corrupt("resident map range"))
    }
}
impl Drop for Version {
    fn drop(&mut self) {
        drop(self.extent.take());
        if let Some(map) = self.map.take() {
            #[cfg(feature = "resident-metadata-reuse")]
            let ranges = {
                let mut ranges = std::mem::take(&mut self.ranges);
                ranges.clear();
                // All admitted objects have <=256 ranges. Never retain a larger
                // allocator capacity from an unusual cold/import caller.
                if ranges.capacity() > 256 {
                    ranges = Vec::new();
                }
                ranges
            };
            self.pool.release(
                Free {
                    map,
                    #[cfg(feature = "resident-metadata-reuse")]
                    ranges,
                    slot: self.slot,
                    generation: self.generation,
                },
                self.tiny,
                self.published.load(Ordering::Acquire),
            );
        }
    }
}
pub(super) fn check(ranges: &[(u16, u16)], offset: usize, len: usize) -> Result<()> {
    let end = offset
        .checked_add(len)
        .filter(|n| *n <= PAGE)
        .ok_or(Error::Corrupt("resident cross-page extent"))?;
    let i = ranges.partition_point(|r| r.0 as usize <= offset);
    if i == 0 {
        return Err(Error::Corrupt("resident missing object membership"));
    }
    let (start, n) = ranges[i - 1];
    if offset < start as usize || end > start as usize + n as usize {
        return Err(Error::Corrupt("resident cross-object extent"));
    }
    Ok(())
}
pub(super) fn tagged(slot: u32, generation: u64, within: usize) -> crate::Reference {
    crate::Reference {
        segment: 1,
        offset: (generation << 28) | ((u64::from(slot) + 1) * PAGE as u64 + within as u64),
    }
}
pub(super) fn unpack(at: crate::Reference) -> Result<(u32, u64, usize)> {
    let generation = at.offset >> 28;
    let physical = at.offset & MASK;
    if at.segment != 1
        || generation == 0
        || physical < PAGE as u64
        || physical > ((MAX_SLOTS + 1) * PAGE) as u64
    {
        return Err(Error::Corrupt("resident tagged reference"));
    }
    Ok((
        (physical / PAGE as u64 - 1) as u32,
        generation,
        (physical % PAGE as u64) as usize,
    ))
}

fn private_quota_header_bytes() -> u64 {
    (2 * (std::mem::size_of::<super::private_quota::Quota>() + 2 * std::mem::size_of::<usize>()))
        as u64
}
