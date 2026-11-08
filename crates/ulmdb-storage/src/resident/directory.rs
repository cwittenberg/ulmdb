//! Persistent membership; the original 4096-slot directory remains inline.
use super::pool::{Pool, Version, VersionKey, ViewLease, unpack};
use crate::{Error, ReadStore, Reference, Result};
use std::sync::Arc;
#[derive(Clone)]
struct Chunk {
    entries: [Option<Arc<Version>>; 64],
}
impl Default for Chunk {
    fn default() -> Self {
        Self {
            entries: std::array::from_fn(|_| None),
        }
    }
}
struct Root {
    chunks: [Option<Arc<Chunk>>; 64],
    overflow: Option<Arc<Overflow>>,
    count: usize,
    _lease: ViewLease,
}
#[derive(Clone)]
struct Group {
    chunks: [Option<Arc<Chunk>>; 64],
}
impl Default for Group {
    fn default() -> Self {
        Self {
            chunks: std::array::from_fn(|_| None),
        }
    }
}
#[derive(Clone)]
struct Overflow {
    // Group zero is Root.chunks. Fifteen further groups cover 65536 slots;
    // actual admission is restricted by the physical reference/config bound.
    groups: [Option<Arc<Group>>; 15],
}
impl Default for Overflow {
    fn default() -> Self {
        Self {
            groups: std::array::from_fn(|_| None),
        }
    }
}
/// Conservative capacity admission, not allocator/RSS measurement. Keep the
/// historical small-pool charge; large views additionally cover every possible
/// overflow node, its Arc control block and an allocation alignment envelope.
pub(super) fn metadata_bound_bytes(slots: usize) -> Option<u64> {
    if slots == 0 || slots > super::pool::MAX_SLOTS || slots > 65536 {
        return None;
    }
    let chunks = slots.div_ceil(64);
    if chunks <= 64 {
        return Some(66560);
    }
    let groups = chunks.div_ceil(64) - 1;
    let envelope = 4 * std::mem::size_of::<usize>();
    // Charge twice each additional Chunk payload, matching the conservative
    // small-directory envelope, plus all additional routing allocations.
    let extra_chunk = 2 * std::mem::size_of::<Chunk>() + envelope;
    let extra_group = std::mem::size_of::<Group>() + envelope;
    66560u64
        .checked_add(((chunks - 64) as u64).checked_mul(extra_chunk as u64)?)?
        .checked_add((groups as u64).checked_mul(extra_group as u64)?)?
        .checked_add((std::mem::size_of::<Overflow>() + envelope) as u64)
}
impl Root {
    fn empty(pool: &Arc<Pool>) -> Result<Self> {
        Ok(Self {
            chunks: std::array::from_fn(|_| None),
            overflow: None,
            count: 0,
            _lease: pool.view_lease()?,
        })
    }
    fn chunk_mut(&mut self, slot: usize, create: bool) -> Result<&mut Option<Arc<Chunk>>> {
        if let Some(chunk) = self.chunks.get_mut(slot / 64) {
            return Ok(chunk);
        }
        let group_index = (slot / 4096)
            .checked_sub(1)
            .filter(|i| *i < 15)
            .ok_or(Error::Corrupt("resident directory slot bounds"))?;
        if !create && self.overflow.is_none() {
            return Err(Error::Corrupt("resident retire missing group"));
        }
        let overflow = self
            .overflow
            .get_or_insert_with(|| Arc::new(Overflow::default()));
        let group = &mut Arc::make_mut(overflow).groups[group_index];
        if !create && group.is_none() {
            return Err(Error::Corrupt("resident retire missing group"));
        }
        let group = group.get_or_insert_with(|| Arc::new(Group::default()));
        Ok(&mut Arc::make_mut(group).chunks[(slot / 64) % 64])
    }
}
#[derive(Clone)]
pub struct View {
    pub(super) pool: Arc<Pool>,
    root: Arc<Root>,
}
impl View {
    /// Cold admission observation for optional readers that cannot retain
    /// transformed/decoded source bytes outside their transform quota policy.
    pub fn has_transform(&self) -> bool {
        self.pool.transform_context().is_some()
    }

    pub(super) fn empty(pool: Arc<Pool>) -> Result<Self> {
        let root = Arc::new(Root::empty(&pool)?);
        Ok(Self { pool, root })
    }
    pub(super) fn exact(&self, at: Reference, len: usize) -> Result<bool> {
        let (slot, generation, offset) = unpack(at)?;
        Ok(self
            .version(slot)
            .is_some_and(|v| v.exact(generation, offset, len)))
    }
    /// Cold typed-graph observation: require an entire canonical allocation,
    /// rather than merely a readable subspan. This grants no reclaim authority.
    pub fn allocation_key(&self, at: Reference, len: usize) -> Result<VersionKey> {
        if at.segment != 1 {
            return self.overflow_allocation(at, len);
        }
        let (slot, generation, offset) = unpack(at)?;
        let v = self
            .version(slot)
            .ok_or(Error::Corrupt("typed object absent"))?;
        if !v.exact(generation, offset, len) {
            return Err(Error::Corrupt("typed object is not exact allocation"));
        }
        Ok(v.key())
    }
    /// Cold source identity, including physical pool and immutable membership root.
    pub fn same_snapshot(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.pool, &other.pool) && Arc::ptr_eq(&self.root, &other.root)
    }
    pub fn slot_capacity(&self) -> usize {
        let (a, b) = self.pool.geometry();
        a + b
    }
    pub fn contains_version(&self, key: VersionKey) -> bool {
        self.version(key.slot).is_some_and(|v| v.key() == key)
    }
    pub fn member_count(&self) -> usize {
        self.root.count
    }
    #[inline]
    pub(super) fn version(&self, slot: u32) -> Option<&Arc<Version>> {
        let slot = slot as usize;
        if let Some(chunk) = self.root.chunks.get(slot / 64) {
            return chunk.as_ref()?.entries[slot % 64].as_ref();
        }
        self.version_overflow(slot)
    }
    // Preserve the original inline low4096 lookup. Capacity/group checks remain
    // mandatory for overflow; their larger body must not enlarge every read.
    #[cold]
    #[inline(never)]
    fn version_overflow(&self, slot: usize) -> Option<&Arc<Version>> {
        if slot >= self.slot_capacity() {
            return None;
        }
        self.root
            .overflow
            .as_ref()?
            .groups
            .get((slot / 4096).checked_sub(1)?)?
            .as_ref()?
            .chunks[(slot / 64) % 64]
            .as_ref()?
            .entries[slot % 64]
            .as_ref()
    }
    pub(super) fn changed(
        &self,
        versions: impl IntoIterator<Item = Arc<Version>>,
        remove: &[VersionKey],
    ) -> Result<Self> {
        let mut root = Root {
            chunks: self.root.chunks.clone(),
            overflow: self.root.overflow.clone(),
            count: self.root.count,
            _lease: self.pool.view_lease()?,
        };
        for key in remove {
            if key.slot as usize >= self.slot_capacity() {
                return Err(Error::Corrupt("resident retire slot bounds"));
            }
            let ch = root
                .chunk_mut(key.slot as usize, false)?
                .as_mut()
                .ok_or(Error::Corrupt("resident retire missing chunk"))?;
            let entries = &mut Arc::make_mut(ch).entries;
            let entry = &mut entries[key.slot as usize % 64];
            if entry.as_ref().map(|v| v.key()) != Some(*key) {
                return Err(Error::Corrupt("resident retire wrong generation"));
            }
            *entry = None;
            root.count -= 1;
        }
        for v in versions {
            if !v.belongs_to(&self.pool) {
                return Err(Error::Corrupt("resident foreign owner"));
            }
            let slot = v.slot as usize;
            if slot >= self.slot_capacity() {
                return Err(Error::Corrupt("resident insert slot bounds"));
            }
            let ch = root
                .chunk_mut(slot, true)?
                .get_or_insert_with(|| Arc::new(Chunk::default()));
            let entry = &mut Arc::make_mut(ch).entries[slot % 64];
            if entry.is_some() {
                return Err(Error::Corrupt("resident overwrite live member"));
            }
            *entry = Some(v);
            root.count += 1;
        }
        Ok(Self {
            pool: Arc::clone(&self.pool),
            root: Arc::new(root),
        })
    }
    pub fn members(&self) -> impl Iterator<Item = VersionKey> + '_ {
        self.root
            .chunks
            .iter()
            .chain(
                self.root
                    .overflow
                    .iter()
                    .flat_map(|o| o.groups.iter().filter_map(Option::as_ref))
                    .flat_map(|g| g.chunks.iter()),
            )
            .filter_map(Option::as_ref)
            .flat_map(|c| c.entries.iter().filter_map(Option::as_ref).map(|v| v.key()))
    }
    /// Remove exactly the listed members without scanning unrelated slots.
    /// Keys must name existing generations in strictly increasing slot order.
    /// This checks physical membership only: the caller must separately establish
    /// logical retirement and durable publication before selecting this view.
    /// Old views and borrowed bytes keep their original immutable owners.
    pub fn without_versions(&self, remove: &[VersionKey]) -> Result<Self> {
        if remove.is_empty() {
            return Ok(self.clone());
        }
        if remove.len() > self.root.count {
            return Err(Error::Corrupt("resident removal exceeds membership"));
        }
        let mut previous = None;
        for key in remove {
            if previous.is_some_and(|slot| slot >= key.slot)
                || self.version(key.slot).map(|v| v.key()) != Some(*key)
            {
                return Err(Error::Corrupt("resident removal order/generation"));
            }
            previous = Some(key.slot);
        }
        self.changed(std::iter::empty(), remove)
    }
    pub fn retain(&self, keep: &std::collections::BTreeSet<VersionKey>) -> Result<Self> {
        let remove = self
            .members()
            .filter(|k| !keep.contains(k))
            .collect::<Vec<_>>();
        self.changed(std::iter::empty(), &remove)
    }
}
impl View {
    pub fn read_with_key(&self, at: Reference, len: usize) -> Result<(&[u8], VersionKey)> {
        self.read_with_sources(at, len).map(|(b, k, _)| (b, k))
    }
    pub fn read_with_sources(
        &self,
        at: Reference,
        len: usize,
    ) -> Result<(&[u8], VersionKey, &[super::extent::ExtentSource])> {
        self.read_with_sources_limited(at, len, usize::MAX)
    }
    pub fn read_with_sources_limited(
        &self,
        at: Reference,
        len: usize,
        limit: usize,
    ) -> Result<(&[u8], VersionKey, &[super::extent::ExtentSource])> {
        if at.segment != 1 {
            return self.read_overflow_limited(at, len, limit);
        }
        let (slot, generation, within) = unpack(at)?;
        if let Some(v) = self.version(slot)
            && let Ok(bytes) = v.read(generation, within, len)
        {
            return Ok((bytes, v.key(), &[]));
        }
        if len == 0
            && within == 0
            && slot > 0
            && let Some(v) = self.version(slot - 1)
            && let Ok(bytes) = v.read(generation, super::pool::PAGE, 0)
        {
            return Ok((bytes, v.key(), &[]));
        }
        Err(Error::Corrupt("resident reference absent from snapshot"))
    }
}
impl ReadStore for View {
    fn max_duplicate_key_size(&self) -> usize {
        4080
    }
    fn max_duplicate_value_size(&self) -> usize {
        65536
    }
    fn max_key_size(&self) -> usize {
        65536
    }
    fn max_value_size(&self) -> usize {
        super::extent::MAX_OVERFLOW_VALUE_BYTES
    }
    fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
        if at.segment != 1 {
            return self.read_overflow(at, len).map(|(b, _, _)| b);
        }
        // Ordinary lookups return only bytes; maintenance tracing uses the
        // separate wider bytes+VersionKey API above. Exact checks are identical.
        let (slot, generation, within) = unpack(at)?;
        if let Some(v) = self.version(slot)
            && let Ok(bytes) = v.read(generation, within, len)
        {
            return Ok(bytes);
        }
        if len == 0
            && within == 0
            && slot > 0
            && let Some(v) = self.version(slot - 1)
            && let Ok(bytes) = v.read(generation, super::pool::PAGE, 0)
        {
            return Ok(bytes);
        }
        Err(Error::Corrupt("resident reference absent from snapshot"))
    }
}

#[cfg(test)]
#[path = "directory_tests.rs"]
mod directory_tests;

#[cfg(test)]
#[path = "scalable_directory_tests.rs"]
mod scalable_directory_tests;

#[cfg(all(test, feature = "resident-metadata-reuse"))]
#[path = "metadata_reuse_directory_tests.rs"]
mod metadata_reuse_directory_tests;
