use crate::{Error, ReadStore, Reference, Result, mapping::Mapping};
use std::{collections::BTreeSet, path::Path, sync::Arc};

mod weak;
pub use weak::WeakPlainSegmentTable;

#[derive(Clone, Debug)]
pub struct SegmentEntry {
    pub id: u64,
    pub length: u64,
    pub hash: [u8; 32],
    /// Zero selects the legacy hash-named segment; nonzero selects a cohort prefix.
    pub cohort: u128,
}
type Chunk = [Option<Arc<Mapping>>; 256];

/// Minted only by a successful insertion into an empty logical slot. CoW
/// preserves every previous Mapping owner. Selection/reload/reuse revoke it.
#[derive(Clone, Copy)]
struct AppendLineage {
    parent: u64,
    added: std::num::NonZeroU64,
}

#[cfg(test)]
thread_local! { static MAPPING_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
pub(crate) fn take_mapping_visits() -> usize {
    MAPPING_VISITS.with(|counter| counter.replace(0))
}

// An unaligned window of 1,048,576 IDs can span 4,097 chunks.
pub(crate) const MAX_DIRECTORY_CHUNKS: u64 = 4097;

/// Two indexed loads resolve a segment; no hash lookup/Arc clone per tree node.
#[derive(Clone, Default)]
pub struct SegmentTable {
    cached: Option<Arc<crate::chunk_table::ChunkedSegmentTable>>,
    chunk_budget: Option<(u64, u64, usize)>,
    page_size: ulmdb_core::PageSize,
    value_limit: ulmdb_core::ValueLimit,
    // Chunk numbers are relative to the oldest live chunk, independent of
    // process lifetime/absolute ID. Empty gaps allocate no chunk or Arc.
    first_chunk: u64,
    chunks: Vec<Option<Arc<Chunk>>>,
    live_segments: usize,
    residency_identity: u64,
    append_lineage: Option<AppendLineage>,
    read_ahead: crate::ReadAhead,
    transform: Option<Arc<crate::transform::TransformContext>>,
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fixed: Option<(Arc<crate::fixed_map::FixedMap>, bool)>,
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fixed_declaration: Option<Arc<crate::fixed_map::FixedMapDeclaration>>,
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fixed_external: Option<crate::external::ExternalSynchronization>,
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fixed_policy: crate::Durability,
}

impl SegmentTable {
    /// Reauthenticate ordinary mapped bytes without rebuilding their logical
    /// graph. This grants no namespace/pin authority: callers must independently
    /// bind every mapping to the freshly selected immutable source namespace.
    /// Chunked and transformed views keep their own admission protocol.
    pub fn revalidate_mapped_content(&self) -> Result<u64> {
        if self.cached.is_some() || self.transform.is_some() {
            return Err(Error::InvalidInput("mapped revalidation requires ordinary files"));
        }
        let mut bytes = 0u64;
        for chunk in self.chunks.iter().flatten() {
            for mapping in chunk.iter().flatten() {
                if mapping.range.is_some() {
                    return Err(Error::InvalidInput("range mapping revalidation unsupported"));
                }
                let content = mapping.data_bytes()?;
                if crate::work::digest(content)? != mapping.entry.hash {
                    return Err(Error::Corrupt("reused segment hash validation failed"));
                }
                bytes = bytes.checked_add(mapping.entry.length).ok_or(Error::LimitExceeded)?;
            }
        }
        Ok(bytes)
    }
    pub fn with_chunk_budget(mut self, reads: u64, nodes: u64, metadata: usize) -> Self {
        self.chunk_budget = Some((reads, nodes, metadata));
        self
    }
    pub fn chunk_pin_limits(&self) -> Option<crate::chunk_table::PinLimits> {
        self.cached.as_ref().map(|c| c.limits())
    }
    pub fn chunk_budget(&self) -> Option<(u64, u64, usize)> {
        self.chunk_budget
    }
    pub fn is_chunked(&self) -> bool {
        self.cached.is_some()
    }
    pub fn chunk_cache(&self) -> Option<&Arc<crate::chunk_cache::ChunkCache>> {
        self.cached.as_ref().map(|c| c.cache())
    }
    pub fn fresh_read_scope(&self) -> Result<Self> {
        let mut table = self.clone();
        if let Some(c) = &self.cached {
            table.cached = Some(Arc::new(c.scope()));
            table.residency_identity = Self::fresh_residency_identity()?;
            table.append_lineage = None;
        }
        Ok(table)
    }
    pub fn load_chunked(
        directory: &Path,
        entries: &crate::Inventory,
        cache: Arc<crate::chunk_cache::ChunkCache>,
        limits: crate::chunk_table::PinLimits,
        page_size: ulmdb_core::PageSize,
        value_limit: ulmdb_core::ValueLimit,
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<crate::transform::TransformContext>>,
    ) -> Result<Self> {
        limits.validate()?;
        read_ahead.validate()?;
        if entries.len() > limits.sources {
            return Err(Error::LimitExceeded);
        }
        let cached = crate::chunk_table::ChunkedSegmentTable::load_with_read_ahead(
            directory,
            entries,
            cache,
            limits,
            page_size,
            value_limit,
            transform.clone(),
            read_ahead,
        )?;
        let mut table = Self::empty_with_transform(read_ahead, transform)?;
        table.page_size = page_size;
        table.value_limit = value_limit;
        table.live_segments = entries.len();
        table.residency_identity = Self::fresh_residency_identity()?;
        table.cached = Some(Arc::new(cached));
        Ok(table)
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub fn with_fixed_map(
        &self,
        directory: &Path,
        fixed: Arc<crate::fixed_map::FixedMap>,
    ) -> Result<Self> {
        if self.cached.is_some() {
            return Err(Error::InvalidInput("fixed chunk mapping incompatible"));
        }
        if self.transform.as_ref().is_some_and(|c| c.encrypted()) {
            return Err(Error::InvalidInput("encrypted fixed mapping unsupported"));
        }
        fixed.check()?;
        if directory.canonicalize()? != fixed.directory() {
            return Err(Error::Corrupt("fixed table namespace"));
        }
        let mut table = Self::empty_with_read_ahead(self.read_ahead)?
            .with_page_size(self.page_size)?
            .with_value_limit(self.value_limit)?;
        table.transform = self.transform.clone();
        table.fixed_external = self.fixed_external.clone();
        table.fixed_policy = self.fixed_policy;
        table.fixed = Some((fixed, false));
        for entry in self.entries() {
            table.add(directory, entry)?;
        }
        Ok(table)
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub fn fixed_map(&self) -> Option<Arc<crate::fixed_map::FixedMap>> {
        self.fixed.as_ref().map(|(fixed, _)| fixed.clone())
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub fn with_fixed_declaration(
        mut self,
        declaration: Arc<crate::fixed_map::FixedMapDeclaration>,
    ) -> Self {
        self.fixed_declaration = Some(declaration);
        self
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub fn fixed_declaration(&self) -> Option<Arc<crate::fixed_map::FixedMapDeclaration>> {
        self.fixed_declaration.clone()
    }
    /// Carry numeric placement tracking only when rebuilding the same namespace.
    /// Independent copy targets must not inherit the source's authority.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub fn with_fixed_declaration_from(mut self, source: &Self, policy: crate::Durability) -> Self {
        self.fixed_declaration = source.fixed_declaration.clone();
        self.fixed_external = source.fixed_external.clone();
        self.fixed_policy = policy;
        self
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub fn with_fixed_external_authority(
        mut self,
        external: Option<crate::external::ExternalSynchronization>,
    ) -> Self {
        self.fixed_external = external;
        self
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub(crate) fn fixed_external_authority(
        &self,
    ) -> Option<crate::external::ExternalSynchronization> {
        self.fixed_external.clone()
    }
    pub(crate) fn fixed_registration_policy(&mut self, policy: crate::Durability) {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            self.fixed_policy = policy;
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let _ = policy;
    }
    pub(crate) fn allow_fixed_registration(&mut self, allow: bool) {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if let Some((_, register)) = &mut self.fixed {
            *register = allow;
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let _ = allow;
    }
    pub fn with_page_size(mut self, size: ulmdb_core::PageSize) -> Result<Self> {
        if self.page_size != size {
            self.page_size = size;
            self.residency_identity = Self::fresh_residency_identity()?;
            self.append_lineage = None;
        }
        Ok(self)
    }
    pub fn with_value_limit(mut self, limit: ulmdb_core::ValueLimit) -> Result<Self> {
        if !limit.is_supported() {
            return Err(Error::InvalidInput(
                "maximum file profile requires64-bit host",
            ));
        }
        if self.value_limit != limit {
            self.value_limit = limit;
            self.residency_identity = Self::fresh_residency_identity()?;
            self.append_lineage = None;
        }
        Ok(self)
    }
    pub fn value_limit(&self) -> ulmdb_core::ValueLimit {
        self.value_limit
    }
    pub fn page_geometry(&self) -> ulmdb_core::PageSize {
        self.page_size
    }
    pub(crate) fn matches_inventory(&self, inventory: &crate::Inventory) -> bool {
        if self.cached.is_some() {
            return self.live_segments == inventory.len()
                && self
                    .entries()
                    .iter()
                    .zip(inventory.iter())
                    .all(|(a, b)| crate::metadata::same_entry(a, b));
        }
        self.live_segments == inventory.len()
            && self
                .mappings()
                .zip(inventory.iter())
                .all(|(mapping, expected)| crate::metadata::same_entry(&mapping.entry, expected))
    }
    pub(crate) fn residency_identity(&self) -> u64 {
        self.residency_identity
    }
    pub(crate) fn appended_mapping<'a>(&'a self, base: &Self) -> Option<&'a Mapping> {
        let lineage = self.append_lineage?;
        if lineage.parent != base.residency_identity
            || self.residency_identity == base.residency_identity
            || base.live_segments.checked_add(1) != Some(self.live_segments)
            || base.residency_identity == 0
        {
            return None;
        }
        self.mapping(lineage.added.get())
    }
    pub(crate) fn mappings(&self) -> impl Iterator<Item = &Mapping> {
        let mappings = self
            .chunks
            .iter()
            .filter_map(Option::as_ref)
            .flat_map(|c| c.iter())
            .filter_map(|m| m.as_deref());
        #[cfg(test)]
        return mappings.inspect(|_| MAPPING_VISITS.with(|counter| counter.set(counter.get() + 1)));
        #[cfg(not(test))]
        mappings
    }
    fn fresh_residency_identity() -> Result<u64> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static IDS: AtomicU64 = AtomicU64::new(1);
        IDS.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::LimitExceeded)
    }

    /// Empty table retaining the owner's immutable mapping advice request.
    pub fn empty_with_read_ahead(read_ahead: crate::ReadAhead) -> Result<Self> {
        read_ahead.validate()?;
        Ok(Self {
            read_ahead,
            ..Self::default()
        })
    }
    pub fn empty_with_transform(
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<crate::transform::TransformContext>>,
    ) -> Result<Self> {
        let mut table = Self::empty_with_read_ahead(read_ahead)?;
        table.transform = transform;
        Ok(table)
    }
    /// Empty table with the same immutable cache/advice/transform authority.
    /// This does not retain source data or transfer any fixed address namespace.
    pub fn empty_like(&self) -> Result<Self> {
        let mut table = Self::empty_with_transform(self.read_ahead, self.transform.clone())?
            .with_page_size(self.page_size)?
            .with_value_limit(self.value_limit)?;
        if let Some(cached) = &self.cached {
            table.cached = Some(Arc::new(cached.select_entries(&[])?));
            table.chunk_budget = self.chunk_budget;
        }
        Ok(table)
    }
    pub fn transform(&self) -> Option<&Arc<crate::transform::TransformContext>> {
        self.transform.as_ref()
    }
    pub fn read_ahead(&self) -> crate::ReadAhead {
        self.read_ahead
    }
    pub fn load(directory: &Path, entries: &[SegmentEntry]) -> Result<Self> {
        Self::load_with_read_ahead(directory, entries, crate::ReadAhead::Normal)
    }
    pub fn load_with_read_ahead(
        directory: &Path,
        entries: &[SegmentEntry],
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        Self::load_with_transform(directory, entries, read_ahead, None)
    }
    pub fn load_with_transform(
        directory: &Path,
        entries: &[SegmentEntry],
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<crate::transform::TransformContext>>,
    ) -> Result<Self> {
        Self::load_with_profile(
            directory,
            entries,
            read_ahead,
            transform,
            Default::default(),
        )
    }
    pub fn load_with_profile(
        directory: &Path,
        entries: &[SegmentEntry],
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<crate::transform::TransformContext>>,
        value_limit: ulmdb_core::ValueLimit,
    ) -> Result<Self> {
        read_ahead.validate()?;
        // Validate the complete inventory before opening, hashing or mapping a
        // single file. Descriptor decoding applies the same sorted/window bound.
        if entries.len() >= 1_048_576 {
            return Err(Error::LimitExceeded);
        }
        let observer = ulmdb_core::work::current_observer();
        let mut prior = 0;
        let first = entries.first().map_or(0, |entry| entry.id >> 8);
        for entry in entries {
            if let Some(observer) = &observer { observer.checkpoint(1, 64)?; }
            if entry.id <= prior || entry.id == u64::MAX {
                return Err(Error::Corrupt("invalid segment inventory IDs"));
            }
            if (entry.id >> 8) - first >= MAX_DIRECTORY_CHUNKS {
                return Err(Error::LimitExceeded);
            }
            prior = entry.id;
        }
        let mut table =
            Self::empty_with_transform(read_ahead, transform)?.with_value_limit(value_limit)?;
        for entry in entries {
            if let Some(observer) = &observer { observer.checkpoint(1, 0)?; }
            table.add(directory, entry.clone())?;
        }
        table.append_lineage = None;
        Ok(table)
    }
    pub fn add(&mut self, directory: &Path, entry: SegmentEntry) -> Result<()> {
        if !(4096..=self.value_limit.segment_bytes() as u64).contains(&entry.length) {
            return Err(Error::Corrupt("segment exceeds table value profile"));
        }
        self.check_insert_id(entry.id)?;
        if let Some(cached) = &self.cached {
            let loaded = crate::chunk_table::ChunkedSegmentTable::load_with_read_ahead(
                directory,
                std::slice::from_ref(&entry),
                cached.cache().clone(),
                cached.limits(),
                self.page_size,
                self.value_limit,
                self.transform.clone(),
                self.read_ahead,
            )?;
            let mut entries = cached.entries().to_vec();
            entries.push(entry.clone());
            entries.sort_by_key(|entry| entry.id);
            let mut owners = std::collections::BTreeMap::new();
            for old in cached.entries() {
                owners.insert(old.id, cached.as_ref());
            }
            owners.insert(entry.id, &loaded);
            let joined = crate::chunk_table::ChunkedSegmentTable::join_sources(&entries, &owners)?;
            self.cached = Some(Arc::new(joined));
            self.live_segments += 1;
            self.residency_identity = Self::fresh_residency_identity()?;
            self.append_lineage = None;
            return Ok(());
        }
        self.insert_mapping(Mapping::open_with_transform(
            directory,
            entry,
            None,
            self.read_ahead,
            self.transform.as_ref(),
        )?)
    }
    pub(crate) fn add_written(
        &mut self,
        directory: &Path,
        entry: SegmentEntry,
        written: &[u8],
    ) -> Result<()> {
        self.check_insert_id(entry.id)?;
        self.insert_mapping(Mapping::open_with_transform(
            directory,
            entry,
            Some(written),
            self.read_ahead,
            self.transform.as_ref(),
        )?)
    }
    /// Resource preflight before an arena can prepare any files. IDs are never
    /// reused, and the final representable ID is reserved for next-unused.
    pub(crate) fn check_insert_id(&self, id: u64) -> Result<()> {
        if id == 0 || id == u64::MAX {
            return Err(Error::LimitExceeded);
        }
        if let Some(cached) = &self.cached {
            if self.live_segments >= cached.limits().sources {
                return Err(Error::LimitExceeded);
            }
            if cached.contains(id) {
                return Err(Error::Corrupt("duplicate cached segment ID"));
            }
            let first = cached
                .entries()
                .first()
                .map_or(id, |e| e.id)
                .min(if self.chunks.is_empty() {
                    id
                } else {
                    self.first_chunk * 256
                })
                .min(id)
                >> 8;
            let last = cached.entries().last().map_or(id, |e| e.id).max(id) >> 8;
            if last - first >= MAX_DIRECTORY_CHUNKS {
                return Err(Error::LimitExceeded);
            }
        }
        if self.live_segments >= 1_048_575 {
            return Err(Error::LimitExceeded);
        }
        let chunk = id >> 8;
        if !self.chunks.is_empty() {
            let last = self.first_chunk + self.chunks.len() as u64 - 1;
            let span = last.max(chunk) - self.first_chunk.min(chunk) + 1;
            if span > MAX_DIRECTORY_CHUNKS {
                return Err(Error::LimitExceeded);
            }
            if self.mapping(id).is_some() {
                return Err(Error::Corrupt("duplicate segment ID"));
            }
        }
        Ok(())
    }
    #[inline]
    fn mapping(&self, id: u64) -> Option<&Mapping> {
        // Below-anchor IDs wrap to >= 2^64-(2^56-1): conversion (32-bit) or
        // get's bounds check (64-bit) rejects them. This avoids a second branch
        // before that existing bounds check. Directory length is <= 4,097.
        let relative = (id >> 8).wrapping_sub(self.first_chunk);
        let index = usize::try_from(relative).ok()?;
        self.chunks.get(index)?.as_ref()?[(id & 255) as usize].as_deref()
    }
    pub(crate) fn insert_mapping(&mut self, mut mapping: Mapping) -> Result<()> {
        if mapping.entry.length > self.value_limit.segment_bytes() as u64 {
            return Err(Error::Corrupt("mapping exceeds table value profile"));
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if let Some((fixed, register)) = &self.fixed {
            mapping.bind_fixed(
                fixed,
                *register,
                self.fixed_policy,
                self.fixed_external.clone(),
            )?;
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if self.fixed.is_none() {
            if let Some(declaration) = &self.fixed_declaration {
                declaration.register_entry(
                    &mapping.entry,
                    self.fixed_policy,
                    self.fixed_external.clone(),
                )?;
            }
        }
        self.insert_owner(Arc::new(mapping))
    }
    fn insert_owner(&mut self, mapping: Arc<Mapping>) -> Result<()> {
        if mapping.read_ahead != self.read_ahead {
            return Err(Error::InvalidInput(
                "shared mapping advice differs from table",
            ));
        }
        let id = mapping.entry.id;
        self.check_insert_id(id)?;
        let added = std::num::NonZeroU64::new(id).ok_or(Error::LimitExceeded)?;
        let revision = Self::fresh_residency_identity()?;
        let parent = self.residency_identity;
        // Revoke old optimization permission before any allocating mutation.
        self.append_lineage = None;
        let chunk = id >> 8;
        if self.chunks.is_empty() {
            self.first_chunk = chunk;
        } else if chunk < self.first_chunk {
            let prefix = (self.first_chunk - chunk) as usize;
            self.chunks.splice(0..0, std::iter::repeat_n(None, prefix));
            self.first_chunk = chunk;
        }
        let index = (chunk - self.first_chunk) as usize;
        if self.chunks.len() <= index {
            self.chunks.resize_with(index + 1, || None);
        }
        let chunk =
            self.chunks[index].get_or_insert_with(|| Arc::new(std::array::from_fn(|_| None)));
        Arc::make_mut(chunk)[(id & 255) as usize] = Some(mapping);
        self.live_segments += 1;
        self.residency_identity = revision;
        self.append_lineage = Some(AppendLineage { parent, added });
        Ok(())
    }
    /// Reuse an already-verified immutable mapping without rehashing/remapping.
    /// A present ID with changed identity is corruption, not a replacement.
    pub fn reuse_from(&mut self, source: &Self, expected: &SegmentEntry) -> Result<bool> {
        if expected.length > self.value_limit.segment_bytes() as u64 {
            return Err(Error::Corrupt("reused segment exceeds value profile"));
        }
        if source.mapping(expected.id).is_none()
            && source
                .cached
                .as_ref()
                .is_some_and(|c| c.contains(expected.id))
        {
            if self.transform.as_ref().map(Arc::as_ptr)
                != source.transform.as_ref().map(Arc::as_ptr)
                || self.read_ahead != source.read_ahead
            {
                return Ok(false);
            }
            if let (Some(target), Some(source)) = (&self.cached, &source.cached) {
                if !Arc::ptr_eq(target.cache(), source.cache()) {
                    return Ok(false);
                }
            }
            if self.live_segments >= source.cached.as_ref().unwrap().limits().sources {
                return Err(Error::LimitExceeded);
            }
            self.check_insert_id(expected.id)?;
            let cache = source.cached.as_ref().unwrap();
            let mut entries = self
                .cached
                .as_ref()
                .map(|c| c.entries().to_vec())
                .unwrap_or_default();
            entries.push(expected.clone());
            entries.sort_by_key(|e| e.id);
            let mut origins = std::collections::BTreeMap::new();
            for entry in &entries {
                let owner = if self.cached.as_ref().is_some_and(|c| c.contains(entry.id)) {
                    self.cached.as_ref().unwrap()
                } else {
                    cache
                };
                origins.insert(entry.id, owner.as_ref());
            }
            // All cached owners share the same immutable authenticated source family.
            let joined = crate::chunk_table::ChunkedSegmentTable::join_sources(&entries, &origins)?;
            if self.chunk_budget.is_none() {
                self.chunk_budget = source.chunk_budget;
            }
            self.cached = Some(Arc::new(joined));
            self.live_segments += 1;
            self.residency_identity = Self::fresh_residency_identity()?;
            self.append_lineage = None;
            return Ok(true);
        }
        let relative = (expected.id >> 8).wrapping_sub(source.first_chunk);
        let Some(owner) = usize::try_from(relative)
            .ok()
            .and_then(|index| source.chunks.get(index))
            .and_then(Option::as_ref)
            .and_then(|chunk| chunk[(expected.id & 255) as usize].as_ref())
        else {
            return Ok(false);
        };
        if owner.range.is_some() {
            return Err(Error::InvalidInput(
                "range reuse requires explicit physical identity",
            ));
        }
        let actual = &owner.entry;
        if actual.id != expected.id
            || actual.length != expected.length
            || actual.hash != expected.hash
            || actual.cohort != expected.cohort
        {
            return Err(Error::Corrupt("immutable segment ID was rebound"));
        }
        // Advice belongs to this VMA. Never re-advise a mapping shared with a
        // differently configured owner; the caller must load an independent VMA.
        if !match (&self.transform, &source.transform) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        } {
            return Ok(false);
        }
        if owner.read_ahead != self.read_ahead {
            return Ok(false);
        }
        self.insert_owner(Arc::clone(owner))?;
        self.append_lineage = None;
        Ok(true)
    }
    /// Select an inventory without remapping or copying segment bytes. Existing
    /// tables/readers keep their own immutable mappings. Every requested ID must
    /// already be present; callers must trace complete typed object graphs first.
    pub fn select(&self, ids: &BTreeSet<u64>) -> Result<Self> {
        for id in ids {
            if *id == 0
                || (self.mapping(*id).is_none()
                    && !self.cached.as_ref().is_some_and(|c| c.contains(*id)))
            {
                return Err(Error::Corrupt("selected segment is missing"));
            }
        }
        if ids.len() == self.live_segments {
            let mut selected = self.clone();
            selected.append_lineage = None;
            return Ok(selected);
        }
        let revision = Self::fresh_residency_identity()?;
        let mut selected = self.clone();
        selected.append_lineage = None;
        selected.residency_identity = revision;
        selected.live_segments = ids.len();
        if let Some(cached) = &self.cached {
            let entries: Vec<_> = cached
                .entries()
                .iter()
                .filter(|e| ids.contains(&e.id))
                .cloned()
                .collect();
            selected.cached = Some(Arc::new(cached.select_entries(&entries)?));
        }

        for (chunk_index, slot) in selected.chunks.iter_mut().enumerate() {
            let Some(chunk) = slot.as_ref() else { continue };
            let mut kept = 0usize;
            let mut present = 0usize;
            for (offset, mapping) in chunk.iter().enumerate() {
                if mapping.is_some() {
                    present += 1;
                    if ids
                        .contains(&((self.first_chunk + chunk_index as u64) * 256 + offset as u64))
                    {
                        kept += 1;
                    }
                }
            }
            if kept == 0 {
                *slot = None;
            } else if kept != present {
                let chunk = Arc::make_mut(slot.as_mut().expect("present selected chunk"));
                for (offset, mapping) in chunk.iter_mut().enumerate() {
                    if !ids
                        .contains(&((self.first_chunk + chunk_index as u64) * 256 + offset as u64))
                    {
                        *mapping = None;
                    }
                }
            }
        }
        while selected.chunks.last().is_some_and(Option::is_none) {
            selected.chunks.pop();
        }
        let leading = selected
            .chunks
            .iter()
            .position(Option::is_some)
            .unwrap_or(selected.chunks.len());
        if leading != 0 {
            selected.chunks.drain(..leading);
            selected.first_chunk += leading as u64;
        }
        if selected.chunks.is_empty() {
            selected.first_chunk = 0;
        }
        Ok(selected)
    }
    /// Reserve one inventory slot before cohort insertion, avoiding a full
    /// N -> 2N growth/copy when the base inventory is already exactly sized.
    pub(crate) fn entries_for_addition(&self) -> Result<Vec<SegmentEntry>> {
        let capacity = self
            .live_segments
            .checked_add(1)
            .filter(|count| *count <= 1_048_575)
            .ok_or(Error::LimitExceeded)?;
        Ok(self.collect_entries(capacity))
    }
    pub fn entries(&self) -> Vec<SegmentEntry> {
        self.collect_entries(self.live_segments)
    }
    fn collect_entries(&self, capacity: usize) -> Vec<SegmentEntry> {
        let mut entries = Vec::with_capacity(capacity);
        if let Some(cached) = &self.cached {
            entries.extend_from_slice(cached.entries());
        }

        entries.extend(
            self.chunks
                .iter()
                .filter_map(Option::as_ref)
                .flat_map(|c| c.iter())
                .filter_map(|m| m.as_ref().map(|m| m.entry.clone())),
        );
        if self.cached.is_some() {
            entries.sort_by_key(|entry| entry.id);
        }
        entries
    }
}
impl ReadStore for SegmentTable {
    fn max_value_size(&self) -> usize {
        self.value_limit.get()
    }
    fn page_size(&self) -> usize {
        self.page_size.get()
    }
    fn validate_extent(&self, at: Reference, length: usize) -> Result<()> {
        if self.mapping(at.segment).is_some() {
            return self.read(at, length).map(|_| ());
        }
        match &self.cached {
            Some(cached) => cached.validate_extent(at, length),
            None => Err(Error::Corrupt("missing segment")),
        }
    }
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        if at.segment == 0 {
            return Err(Error::Corrupt("null object dereference"));
        }
        let mapping = match self.mapping(at.segment) {
            Some(mapping) => mapping,
            None => {
                return match &self.cached {
                    Some(cached) => cached.read(at, length),
                    None => Err(Error::Corrupt("missing segment")),
                };
            }
        };
        let offset = usize::try_from(at.offset).map_err(|_| Error::Corrupt("offset overflow"))?;
        let end = offset
            .checked_add(length)
            .ok_or(Error::Corrupt("extent overflow"))?;
        if offset < 4096 {
            return Err(Error::Corrupt("reference targets segment header"));
        }
        mapping
            .data_bytes()?
            .get(offset..end)
            .ok_or(Error::Corrupt("object exceeds mapped segment"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Arena, WriteStore};
    #[test]
    fn high_water_ids_allocate_only_live_chunks_and_preserve_cow_tables() {
        let directory =
            std::env::temp_dir().join(format!("ulmdb-sparse-{}", crate::new_identity().unwrap()));
        crate::create_layout(&directory).unwrap();
        let mut arena = Arena::new(SegmentTable::default(), 1_000_000).unwrap();
        let value = arena.put_payload(b"old").unwrap();
        let (old, _) = arena.seal(&directory).unwrap();
        assert_eq!(old.chunks.iter().filter(|chunk| chunk.is_some()).count(), 1);
        assert_eq!(old.chunks.len(), 1);
        assert_eq!(old.first_chunk, 1_000_000 / 256);
        assert_eq!(old.payload(value).unwrap(), b"old");
        let mut arena = Arena::new(old.clone(), 1_000_001).unwrap();
        let new_value = arena.put_payload(b"new").unwrap();
        let (new, _) = arena.seal(&directory).unwrap();
        assert_eq!(new.entries().len(), 2);
        assert_eq!(old.entries().len(), 1);
        assert!(old.payload(new_value).is_err());
        assert_eq!(new.payload(value).unwrap(), b"old");
        assert_eq!(new.payload(new_value).unwrap(), b"new");
        assert!(
            new.read(
                Reference {
                    segment: 1,
                    offset: 4096
                },
                1
            )
            .is_err()
        );
        drop(new);
        drop(old);
        std::fs::remove_dir_all(directory).unwrap();
    }
    struct Directory(std::path::PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("ulmdb-window-{}", crate::new_identity().unwrap()));
            crate::create_layout(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn append(dir: &Path, base: SegmentTable, id: u64, value: &[u8]) -> (SegmentTable, Reference) {
        let mut arena = Arena::new(base, id).unwrap();
        let at = arena.put_payload(value).unwrap();
        (arena.seal(dir).unwrap().0, at)
    }
    #[test]
    fn cohort_inventory_addition_reserves_one_extra_slot_and_checks_limit() {
        let dir = Directory::new();
        let (table, _) = append(&dir.0, SegmentTable::default(), 17, b"value");
        let mut entries = table.entries_for_addition().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries.capacity() >= 2);
        let pointer = entries.as_ptr();
        let mut next = entries[0].clone();
        next.id = 18;
        entries.push(next);
        assert_eq!(entries.as_ptr(), pointer);
        let impossible = SegmentTable {
            live_segments: 1_048_575,
            ..SegmentTable::default()
        };
        assert!(matches!(
            impossible.entries_for_addition(),
            Err(Error::LimitExceeded)
        ));
    }
    #[test]
    fn near_u64_limit_prepend_trim_empty_reanchor_and_old_views() {
        let dir = Directory::new();
        let (original, high) = append(&dir.0, SegmentTable::default(), u64::MAX - 1, b"high");
        assert_eq!(original.live_segments, 1);
        for segment in [0, 1, u64::MAX - 256, u64::MAX - 2, u64::MAX] {
            assert!(
                original
                    .read(
                        Reference {
                            segment,
                            offset: 4096
                        },
                        1
                    )
                    .is_err()
            );
        }
        let ptr = original.payload(high).unwrap().as_ptr();
        let (with_lower, lower) = append(&dir.0, original.clone(), u64::MAX - 1024, b"lower");
        assert_eq!(with_lower.chunks.len(), 5);
        assert_eq!(with_lower.live_segments, 2);
        assert_eq!(with_lower.payload(high).unwrap().as_ptr(), ptr);
        assert!(original.payload(lower).is_err());
        let selected = with_lower.select(&BTreeSet::from([high.segment])).unwrap();
        assert_eq!(selected.chunks.len(), 1);
        assert_eq!(selected.live_segments, 1);
        assert_eq!(selected.first_chunk, high.segment >> 8);
        assert_eq!(selected.payload(high).unwrap().as_ptr(), ptr);
        assert!(selected.payload(lower).is_err());
        assert_eq!(with_lower.payload(lower).unwrap(), b"lower");
        assert!(
            with_lower
                .select(&BTreeSet::from([high.segment - 256]))
                .is_err()
        );
        let empty = selected.select(&BTreeSet::new()).unwrap();
        assert_eq!((empty.first_chunk, empty.chunks.len()), (0, 0));
        assert_eq!(empty.live_segments, 0);
        let (reanchored, low) = append(&dir.0, empty, 1, b"low");
        assert_eq!(reanchored.payload(low).unwrap(), b"low");
        assert!(reanchored.payload(high).is_err());
        assert_eq!(original.payload(high).unwrap(), b"high");
        for id in [0, u64::MAX] {
            assert!(matches!(
                Arena::new(SegmentTable::default(), id),
                Err(Error::LimitExceeded)
            ));
        }
        assert!(matches!(
            Arena::new(original, high.segment),
            Err(Error::Corrupt(_))
        ));
    }
    #[test]
    fn unaligned_window_uses_4097_chunks_and_limit_precedes_io() {
        let dir = Directory::new();
        let first = (1u64 << 40) + 255;
        let last = first + 1_048_575;
        let (table, low) = append(&dir.0, SegmentTable::default(), first, b"old");
        let (mut table, high) = append(&dir.0, table, last, b"new");
        assert_eq!(table.chunks.len(), MAX_DIRECTORY_CHUNKS as usize);
        assert_eq!(table.chunks.iter().filter(|c| c.is_some()).count(), 2);
        assert_eq!(table.payload(low).unwrap(), b"old");
        assert_eq!(table.payload(high).unwrap(), b"new");
        let too_far = ((last >> 8) + 1) * 256;
        let oversized = [
            SegmentEntry {
                id: first,
                length: 4096,
                hash: [0; 32],
                cohort: 0,
            },
            SegmentEntry {
                id: too_far,
                length: 4096,
                hash: [1; 32],
                cohort: 0,
            },
        ];
        assert!(matches!(
            SegmentTable::load(Path::new("/nonexistent-ulmdb-window"), &oversized),
            Err(Error::LimitExceeded)
        ));
        let duplicate = [oversized[0].clone(), oversized[0].clone()];
        assert!(matches!(
            SegmentTable::load(Path::new("/nonexistent-ulmdb-window"), &duplicate),
            Err(Error::Corrupt(_))
        ));
        assert!(matches!(
            Arena::new(table.clone(), too_far),
            Err(Error::LimitExceeded)
        ));
        let before = table.chunks.len();
        assert!(matches!(
            table.add(
                Path::new("/nonexistent-ulmdb-window"),
                SegmentEntry {
                    id: too_far,
                    length: 4096,
                    hash: [0; 32],
                    cohort: 0,
                }
            ),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(table.chunks.len(), before);
        let rebased = table.select(&BTreeSet::from([last])).unwrap();
        assert_eq!(rebased.chunks.len(), 1);
        assert!(Arena::new(rebased.clone(), too_far).is_ok());
        assert_eq!(rebased.payload(high).unwrap(), b"new");
        assert!(rebased.payload(low).is_err());
    }
    #[test]
    fn verified_mapping_reuse_preserves_owner_without_file_access_and_rejects_rebinding() {
        let dir = Directory::new();
        let (source, value) = append(&dir.0, SegmentTable::default(), 41, b"stable");
        let entry = source.entries().pop().unwrap();
        let pointer = source.payload(value).unwrap().as_ptr();
        // Unlinking is permitted by the mapping ownership contract; a remap or
        // rehash would now fail. The owner still keeps its immutable bytes live.
        std::fs::remove_file(crate::segment_path(&dir.0, &entry)).unwrap();
        let mut reused = SegmentTable::default();
        assert!(reused.reuse_from(&source, &entry).unwrap());
        assert_eq!(reused.payload(value).unwrap().as_ptr(), pointer);
        assert_eq!(reused.payload(value).unwrap(), b"stable");
        let mut different = entry.clone();
        different.hash[0] ^= 1;
        assert!(matches!(
            SegmentTable::default().reuse_from(&source, &different),
            Err(Error::Corrupt(_))
        ));
        different = entry.clone();
        different.cohort = 7;
        assert!(matches!(
            SegmentTable::default().reuse_from(&source, &different),
            Err(Error::Corrupt(_))
        ));
        different = entry.clone();
        different.length += 1;
        assert!(matches!(
            SegmentTable::default().reuse_from(&source, &different),
            Err(Error::Corrupt(_))
        ));
        different = entry.clone();
        different.id += 1;
        assert!(
            !SegmentTable::default()
                .reuse_from(&source, &different)
                .unwrap()
        );
        assert!(matches!(
            reused.reuse_from(&source, &entry),
            Err(Error::Corrupt(_))
        ));
        drop(source);
        assert_eq!(reused.payload(value).unwrap(), b"stable");
    }
}
