//! Actual index adapter. Whole pages reuse; tiny objects pack into private pages.
use super::{
    directory::View,
    pool::{PAGE, Permit, Pool, Version, tagged, unpack},
};
use crate::{Error, ReadStore, Reference, Result, WriteStore};
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};
#[cfg(all(
    feature = "resident-direct-page-patch",
    any(not(feature = "resident-fused-patch"), test)
))]
#[path = "direct_patch.rs"]
mod direct_patch;
#[path = "extent_write.rs"]
mod extent_write;
#[cfg(feature = "resident-fused-patch")]
#[path = "fused_patch.rs"]
mod fused_patch;
#[cfg(feature = "resident-direct-page-patch")]
#[path = "patch_many.rs"]
mod patch_many;
#[path = "savepoint.rs"]
mod savepoint;
pub use savepoint::ArenaCheckpoint;

pub struct Arena {
    pool: Arc<Pool>,
    base: View,
    private: super::private_workspace::Pages,
    tiny: Option<u32>,
    revision: u64,
    allocation_serial: u64,
    pending_extents: std::collections::BTreeMap<u64, super::extent::Pending>,
    journal: Option<savepoint::Journal>,
}
#[cfg(feature = "resident-metadata-reuse")]
#[path = "metadata_reuse.rs"]
mod metadata_reuse;
#[cfg(feature = "resident-metadata-reuse")]
use metadata_reuse::FrozenVersions;
#[cfg(not(feature = "resident-metadata-reuse"))]
type FrozenVersions = Vec<NewVersion>;
pub struct Prepared {
    source_base: View,
    view: View,
    new: FrozenVersions,
}
/// Immutable new-object data; copy_source is only a hint, never authority.
pub struct FrozenSlot<'a> {
    pub key: super::VersionKey,
    pub ranges: &'a [(u16, u16)],
    pub bytes: &'a [u8],
    pub copy_source: Option<super::VersionKey>,
}
pub(super) struct Private {
    permit: Permit,
    source: Option<super::VersionKey>,
    born: u64,
    protected: bool,
}
impl Private {
    pub(super) fn new(permit: Permit) -> Self {
        Self {
            permit,
            source: None,
            born: 0,
            protected: false,
        }
    }
}
impl Deref for Private {
    type Target = Permit;
    fn deref(&self) -> &Permit {
        &self.permit
    }
}
impl DerefMut for Private {
    fn deref_mut(&mut self) -> &mut Permit {
        &mut self.permit
    }
}
pub(super) struct NewVersion {
    version: Arc<Version>,
    source: Option<super::VersionKey>,
}
#[cfg(feature = "resident-metadata-reuse")]
impl NewVersion {
    pub(super) fn seal(p: Private) -> Option<Self> {
        (!p.ranges.is_empty()).then(|| Self {
            version: p.permit.seal(),
            source: p.source,
        })
    }
}
impl Pool {
    pub fn begin(self: &Arc<Self>, base: View) -> Result<Arena> {
        if !Arc::ptr_eq(self, &base.pool) {
            return Err(Error::Corrupt("resident foreign physical owner"));
        }
        self.ensure_live()?;
        Ok(Arena {
            pool: Arc::clone(self),
            base,
            private: super::private_workspace::Pages::new(self)?,
            tiny: None,
            revision: 0,
            allocation_serial: 0,
            pending_extents: Default::default(),
            journal: None,
        })
    }
}
#[cfg(feature = "resident-private-index")]
impl Pool {
    pub fn begin_with_private_cache(
        self: &Arc<Self>,
        base: View,
        cache: &Arc<super::PrivateCache>,
    ) -> Result<Arena> {
        if !Arc::ptr_eq(self, &base.pool) {
            return Err(Error::Corrupt("resident foreign physical owner"));
        }
        self.ensure_live()?;
        let private = cache.take(self)?;
        Ok(Arena {
            pool: Arc::clone(self),
            base,
            private,
            tiny: None,
            revision: 0,
            allocation_serial: 0,
            pending_extents: Default::default(),
            journal: None,
        })
    }
}
impl Arena {
    fn exact(&self, at: Reference, len: usize) -> Result<bool> {
        let (slot, generation, offset) = unpack(at)?;
        if let Some(p) = self.private.get(&slot)? {
            Ok(p.generation == generation && p.exact(offset, len))
        } else {
            self.base.exact(at, len)
        }
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    fn tick(&mut self) -> Result<()> {
        self.revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
        Ok(())
    }
    fn parts(&mut self, first: &[u8], second: &[u8], alignment: usize) -> Result<Reference> {
        self.pool.ensure_live()?;
        let len = first
            .len()
            .checked_add(second.len())
            .filter(|n| *n > 0 && *n <= PAGE)
            .ok_or(Error::LimitExceeded)?;
        if !alignment.is_power_of_two() || alignment > PAGE {
            return Err(Error::InvalidInput("resident allocation alignment"));
        }
        self.tick()?;
        let page = alignment == PAGE && len == PAGE;
        let slot = if page {
            let p = self.pool.reserve(false)?;
            let slot = p.slot;
            self.insert_private(slot, Private::new(p))?;
            slot
        } else {
            let active = if let Some(s) = self.tiny {
                self.private
                    .get(&s)?
                    .filter(|p| {
                        p.ranges.len() < 256
                            && p.cursor.div_ceil(alignment) * alignment + len <= PAGE
                    })
                    .map(|_| s)
            } else {
                None
            };
            if let Some(s) = active {
                s
            } else {
                let p = self.pool.reserve(true)?;
                let slot = p.slot;
                self.insert_private(slot, Private::new(p))?;
                self.tiny = Some(slot);
                slot
            }
        };
        self.remember_private(slot)?;
        let p = self.private.get_mut(&slot)?.unwrap();
        let offset = p.cursor.div_ceil(alignment) * alignment;
        p.ranges.try_reserve(1).map_err(|_| Error::LimitExceeded)?;
        p.write(offset, first)?;
        p.write(offset + first.len(), second)?;
        p.ranges.push((offset as u16, len as u16));
        p.cursor = offset + len;
        Ok(tagged(slot, p.generation, offset))
    }
    fn allocate_copy(
        &mut self,
        old: Reference,
        bytes: &[u8],
        alignment: usize,
    ) -> Result<Reference> {
        let at = self.allocate(bytes, alignment)?;
        if bytes.len() == PAGE && alignment == PAGE {
            let (source, generation, within) = unpack(old)?;
            if within == 0 && (source as usize) < self.pool.geometry().0 {
                let (slot, _, _) = unpack(at)?;
                self.private.get_mut(&slot)?.unwrap().source = Some(super::VersionKey {
                    slot: source,
                    generation,
                });
            }
        }
        Ok(at)
    }
    pub fn freeze(mut self) -> Result<Prepared> {
        let pending_extents = std::mem::take(&mut self.pending_extents);
        self.pool.ensure_live()?;
        if self.savepoint_depth() != 0 {
            return Err(Error::InvalidInput("active resident checkpoint"));
        }
        #[cfg(feature = "resident-metadata-reuse")]
        {
            self.private.seal_private()?;
            let view = self.base.changed(
                self.private.sealed().iter().map(|p| Arc::clone(&p.version)),
                &[],
            )?;
            let source_base = self.base.clone();
            let new = FrozenVersions::cached(self.private);
            view.install_extents(pending_extents)?;
            Ok(Prepared {
                source_base,
                view,
                new,
            })
        }
        #[cfg(not(feature = "resident-metadata-reuse"))]
        {
            let mut new = Vec::new();
            new.try_reserve_exact(self.private.len())
                .map_err(|_| Error::LimitExceeded)?;
            for p in self.private.drain_sorted()? {
                if !p.ranges.is_empty() {
                    new.push(NewVersion {
                        version: p.permit.seal(),
                        source: p.source,
                    });
                }
            }
            let view = self
                .base
                .changed(new.iter().map(|p| Arc::clone(&p.version)), &[])?;
            self.private.finish()?;
            view.install_extents(pending_extents)?;
            Ok(Prepared {
                source_base: self.base.clone(),
                view,
                new,
            })
        }
    }
}
impl Prepared {
    /// Exact private source binding; no raw Reference or hint grants this identity.
    pub fn was_based_on(&self, base: &View) -> bool {
        self.source_base.same_snapshot(base)
    }

    /// Frozen writer metadata only; serving Version/View layout is unchanged.
    pub fn slot(&self, index: usize) -> Option<FrozenSlot<'_>> {
        self.new.get(index).map(|n| FrozenSlot {
            key: n.version.key(),
            ranges: n.version.literal_ranges(),
            bytes: n.version.literal_bytes(),
            copy_source: n.source,
        })
    }
    /// Borrow capability only; this is NOT proof of any durable WAL selection.
    pub fn view(&self) -> &View {
        &self.view
    }
    /// Facade calls only after a complete durable decision. Allocation/free/drop
    /// handling here is safe independently of a caller's durability contract.
    pub fn activate(self) -> View {
        for v in &self.new {
            v.version.publish();
        }
        self.view
    }
    pub fn literals(
        &self,
        state: &super::wal_codec::ReplayState,
    ) -> Result<Vec<super::wal_codec::LiteralSlot>> {
        use super::wal_codec::{LiteralSlot, SlotState};
        let geometry = self.view.pool.geometry();
        let boot = state.boot();
        if geometry != (boot.kv_pages as usize, boot.tiny_pages as usize) {
            return Err(Error::Corrupt("resident literal foreign geometry"));
        }
        let mut out = Vec::new();
        out.try_reserve_exact(self.new.len())
            .map_err(|_| Error::LimitExceeded)?;
        for new in &self.new {
            let v = &new.version;
            let expected = match state.slot(v.slot) {
                Some(SlotState::Free) => 0,
                Some(SlotState::Retired(_)) if v.slot >= boot.kv_pages => 0,
                Some(SlotState::Retired(g)) => g,
                _ => return Err(Error::Corrupt("resident private slot is logically live")),
            };
            out.push(LiteralSlot {
                key: v.key(),
                expected_previous_generation: expected,
                ranges: v.literal_ranges().to_vec(),
                bytes: v.literal_bytes().to_vec(),
            });
        }
        Ok(out)
    }
    pub fn slots(&self) -> impl Iterator<Item = FrozenSlot<'_>> {
        self.new.iter().map(|n| FrozenSlot {
            key: n.version.key(),
            ranges: n.version.literal_ranges(),
            bytes: n.version.literal_bytes(),
            copy_source: n.source,
        })
    }
    pub fn quarantine(self) {
        self.view.pool.quarantine();
    }
}
impl ReadStore for Arena {
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
            return self.read_overflow(at, len);
        }
        let (slot, generation, offset) = unpack(at)?;
        if let Some(p) = self.private.get(&slot)? {
            if p.generation == generation
                && let Ok(bytes) = p.read(offset, len)
            {
                return Ok(bytes);
            }
            if len != 0 || offset != 0 {
                return Err(Error::Corrupt("resident private membership/generation"));
            }
        }
        if let Ok(bytes) = self.base.read(at, len) {
            return Ok(bytes);
        }
        if len == 0
            && offset == 0
            && slot > 0
            && let Some(p) = self.private.get(&(slot - 1))?
            && p.generation == generation
            && let Ok(bytes) = p.read(PAGE, 0)
        {
            return Ok(bytes);
        }
        Err(Error::Corrupt("resident reference absent from writer"))
    }
}
impl WriteStore for Arena {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.parts(bytes, &[], alignment)
    }
    #[cfg(feature = "resident-direct-page-patch")]
    fn supports_patch_many(&self, object_len: usize, alignment: usize) -> bool {
        object_len == PAGE && alignment == PAGE
    }
    #[cfg(feature = "resident-direct-page-patch")]
    fn try_patch_many(
        &mut self,
        old: Reference,
        object_len: usize,
        patches: &[(usize, &[u8])],
        alignment: usize,
    ) -> Result<Option<Reference>> {
        if !self.supports_patch_many(object_len, alignment) {
            return Ok(None);
        }
        self.patch_many_page(old, patches).map(Some)
    }
    fn rewrite(&mut self, old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.pool.ensure_live()?;
        if !alignment.is_power_of_two()
            || alignment > PAGE
            || !old.offset.is_multiple_of(alignment as u64)
        {
            return Err(Error::InvalidInput("resident rewrite alignment"));
        }
        if !self.exact(old, bytes.len())? {
            return Err(Error::InvalidInput(
                "resident rewrite requires exact allocation",
            ));
        }
        if self.read(old, bytes.len())? == bytes {
            return Ok(old);
        }
        let (slot, generation, offset) = unpack(old)?;
        if let Some(p) = self.private.get(&slot)? {
            if p.generation != generation {
                return Err(Error::Corrupt("resident stale rewrite generation"));
            }
            p.read(offset, bytes.len())?;
            self.remember_private(slot)?;
            self.tick()?;
            self.private.get_mut(&slot)?.unwrap().write(offset, bytes)?;
            return Ok(old);
        }
        self.allocate_copy(old, bytes, alignment)
    }
    #[cfg(not(feature = "resident-fused-patch"))]
    fn patch(
        &mut self,
        old: Reference,
        object_len: usize,
        offset: usize,
        replacement: &[u8],
        alignment: usize,
    ) -> Result<Reference> {
        self.pool.ensure_live()?;
        if !alignment.is_power_of_two()
            || alignment > PAGE
            || !old.offset.is_multiple_of(alignment as u64)
        {
            return Err(Error::InvalidInput("resident patch alignment"));
        }
        let end = offset
            .checked_add(replacement.len())
            .filter(|n| *n <= object_len)
            .ok_or(Error::InvalidInput("resident patch bounds"))?;
        if !self.exact(old, object_len)? {
            return Err(Error::InvalidInput(
                "resident patch requires exact allocation",
            ));
        }
        let bytes = self.read(old, object_len)?;
        if &bytes[offset..end] == replacement {
            return Ok(old);
        }
        let (slot, generation, start) = unpack(old)?;
        if self
            .private
            .get(&slot)?
            .is_some_and(|p| p.generation == generation)
        {
            self.remember_private(slot)?;
            self.tick()?;
            self.private
                .get_mut(&slot)?
                .unwrap()
                .write(start + offset, replacement)?;
            Ok(old)
        } else {
            #[cfg(feature = "resident-direct-page-patch")]
            if object_len == PAGE && alignment == PAGE {
                return self.direct_page_patch(old, offset, replacement);
            }
            let mut bytes = bytes.to_vec();
            bytes[offset..end].copy_from_slice(replacement);
            self.allocate_copy(old, &bytes, alignment)
        }
    }
    #[cfg(feature = "resident-fused-patch")]
    fn patch(
        &mut self,
        old: Reference,
        object_len: usize,
        offset: usize,
        replacement: &[u8],
        alignment: usize,
    ) -> Result<Reference> {
        self.fused_patch(old, object_len, offset, replacement, alignment)
    }
    fn put_payload(&mut self, value: &[u8]) -> Result<Reference> {
        if value.len() > 4080 {
            return self.put_overflow(value);
        }
        let mut header = [0u8; 16];
        header[..2].copy_from_slice(&1u16.to_le_bytes());
        header[4..8].copy_from_slice(&16u32.to_le_bytes());
        header[8..].copy_from_slice(&(value.len() as u64).to_le_bytes());
        self.parts(&header, value, 16)
    }
}

impl View {
    /// Caller supplies an authenticated, state-checked new-family record. Safe
    /// ownership/range checks below remain independent of that authority claim.
    pub fn replay_literals(&self, record: &super::wal_codec::Record) -> Result<Prepared> {
        let mut versions = Vec::new();
        versions
            .try_reserve_exact(record.allocations.len())
            .map_err(|_| Error::LimitExceeded)?;
        for a in &record.allocations {
            if a.bytes.len() != PAGE || a.ranges.is_empty() || a.ranges.len() > 256 {
                return Err(Error::Corrupt("resident replay literal extent"));
            }
            let mut end = 0;
            for &(start, len) in &a.ranges {
                let next = start as usize + len as usize;
                if len == 0 || (start as usize) < end || next > PAGE {
                    return Err(Error::Corrupt("resident replay range bounds"));
                }
                end = next;
            }
            let mut p = self.pool.replay_permit(a.key)?;
            p.ranges
                .try_reserve_exact(a.ranges.len())
                .map_err(|_| Error::LimitExceeded)?;
            p.write(0, &a.bytes)?;
            p.ranges.extend_from_slice(&a.ranges);
            p.cursor = end;
            versions.push(NewVersion {
                version: p.seal(),
                source: None,
            });
        }
        let view = self.changed(
            versions.iter().map(|p| Arc::clone(&p.version)),
            &record.retired,
        )?;
        Ok(Prepared {
            source_base: self.clone(),
            view,
            new: versions.into(),
        })
    }
}

impl View {
    /// Low-level safe materialization only. Caller must first authenticate the RL2
    /// frame and prestate/source/retirement authority; this method grants none.
    /// All old source bytes stay borrowed until every new map has been sealed.
    pub fn replay_recipes(
        &self,
        boot: super::wal_codec::Boot,
        allocations: &[super::versioned_redo::Allocation],
        retired: &[super::VersionKey],
    ) -> Result<Prepared> {
        use super::versioned_redo::Body;
        let geometry = self.pool.geometry();
        if geometry != (boot.kv_pages as usize, boot.tiny_pages as usize)
            || allocations.len() > geometry.0 + geometry.1
            || retired.len() > geometry.0 + geometry.1
        {
            return Err(Error::Corrupt("RL2 materialization geometry/count"));
        }
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(allocations.len())
            .map_err(|_| Error::LimitExceeded)?;
        let mut last = None;
        for a in allocations {
            if a.destination.slot as usize >= geometry.0 + geometry.1
                || a.destination.generation == 0
                || a.destination.generation > super::wal_codec::MAX_GENERATION
                || last.is_some_and(|s| s >= a.destination.slot)
            {
                return Err(Error::Corrupt("RL2 materialization destination"));
            }
            super::wal_codec::validate_ranges(boot, a.destination.slot, &a.ranges)?;
            last = Some(a.destination.slot);
            let source = match &a.body {
                Body::Literal(b) => {
                    if b.len() != PAGE {
                        return Err(Error::Corrupt("RL2 literal width"));
                    }
                    None
                }
                Body::Copy { source, patches } => {
                    if source.slot >= boot.kv_pages
                        || a.destination.slot >= boot.kv_pages
                        || source.slot == a.destination.slot
                        || source.generation == 0
                        || source.generation > super::wal_codec::MAX_GENERATION
                        || patches.len() > PAGE / 2
                    {
                        return Err(Error::Corrupt("RL2 source geometry"));
                    }
                    let (bytes, key) =
                        self.read_with_key(tagged(source.slot, source.generation, 0), PAGE)?;
                    if key != *source {
                        return Err(Error::Corrupt("RL2 source physical membership"));
                    }
                    let mut end = 0;
                    let mut encoded = 16;
                    for (i, p) in patches.iter().enumerate() {
                        let start = p.offset as usize;
                        let next = start
                            .checked_add(p.bytes.len())
                            .filter(|n| *n <= PAGE)
                            .ok_or(Error::Corrupt("RL2 patch extent"))?;
                        if p.bytes.is_empty()
                            || (i > 0 && start <= end)
                            || bytes[start..next].iter().zip(&p.bytes).any(|(a, b)| a == b)
                        {
                            return Err(Error::Corrupt("RL2 nonminimal/overlapping patch"));
                        }
                        end = next;
                        encoded += 8 + p.bytes.len().next_multiple_of(8);
                    }
                    if encoded >= PAGE {
                        return Err(Error::Corrupt("RL2 noncompact copy"));
                    }
                    Some(bytes)
                }
            };
            sources.push(source);
        }
        let mut last = None;
        for key in retired {
            if key.slot as usize >= geometry.0 + geometry.1
                || last.is_some_and(|s| s >= key.slot)
                || allocations.iter().any(|a| {
                    a.destination.slot == key.slot
                        || matches!(&a.body,Body::Copy{source,..}if source.slot==key.slot)
                })
            {
                return Err(Error::Corrupt("RL2 retirement/source overlap"));
            }
            last = Some(key.slot);
        }
        let mut new = Vec::new();
        new.try_reserve_exact(allocations.len())
            .map_err(|_| Error::LimitExceeded)?;
        for (a, source) in allocations.iter().zip(sources) {
            let mut permit = self.pool.replay_permit(a.destination)?;
            permit
                .ranges
                .try_reserve_exact(a.ranges.len())
                .map_err(|_| Error::LimitExceeded)?;
            match (&a.body, source) {
                (Body::Literal(b), None) => permit.write(0, b)?,
                (Body::Copy { patches, .. }, Some(bytes)) => {
                    permit.write(0, bytes)?;
                    for p in patches {
                        permit.write(p.offset as usize, &p.bytes)?;
                    }
                }
                _ => unreachable!(),
            }
            permit.ranges.extend_from_slice(&a.ranges);
            let &(start, len) = a.ranges.last().unwrap();
            permit.cursor = start as usize + len as usize;
            new.push(NewVersion {
                version: permit.seal(),
                source: None,
            });
        }
        let view = self.changed(new.iter().map(|v| Arc::clone(&v.version)), retired)?;
        Ok(Prepared {
            source_base: self.clone(),
            view,
            new: new.into(),
        })
    }
}

#[cfg(all(test, feature = "resident-metadata-reuse"))]
#[path = "metadata_reuse_tests.rs"]
mod metadata_reuse_tests;
