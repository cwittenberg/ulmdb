//! Bounded immutable file windows with explicit pointer ownership. A ChunkPin
//! keeps its exact decoded mmap alive; only unpinned LRU entries can be evicted.
//! This capability does not by itself admit a database graph or a namespace.
use ulmdb_platform::FileExt;
use crate::{
    Error, Result,
    transform::{self, ObjectKind, TransformContext},
};
use memmap2::{Mmap, MmapMut, MmapOptions};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    mem::ManuallyDrop,
    ops::Deref,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
const PAGE: usize = 4096;
mod files;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Actual OS-rounded decoded mmap bytes, including pinned and in-flight maps.
    pub mapped_bytes: u64,
    /// All live windows, including pins and in-flight loaders. This also bounds
    /// index metadata and concurrent decode scratch (less than16KiB per loader).
    pub windows: usize,
    pub chunk_bytes: usize,
    /// A contiguous request crossing chunks gets a real single mmap. Requests
    /// exceeding this bound fail before eviction or allocation.
    pub max_window_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            mapped_bytes: 64 << 20,
            windows: 256,
            chunk_bytes: 64 << 10,
            max_window_bytes: 16 << 20,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub mapped_bytes: u64,
    pub live_windows: usize,
    pub cached_windows: usize,
    pub pinned_windows: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    /// Open/reserved Native source descriptors; direct owning File callers are independent.
    pub source_descriptors: usize,
    pub peak_source_descriptors: usize,
    pub source_descriptor_limit: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct ObjectIdentity {
    pub kind: ObjectKind,
    pub id: u128,
    pub length: u64,
    /// SHA-256 of the complete decoded object, independently supplied by its
    /// authenticated manifest; frame checks never substitute for this binding.
    pub hash: [u8; 32],
    pub file_offset: u64,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FileIdentity {
    volume: u64,
    object: u128,
    bytes: u64,
    modified: i128,
    changed: i128,
}
impl FileIdentity {
    fn of(file: &File) -> Result<Self> {
        let meta = ulmdb_platform::file_identity(file)?;
        if !meta.regular || meta.reparse || meta.directory {
            return Err(Error::Corrupt("chunk source is not a regular file"));
        }
        Ok(Self {
            volume: meta.volume,
            object: meta.file_id,
            bytes: meta.length,
            modified: meta.modified,
            changed: meta.changed,
        })
    }
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SourceKey {
    file: FileIdentity,
    kind: u16,
    id: u128,
    length: u64,
    hash: [u8; 32],
    base: u64,
    context: usize,
    advice: u8,
}
/// A read-only descriptor and an independently verified whole-object hash.
/// Source verification streams4KiB frames; no whole-object mapping is created.
/// Callers retain the ordinary immutable-source/namespace ownership contract.
pub struct AuthenticatedFile {
    creator: u32,
    file: Option<File>,
    reopen: Option<Reopen>,
    key: SourceKey,
    header: Option<[u8; 128]>,
    context: ManuallyDrop<Option<Arc<TransformContext>>>,
    read_ahead: crate::ReadAhead,
}
struct Reopen {
    directory: Arc<files::Directory>,
    relative: std::path::PathBuf,
}
enum SourceFile<'a> {
    Direct(&'a File),
    Reopened(files::Opened),
}
impl Deref for SourceFile<'_> {
    type Target = File;
    fn deref(&self) -> &File {
        match self {
            Self::Direct(file) => file,
            Self::Reopened(opened) => &opened.file,
        }
    }
}
impl AuthenticatedFile {
    pub fn open(
        file: File,
        identity: ObjectIdentity,
        context: Option<Arc<TransformContext>>,
    ) -> Result<Arc<Self>> {
        Self::open_with_read_ahead(file, identity, context, crate::ReadAhead::Normal)
    }
    pub fn open_with_read_ahead(
        file: File,
        identity: ObjectIdentity,
        context: Option<Arc<TransformContext>>,
        read_ahead: crate::ReadAhead,
    ) -> Result<Arc<Self>> {
        read_ahead.validate()?;
        if identity.length == 0 || identity.length > ulmdb_core::ValueLimit::MAXIMUM.segment_limit()
        {
            return Err(Error::LimitExceeded);
        }
        let physical = FileIdentity::of(&file)?;
        let physical_length = match &context {
            Some(c) => c.encoded_length(identity.length)?,
            None => identity.length,
        };
        if identity
            .file_offset
            .checked_add(physical_length)
            .is_none_or(|n| n > physical.bytes)
        {
            return Err(Error::Corrupt("chunk source extent"));
        }
        let header = context
            .as_ref()
            .map(|c| {
                c.checked_object_header(
                    &file,
                    identity.file_offset,
                    identity.kind,
                    identity.id,
                    identity.length,
                )
            })
            .transpose()?;
        let source = Arc::new(Self {
            creator: std::process::id(),
            file: Some(file),
            reopen: None,
            key: SourceKey {
                file: physical,
                kind: identity.kind as u16,
                id: identity.id,
                length: identity.length,
                hash: identity.hash,
                base: identity.file_offset,
                context: context.as_ref().map_or(0, |c| Arc::as_ptr(c) as usize),
                advice: u8::from(read_ahead == crate::ReadAhead::Random),
            },
            header,
            context: ManuallyDrop::new(context),
            read_ahead,
        });
        let mut digest = Sha256::new();
        let mut page = [0; PAGE];
        let mut scratch = source
            .context
            .as_ref()
            .map(|c| transform::FrameScratch::new(c));
        for start in (0..identity.length).step_by(PAGE) {
            let count = (identity.length - start).min(PAGE as u64) as usize;
            ulmdb_core::work::checkpoint(1, count as u64)?;
            source.frame(
                source.file.as_ref().unwrap(),
                start,
                &mut page[..count],
                scratch.as_mut(),
            )?;
            digest.update(&page[..count]);
        }
        if <[u8; 32]>::from(digest.finalize()) != identity.hash {
            return Err(Error::Corrupt("chunk source hash"));
        }
        source.check()?;
        Ok(source)
    }
    pub(crate) fn open_reopenable(
        directory: Arc<files::Directory>,
        relative: &std::path::Path,
        identity: ObjectIdentity,
        physical_bounds: (u64, u64),
        context: Option<Arc<TransformContext>>,
        read_ahead: crate::ReadAhead,
    ) -> Result<Arc<Self>> {
        let opened = directory.open(relative)?;
        let length = opened.file.metadata()?.len();
        if length < physical_bounds.0 || length > physical_bounds.1 {
            return Err(Error::Corrupt("chunk segment physical length"));
        }
        let source = Self::open_with_read_ahead(opened.file, identity, context, read_ahead)?;
        directory.check()?;
        let mut source = Arc::try_unwrap(source)
            .map_err(|_| Error::Corrupt("new chunk source unexpectedly shared"))?;
        source.reopen = Some(Reopen {
            directory,
            relative: relative.to_path_buf(),
        });
        drop(source.file.take());
        Ok(Arc::new(source))
    }
    fn acquire(&self) -> Result<SourceFile<'_>> {
        check_process(self.creator)?;
        if let Some(file) = &self.file {
            return Ok(SourceFile::Direct(file));
        }
        let reopen = self
            .reopen
            .as_ref()
            .ok_or(Error::Corrupt("chunk source missing backing"))?;
        Ok(SourceFile::Reopened(
            reopen.directory.open(&reopen.relative)?,
        ))
    }
    pub fn length(&self) -> u64 {
        self.key.length
    }
    pub(crate) fn validate_extent(&self, offset: u64, length: usize) -> Result<()> {
        self.check()?;
        if offset
            .checked_add(length as u64)
            .is_none_or(|n| n > self.length())
        {
            return Err(Error::Corrupt("chunk read bounds"));
        }
        Ok(())
    }
    fn check(&self) -> Result<()> {
        let file = self.acquire()?;
        self.check_file(&file)
    }
    fn check_file(&self, file: &File) -> Result<()> {
        check_process(self.creator)?;
        if let Some(reopen) = &self.reopen {
            // Authenticate the current namespace entry too, including after
            // callbacks/IO: a cached mmap must not mask a replaced pathname.
            let current = reopen.directory.open(&reopen.relative)?;
            if FileIdentity::of(&current.file)? != self.key.file {
                return Err(Error::Corrupt("chunk source replaced"));
            }
        }
        if FileIdentity::of(&file)? != self.key.file {
            return Err(Error::Corrupt("chunk source changed"));
        }
        Ok(())
    }
    fn frame(
        &self,
        file: &File,
        start: u64,
        destination: &mut [u8],
        scratch: Option<&mut transform::FrameScratch>,
    ) -> Result<()> {
        if let Some(context) = self.context.as_ref() {
            transform::decode_frame(
                context,
                file,
                self.key.base,
                self.header.as_ref().unwrap(),
                start / PAGE as u64,
                destination,
                scratch.unwrap(),
            )
        } else {
            file.read_exact_at(destination, self.key.base + start)
                .map_err(Into::into)
        }
    }
    fn window(
        &self,
        file: &File,
        start: u64,
        length: usize,
    ) -> Result<(Mmap, Option<transform::DecodedLease>)> {
        self.check_file(file)?;
        if self.context.is_none() {
            let at = self
                .key
                .base
                .checked_add(start)
                .ok_or(Error::LimitExceeded)?;
            // SAFETY: callers own the immutable-file contract. Hash/identity
            // verification bound this exact descriptor and bounded prefix.
            // Every pin owns its mmap; eviction cannot revoke live bytes.
            #[allow(unsafe_code)]
            let map = unsafe { MmapOptions::new().offset(at).len(length).map(file)? };
            self.read_ahead.apply(&map);
            self.check_file(file)?;
            return Ok((map, None));
        }
        let lease = self
            .context
            .as_ref()
            .map(|c| c.reserve_cache_mapping(length as u64))
            .transpose()?;
        let mut map = MmapMut::map_anon(length)?;
        let mut scratch = self
            .context
            .as_ref()
            .map(|c| transform::FrameScratch::new(c));
        for index in (0..length).step_by(PAGE) {
            let count = (length - index).min(PAGE);
            ulmdb_core::work::checkpoint(1, count as u64)?;
            self.frame(
                file,
                start + index as u64,
                &mut map[index..index + count],
                scratch.as_mut(),
            )?;
        }
        self.check_file(file)?;
        let map = map.make_read_only()?;
        self.read_ahead.apply(&map);
        Ok((map, lease))
    }
    fn window_charge(&self, start: u64, length: usize) -> Result<u64> {
        let prefix = if self.context.is_none() {
            let page = crate::residency::os::page_size()? as u64;
            self.key
                .base
                .checked_add(start)
                .ok_or(Error::LimitExceeded)?
                % page
        } else {
            0
        };
        TransformContext::mapped_charge(
            (length as u64)
                .checked_add(prefix)
                .ok_or(Error::LimitExceeded)?,
        )
    }
}
impl Drop for AuthenticatedFile {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            // SAFETY: this field is dropped exactly once, only in its creator.
            #[allow(unsafe_code)]
            unsafe {
                ManuallyDrop::drop(&mut self.context)
            };
        }
    }
}
fn check_process(creator: u32) -> Result<()> {
    if creator != std::process::id() {
        return Err(Error::InvalidInput("chunk cache inherited across fork"));
    }
    Ok(())
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    source: SourceKey,
    start: u64,
    length: usize,
}
struct Counts {
    bytes: u64,
    windows: usize,
}
struct Budget {
    creator: u32,
    limits: Limits,
    used: Mutex<Counts>,
}
struct Charge {
    budget: Arc<Budget>,
    bytes: u64,
}
impl Budget {
    fn reserve(self: &Arc<Self>, bytes: u64) -> Result<Charge> {
        check_process(self.creator)?;
        let mut used = self.used.lock().map_err(|_| Error::Busy)?;
        let total = used.bytes.checked_add(bytes).ok_or(Error::LimitExceeded)?;
        if total > self.limits.mapped_bytes || used.windows >= self.limits.windows {
            return Err(Error::LimitExceeded);
        }
        used.bytes = total;
        used.windows += 1;
        Ok(Charge {
            budget: self.clone(),
            bytes,
        })
    }
}
impl Drop for Charge {
    fn drop(&mut self) {
        if self.budget.creator != std::process::id() {
            return;
        }
        let mut used = self
            .budget
            .used
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        used.bytes -= self.bytes;
        used.windows -= 1;
    }
}
struct Window {
    map: Mmap,
    _decoded: Option<transform::DecodedLease>,
    _charge: Charge,
}
struct Entry {
    window: Arc<Window>,
    used: u64,
}
struct State {
    entries: BTreeMap<Key, Entry>,
    clock: u64,
}
pub struct ChunkCache {
    creator: u32,
    limits: Limits,
    budget: Arc<Budget>,
    state: ManuallyDrop<Mutex<State>>,
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
    files: Arc<files::Pool>,
}
/// The cache may be dropped while pins remain live. Charges, decoded callback
/// owners and the mmap survive until the final pin releases the actual bytes.
#[derive(Clone)]
pub struct ChunkPin {
    creator: u32,
    window: ManuallyDrop<Option<Arc<Window>>>,
    start: usize,
    length: usize,
}
impl Drop for ChunkPin {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            // SAFETY: every pin exclusively owns its field. In a fork child we
            // retain it because decoded-context teardown may take parent locks.
            #[allow(unsafe_code)]
            unsafe {
                ManuallyDrop::drop(&mut self.window)
            };
        }
    }
}
impl ChunkPin {
    /// Process-local deduplication identity only. Keeping any pin alive prevents
    /// address reuse; this does not confer source or snapshot authority.
    pub(crate) fn window_identity(&self) -> Option<usize> {
        self.window
            .as_ref()
            .map(|window| Arc::as_ptr(window) as usize)
    }
}
impl Deref for ChunkPin {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.window
            .as_ref()
            .map_or(&[], |w| &w.map[self.start..self.start + self.length])
    }
}
impl ChunkCache {
    pub fn new(limits: Limits) -> Result<Arc<Self>> {
        Self::new_with_file_limit(limits, 32)
    }
    /// Bound all descriptors retained/reserved by reopenable Native sources.
    /// Direct AuthenticatedFile::open(File) retains its caller-owned descriptor.
    pub fn new_with_file_limit(limits: Limits, source_descriptors: usize) -> Result<Arc<Self>> {
        // Root anchor + active file + parent lookup + final named-file check.
        if source_descriptors < 4 {
            return Err(Error::InvalidInput("chunk source descriptor limit"));
        }
        let files = files::Pool::new(source_descriptors)?;
        let page = crate::residency::os::page_size()?;
        if limits.windows == 0
            || limits.windows > 65_536
            || limits.chunk_bytes == 0
            || !limits.chunk_bytes.is_multiple_of(PAGE)
            || !limits.chunk_bytes.is_multiple_of(page)
            || limits.max_window_bytes < limits.chunk_bytes
            || limits.max_window_bytes > isize::MAX as usize
            || limits.max_window_bytes as u64 > ulmdb_core::ValueLimit::MAXIMUM.segment_limit()
            || limits.mapped_bytes < page as u64
            || !limits.mapped_bytes.is_multiple_of(page as u64)
        {
            return Err(Error::InvalidInput("chunk cache limits"));
        }
        Ok(Arc::new(Self {
            creator: std::process::id(),
            limits,
            budget: Arc::new(Budget {
                creator: std::process::id(),
                limits,
                used: Mutex::new(Counts {
                    bytes: 0,
                    windows: 0,
                }),
            }),
            state: ManuallyDrop::new(Mutex::new(State {
                entries: BTreeMap::new(),
                clock: 0,
            })),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            files,
        }))
    }
    pub(crate) fn source_directory(&self, path: &std::path::Path) -> Result<Arc<files::Directory>> {
        check_process(self.creator)?;
        self.files.directory(path)
    }
    pub fn pin(&self, source: &AuthenticatedFile, offset: u64, length: usize) -> Result<ChunkPin> {
        check_process(self.creator)?;
        let file = source.acquire()?;
        source.check_file(&file)?;
        let end = offset
            .checked_add(length as u64)
            .filter(|n| *n <= source.length())
            .ok_or(Error::Corrupt("chunk read bounds"))?;
        if length == 0 {
            return Ok(ChunkPin {
                creator: self.creator,
                window: ManuallyDrop::new(None),
                start: 0,
                length: 0,
            });
        }
        let width = self.limits.chunk_bytes as u64;
        let start = offset / width * width;
        let last = end.checked_add(width - 1).ok_or(Error::LimitExceeded)? / width * width;
        let size =
            usize::try_from(last.min(source.length()) - start).map_err(|_| Error::LimitExceeded)?;
        let charge = source.window_charge(start, size)?;
        if size > self.limits.max_window_bytes || charge > self.limits.mapped_bytes {
            return Err(Error::LimitExceeded);
        }
        let key = Key {
            source: source.key,
            start,
            length: size,
        };
        {
            let mut state = self.state.lock().map_err(|_| Error::Busy)?;
            state.clock = state.clock.wrapping_add(1);
            let clock = state.clock;
            if let Some(entry) = state.entries.get_mut(&key) {
                entry.used = clock;
                self.hits.fetch_add(1, Ordering::Relaxed);
                return Ok(ChunkPin {
                    creator: self.creator,
                    window: ManuallyDrop::new(Some(entry.window.clone())),
                    start: (offset - start) as usize,
                    length,
                });
            }
        }
        self.misses.fetch_add(1, Ordering::Relaxed);
        let window = loop {
            let reservation = loop {
                match self.budget.reserve(charge) {
                    Ok(lease) => break lease,
                    Err(Error::LimitExceeded) if self.evict_one()? => continue,
                    Err(error) => return Err(error),
                }
            };
            // No cache mutex is held while callbacks, IO, allocation or destructors run.
            match source.window(&file, start, size) {
                Ok((map, decoded)) => {
                    break Arc::new(Window {
                        map,
                        _decoded: decoded,
                        _charge: reservation,
                    });
                }
                Err(Error::LimitExceeded) => {
                    drop(reservation);
                    if !self.evict_matching(Some(source.key.context))? {
                        return Err(Error::LimitExceeded);
                    }
                }
                Err(error) => return Err(error),
            }
        };
        let mut state = self.state.lock().map_err(|_| Error::Busy)?;
        state.clock = state.clock.wrapping_add(1);
        let clock = state.clock;
        let owner = match state.entries.get_mut(&key) {
            Some(existing) => {
                existing.used = clock;
                existing.window.clone()
            }
            None => {
                state.entries.insert(
                    key,
                    Entry {
                        window: window.clone(),
                        used: clock,
                    },
                );
                window.clone()
            }
        };
        drop(state);
        Ok(ChunkPin {
            creator: self.creator,
            window: ManuallyDrop::new(Some(owner)),
            start: (offset - start) as usize,
            length,
        })
    }
    fn evict_one(&self) -> Result<bool> {
        self.evict_matching(None)
    }
    fn evict_matching(&self, context: Option<usize>) -> Result<bool> {
        check_process(self.creator)?;
        let removed = {
            let mut state = self.state.lock().map_err(|_| Error::Busy)?;
            let victim = state
                .entries
                .iter()
                .filter(|(key, e)| {
                    context.is_none_or(|c| key.source.context == c)
                        && Arc::strong_count(&e.window) == 1
                })
                .min_by_key(|(_, e)| e.used)
                .map(|(key, _)| *key);
            victim.and_then(|key| state.entries.remove(&key))
        };
        if removed.is_some() {
            self.evictions.fetch_add(1, Ordering::Relaxed);
        }
        let existed = removed.is_some();
        drop(removed);
        Ok(existed)
    }
    pub fn trim_unpinned(&self) -> Result<usize> {
        let mut removed = 0;
        while self.evict_one()? {
            removed += 1;
        }
        Ok(removed)
    }
    pub fn stats(&self) -> Result<Stats> {
        check_process(self.creator)?;
        let state = self.state.lock().map_err(|_| Error::Busy)?;
        let used = self.budget.used.lock().map_err(|_| Error::Busy)?;
        let (source_descriptors, peak_source_descriptors, source_descriptor_limit) =
            self.files.stats()?;
        Ok(Stats {
            mapped_bytes: used.bytes,
            live_windows: used.windows,
            cached_windows: state.entries.len(),
            pinned_windows: state
                .entries
                .values()
                .filter(|e| Arc::strong_count(&e.window) > 1)
                .count(),
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            source_descriptors,
            peak_source_descriptors,
            source_descriptor_limit,
        })
    }
}
impl Drop for ChunkCache {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            // SAFETY: exclusive destruction in the creator. A fork child must
            // not walk a tree another parent thread may have been mutating.
            #[allow(unsafe_code)]
            unsafe {
                ManuallyDrop::drop(&mut self.state)
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[cfg(unix)]
    #[test]
    #[allow(unsafe_code)]
    fn inherited_cache_rejects_before_parent_locks_and_drops_without_locking() {
        #[cfg(target_os = "linux")]
        const NAME: &str = "chunk_cache::tests::inherited_cache_rejects_before_parent_locks_and_drops_without_locking";
        #[cfg(target_os = "linux")]
        if crate::history::tests::run_isolated(NAME) {
            return;
        }
        let path = std::env::temp_dir().join(format!(
            "ulmdb-cache-fork-{:032x}",
            crate::new_identity().unwrap()
        ));
        let bytes = [17u8; PAGE];
        File::create(&path).unwrap().write_all(&bytes).unwrap();
        let source = AuthenticatedFile::open(
            File::open(&path).unwrap(),
            ObjectIdentity {
                kind: ObjectKind::Segment,
                id: 1,
                length: PAGE as u64,
                hash: Sha256::digest(bytes).into(),
                file_offset: 0,
            },
            None,
        )
        .unwrap();
        let cache = ChunkCache::new(Limits::default()).unwrap();
        let pin = cache.pin(&source, 0, PAGE).unwrap();
        let state = cache.state.lock().unwrap();
        let budget = cache.budget.used.lock().unwrap();
        // Both inherited mutexes are deliberately locked. The child must reject
        // each operation and drop all owners without trying either mutex.
        let child = unsafe { libc::fork() };
        assert!(child >= 0);
        if child == 0 {
            unsafe { libc::alarm(3) };
            let rejected = matches!(cache.stats(), Err(Error::InvalidInput(_)))
                && matches!(cache.trim_unpinned(), Err(Error::InvalidInput(_)))
                && matches!(cache.pin(&source, 0, PAGE), Err(Error::InvalidInput(_)))
                && matches!(source.validate_extent(0, PAGE), Err(Error::InvalidInput(_)))
                && &*pin == bytes;
            std::mem::forget(state);
            std::mem::forget(budget);
            drop(pin);
            drop(source);
            drop(cache);
            unsafe { libc::_exit(if rejected { 0 } else { 1 }) };
        }
        drop(state);
        drop(budget);
        let mut status = 0;
        assert_eq!(unsafe { libc::waitpid(child, &mut status, 0) }, child);
        assert!(libc::WIFEXITED(status));
        assert_eq!(libc::WEXITSTATUS(status), 0);
        assert_eq!(cache.stats().unwrap().live_windows, 1);
        assert_eq!(&*pin, bytes);
        drop(pin);
        assert_eq!(cache.trim_unpinned().unwrap(), 1);
        assert_eq!(cache.stats().unwrap().mapped_bytes, 0);
        std::fs::remove_file(path).unwrap();
        #[cfg(target_os = "linux")]
        crate::history::tests::worker_completed(NAME);
    }
}
