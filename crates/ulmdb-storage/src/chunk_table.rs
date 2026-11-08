//! Transaction-owned bounded chunk pins. No whole segment mapping is created.
use crate::chunk_cache::{AuthenticatedFile, ChunkCache, ChunkPin, ObjectIdentity};
use crate::{Error, ReadStore, Reference, Result, SegmentEntry};
use std::mem::ManuallyDrop;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};
#[derive(Clone, Copy, Debug)]
pub struct PinLimits {
    pub sources: usize,
    pub pins: usize,
}
impl Default for PinLimits {
    fn default() -> Self {
        Self {
            sources: 4096,
            pins: 65536,
        }
    }
}
impl PinLimits {
    pub fn validate(self) -> Result<()> {
        if self.sources == 0 || self.sources > 1_048_575 || self.pins == 0 || self.pins > 1_048_576
        {
            return Err(Error::InvalidInput("chunk table limits"));
        }
        Ok(())
    }
}
struct Sources {
    creator: u32,
    files: BTreeMap<u64, Arc<AuthenticatedFile>>,
    entries: Vec<SegmentEntry>,
    cache: Arc<ChunkCache>,
    limits: PinLimits,
    page_size: ulmdb_core::PageSize,
    value_limit: ulmdb_core::ValueLimit,
}
/// A fresh scope shares authenticated source descriptors/cache, but owns its
/// pins independently. Every returned borrow stays valid until this scope drops.
/// Admission can drop an entire short scope after copying only node metadata.
pub struct ChunkedSegmentTable {
    sources: Arc<Sources>,
    pins: ManuallyDrop<Mutex<BTreeMap<usize, Box<ChunkPin>>>>,
}
impl ChunkedSegmentTable {
    pub fn load(
        directory: &Path,
        entries: &[SegmentEntry],
        cache: Arc<ChunkCache>,
        limits: PinLimits,
        page_size: ulmdb_core::PageSize,
        value_limit: ulmdb_core::ValueLimit,
        context: Option<Arc<crate::transform::TransformContext>>,
    ) -> Result<Self> {
        Self::load_with_read_ahead(
            directory,
            entries,
            cache,
            limits,
            page_size,
            value_limit,
            context,
            crate::ReadAhead::Normal,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn load_with_read_ahead(
        directory: &Path,
        entries: &[SegmentEntry],
        cache: Arc<ChunkCache>,
        limits: PinLimits,
        page_size: ulmdb_core::PageSize,
        value_limit: ulmdb_core::ValueLimit,
        context: Option<Arc<crate::transform::TransformContext>>,
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        read_ahead.validate()?;
        limits.validate()?;
        let _ = cache.stats()?;
        if entries.len() > limits.sources {
            return Err(Error::LimitExceeded);
        }
        let mut previous = 0;
        let first = entries.first().map_or(0, |e| e.id >> 8);
        for e in entries {
            if e.id <= previous || e.id == u64::MAX {
                return Err(Error::Corrupt("chunk inventory IDs"));
            }
            if (e.id >> 8) - first >= crate::table::MAX_DIRECTORY_CHUNKS {
                return Err(Error::LimitExceeded);
            }
            if !(4096..=value_limit.segment_bytes() as u64).contains(&e.length) {
                return Err(Error::Corrupt("chunk segment length"));
            }
            previous = e.id;
        }
        let mut files = BTreeMap::new();
        let namespace = if entries.is_empty() {
            None
        } else {
            Some(cache.source_directory(directory)?)
        };
        for e in entries {
            let path = crate::segment_path(directory, e);
            let relative = path
                .strip_prefix(directory)
                .map_err(|_| Error::Corrupt("chunk source path escapes directory"))?;
            let encoded = context
                .as_ref()
                .map(|c| c.encoded_length(e.length))
                .transpose()?
                .unwrap_or(e.length);
            let physical_bounds = if e.cohort == 0 {
                (encoded, encoded)
            } else {
                (
                    encoded.checked_add(224).ok_or(Error::LimitExceeded)?,
                    crate::MAX_COHORT_BYTES,
                )
            };
            let source = AuthenticatedFile::open_reopenable(
                namespace.as_ref().unwrap().clone(),
                relative,
                ObjectIdentity {
                    kind: crate::transform::ObjectKind::Segment,
                    id: e.id as u128,
                    length: e.length,
                    hash: e.hash,
                    file_offset: 0,
                },
                physical_bounds,
                context.clone(),
                read_ahead,
            )?;
            {
                let header = cache.pin(&source, 0, 4096)?;
                let features = match &header[10..12] {
                    [1, 0] => 1u64,
                    [2, 0] => 3u64,
                    [3, 0] if e.length > 268_435_456 && e.length <= 1_073_741_824 => 7u64,
                    [4, 0] if e.length > 1_073_741_824 => 15u64,
                    _ => return Err(Error::Corrupt("chunk segment minor")),
                };
                if &header[..8] != b"ULMDBSEG"
                    || header[8..10] != 0u16.to_le_bytes()
                    || header[12..16] != 128u32.to_le_bytes()
                    || header[16..24] != e.id.to_le_bytes()
                    || header[24..28] != 4096u32.to_le_bytes()
                    || header[28] != 1
                    || header[29..32] != [0; 3]
                    || header[32..40] != 4096u64.to_le_bytes()
                    || header[40..48] != (e.length - 4096).to_le_bytes()
                    || header[48..56] != e.length.to_le_bytes()
                    || header[56..64] != features.to_le_bytes()
                    || header[64..].iter().any(|b| *b != 0)
                {
                    return Err(Error::Corrupt("chunk segment header"));
                }
            }
            files.insert(e.id, source);
        }
        Ok(Self {
            sources: Arc::new(Sources {
                creator: std::process::id(),
                files,
                entries: entries.to_vec(),
                cache,
                limits,
                page_size,
                value_limit,
            }),
            pins: ManuallyDrop::new(Mutex::new(BTreeMap::new())),
        })
    }
    pub(crate) fn join_sources(
        entries: &[SegmentEntry],
        origins: &BTreeMap<u64, &Self>,
    ) -> Result<Self> {
        crate::metadata::validate_entries(entries)?;
        let first = origins
            .values()
            .next()
            .ok_or(Error::InvalidInput("empty cached source join"))?;
        first.check_process()?;
        if entries.len() > first.sources.limits.sources {
            return Err(Error::LimitExceeded);
        }
        let mut files = BTreeMap::new();
        for entry in entries {
            let owner = origins
                .get(&entry.id)
                .ok_or(Error::Corrupt("cached source join origin"))?;
            owner.check_process()?;
            if !Arc::ptr_eq(&first.sources.cache, &owner.sources.cache)
                || first.sources.limits.sources != owner.sources.limits.sources
                || first.sources.limits.pins != owner.sources.limits.pins
                || first.sources.page_size != owner.sources.page_size
            {
                return Err(Error::InvalidInput("cached source join policy differs"));
            }
            let at = owner
                .sources
                .entries
                .binary_search_by_key(&entry.id, |e| e.id)
                .map_err(|_| Error::Corrupt("cached source join missing"))?;
            if !crate::metadata::same_entry(entry, &owner.sources.entries[at]) {
                return Err(Error::Corrupt("cached source join identity"));
            }
            files.insert(
                entry.id,
                owner
                    .sources
                    .files
                    .get(&entry.id)
                    .ok_or(Error::Corrupt("cached source join fd"))?
                    .clone(),
            );
        }
        Ok(Self {
            sources: Arc::new(Sources {
                creator: std::process::id(),
                files,
                entries: entries.to_vec(),
                cache: first.sources.cache.clone(),
                limits: first.sources.limits,
                page_size: first.sources.page_size,
                value_limit: first.sources.value_limit,
            }),
            pins: ManuallyDrop::new(Mutex::new(BTreeMap::new())),
        })
    }
    fn check_process(&self) -> Result<()> {
        if self.sources.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited chunk table"));
        }
        Ok(())
    }
    pub fn entries(&self) -> &[SegmentEntry] {
        &self.sources.entries
    }
    pub fn limits(&self) -> PinLimits {
        self.sources.limits
    }
    pub fn contains(&self, id: u64) -> bool {
        self.sources.files.contains_key(&id)
    }
    pub fn select_entries(&self, entries: &[SegmentEntry]) -> Result<Self> {
        self.check_process()?;
        self.sources.limits.validate()?;
        crate::metadata::validate_entries(entries)?;
        if entries.len() > self.sources.limits.sources {
            return Err(Error::LimitExceeded);
        }
        let mut files = BTreeMap::new();
        for entry in entries {
            let original = self
                .sources
                .entries
                .binary_search_by_key(&entry.id, |e| e.id)
                .ok()
                .map(|n| &self.sources.entries[n])
                .ok_or(Error::Corrupt("cached source absent"))?;
            if !crate::metadata::same_entry(original, entry) {
                return Err(Error::Corrupt("cached source identity changed"));
            }
            files.insert(
                entry.id,
                self.sources
                    .files
                    .get(&entry.id)
                    .ok_or(Error::Corrupt("cached source descriptor absent"))?
                    .clone(),
            );
        }
        Ok(Self {
            sources: Arc::new(Sources {
                creator: std::process::id(),
                files,
                entries: entries.to_vec(),
                cache: self.sources.cache.clone(),
                limits: self.sources.limits,
                page_size: self.sources.page_size,
                value_limit: self.sources.value_limit,
            }),
            pins: ManuallyDrop::new(Mutex::new(BTreeMap::new())),
        })
    }
    pub fn scope(&self) -> Self {
        Self {
            sources: self.sources.clone(),
            pins: ManuallyDrop::new(Mutex::new(BTreeMap::new())),
        }
    }
    pub fn cache(&self) -> &Arc<ChunkCache> {
        &self.sources.cache
    }
    pub fn pinned_requests(&self) -> Result<usize> {
        self.check_process()?;
        Ok(self.pins.lock().map_err(|_| Error::Busy)?.len())
    }
    pub fn value_limit(&self) -> ulmdb_core::ValueLimit {
        self.sources.value_limit
    }
}
impl ReadStore for ChunkedSegmentTable {
    fn page_size(&self) -> usize {
        self.sources.page_size.get()
    }
    fn max_value_size(&self) -> usize {
        self.sources.value_limit.get()
    }
    fn validate_extent(&self, at: Reference, length: usize) -> Result<()> {
        self.check_process()?;
        if at.offset < 4096 {
            return Err(Error::Corrupt("reference targets segment header"));
        }
        let source = self
            .sources
            .files
            .get(&at.segment)
            .ok_or(Error::Corrupt("unknown chunk segment"))?;
        source.validate_extent(at.offset, length)
    }
    #[allow(unsafe_code)]
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        self.check_process()?;
        if at.offset < 4096 {
            return Err(Error::Corrupt("reference targets segment header"));
        }
        let source = self
            .sources
            .files
            .get(&at.segment)
            .ok_or(Error::Corrupt("unknown chunk segment"))?;
        let _end = at
            .offset
            .checked_add(length as u64)
            .filter(|n| *n <= source.length())
            .ok_or(Error::Corrupt("chunk table read bounds"))?;
        if length == 0 {
            let _ = self.sources.cache.pin(source, at.offset, 0)?;
            return Ok(&[]);
        }
        // The cache authenticates and supplies the exact requested span. Retain one
        // owner per actual Window, so distinct offsets/lengths never accumulate
        // per-query metadata for an already-pinned mmap.
        let pin = self.sources.cache.pin(source, at.offset, length)?;
        let identity = pin
            .window_identity()
            .ok_or(Error::Corrupt("nonempty chunk pin owner"))?;
        let pointer = pin.as_ptr();
        let mut guard = self.pins.lock().map_err(|_| Error::Busy)?;
        if !guard.contains_key(&identity) {
            if guard.len() >= self.sources.limits.pins {
                return Err(Error::LimitExceeded);
            }
            guard.insert(identity, Box::new(pin));
        }
        drop(guard);
        // SAFETY: arena owners are append-only and never removed/replaced. The
        // Window identity is its Arc allocation address; a retained owner prevents
        // ABA and holds the entire actual immutable contiguous mmap, including a
        // different subspan requested by a temporary duplicate pin. Only dropping
        // self frees owners, which Rust forbids while this &self-bound slice lives.
        // Cache eviction cannot remove our Arc owner. No raw pointer escapes.
        Ok(unsafe { std::slice::from_raw_parts(pointer, length) })
    }
}

impl Drop for ChunkedSegmentTable {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        if self.sources.creator == std::process::id() {
            // SAFETY: this owner constructed pins exactly once; only its Drop
            // destroys them. An inherited child deliberately leaves the arena
            // untouched: a vanished parent thread could have been modifying
            // its Mutex/BTreeMap at fork. Process exit reclaims child resources.
            unsafe { ManuallyDrop::drop(&mut self.pins) }
        }
    }
}
