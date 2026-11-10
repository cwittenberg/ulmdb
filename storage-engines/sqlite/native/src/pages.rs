//! Optional SQLite page file over the existing FULL synchronous native log.
//! SQLite owns its B-tree/pager and rollback journal; this file owns only the
//! immutable native snapshot and the bounded pages in its current write batch.
use super::pages_codec;
use super::{Binding, binding, check_registry_process, process_id};
use std::{
    collections::BTreeMap,
    ffi::{CStr, c_char, c_int, c_void},
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicU64, Ordering},
    },
};
use ulmdb::{DatabaseKind, Error, snapshot_log as native};

const OK: i32 = 0;
const BUSY: i32 = 5;
const READONLY: i32 = 8;
const IO: i32 = 10;
const CORRUPT: i32 = 11;
const FULL: i32 = 13;
const CANTOPEN: i32 = 14;
const MISUSE: i32 = 21;
const NOTADB: i32 = 26;
const SHORT_READ: i32 = IO | (2 << 8);
const SHARED: i32 = 1;
const RESERVED: i32 = 2;
const PENDING: i32 = 3;
const EXCLUSIVE: i32 = 4;
const BLOCK: usize = 4096;
const LIMIT: u64 = 64 << 20;
const ENTRY_CHARGE: usize = 128;
const NAME: &[u8] = b"sqlite_pages_v1";
const MAGIC: &[u8; 8] = b"USQLPG02";
const LEGACY_MAGIC: &[u8; 8] = b"USQLPG01";
const META: [u8; 8] = [0; 8];
type Result<T> = std::result::Result<T, i32>;

struct Owner {
    engine: native::Environment,
    binding: Binding,
    locks: Mutex<BTreeMap<u64, i32>>,
}
static OWNERS: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Owner>>>> = OnceLock::new();
static NEXT_FILE: AtomicU64 = AtomicU64::new(1);

struct Delta {
    pages: BTreeMap<u64, Box<[u8; BLOCK]>>,
    length: u64,
    cleared_from: Option<u64>,
}
/// Every field is owned and Send; SQLite's file-call serialization is required.
/// No thread-affine native writer or borrowed result survives an ABI call.
#[allow(non_camel_case_types)]
pub struct UpgFile {
    creator: u32,
    id: u64,
    owner: Arc<Owner>,
    reader: Option<native::ReadTxn>,
    delta: Option<Delta>,
    atomic: bool,
    readonly: bool,
    lock: i32,
    fatal: bool,
}
fn options() -> native::Options {
    native::Options {
        index_bytes: LIMIT as usize,
        journal_capacity: LIMIT as usize,
        transaction_bytes: LIMIT as usize - 4096,
        max_operations: 65536,
        max_databases: 1,
        versioned_values: false,
        ..Default::default()
    }
}
fn error(error: Error) -> i32 {
    match error {
        Error::LimitExceeded => FULL,
        Error::Busy => IO, // Native mutex poison, never a retryable file lock.
        Error::MissingDatabase => NOTADB,
        Error::Corrupt(_) => CORRUPT,
        _ => IO,
    }
}
fn boundary(action: impl FnOnce() -> Result<()>) -> i32 {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(Ok(())) => OK,
        Ok(Err(rc)) => rc,
        Err(_) => IO,
    }
}
fn metadata(length: u64) -> [u8; 16] {
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..].copy_from_slice(&length.to_le_bytes());
    bytes
}
fn read_length(read: &native::ReadTxn) -> Result<u64> {
    let bytes = read.get(NAME, &META).map_err(error)?.ok_or(NOTADB)?;
    if bytes.len() != 16 || (&bytes[..8] != MAGIC && &bytes[..8] != LEGACY_MAGIC) {
        return Err(NOTADB);
    }
    let length = u64::from_le_bytes(bytes[8..].try_into().map_err(|_| CORRUPT)?);
    if length > LIMIT {
        return Err(CORRUPT);
    }
    Ok(length)
}
fn page_key(block: u64) -> [u8; 8] {
    (block + 1).to_be_bytes()
}
fn blocks(length: u64) -> u64 {
    length.div_ceil(BLOCK as u64)
}
fn owner(path: PathBuf, create: bool) -> Result<Arc<Owner>> {
    check_registry_process().map_err(|_| MISUSE)?;
    if !path.is_absolute() {
        return Err(CANTOPEN);
    }
    let created_directory = !path.exists();
    if created_directory {
        if !create {
            return Err(CANTOPEN);
        }
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|_| CANTOPEN)?;
    }
    let store_metadata = std::fs::symlink_metadata(&path).map_err(|_| CANTOPEN)?;
    if !store_metadata.is_dir()
        || store_metadata.uid() != unsafe { libc::geteuid() }
        || store_metadata.permissions().mode() & 0o077 != 0
    {
        return Err(CANTOPEN);
    }
    let path = path.canonicalize().map_err(|_| CANTOPEN)?;
    let mut registry = OWNERS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| IO)?;
    registry.retain(|_, owner| owner.strong_count() != 0);
    if let Some(owner) = registry.get(&path).and_then(Weak::upgrade) {
        if binding(&path).map_err(|_| CANTOPEN)? != owner.binding {
            return Err(CANTOPEN);
        }
        return Ok(owner);
    }
    let engine = if path.join("SNAPSHOTLOG").exists() {
        native::Environment::open(&path, options()).map_err(error)?
    } else {
        if !create
            || std::fs::read_dir(&path)
                .map_err(|_| CANTOPEN)?
                .next()
                .is_some()
        {
            return Err(CANTOPEN);
        }
        let engine = native::Environment::create(&path, options()).map_err(error)?;
        {
            let mut write = engine.begin_write().map_err(error)?;
            write
                .create_database(NAME, DatabaseKind::Kv)
                .map_err(error)?;
            write.put(NAME, &META, &metadata(0)).map_err(error)?;
            write.commit().map_err(error)?;
        }
        engine
    };
    read_length(&engine.begin_read())?;
    if created_directory {
        std::fs::File::open(path.parent().ok_or(CANTOPEN)?)
            .and_then(|parent| parent.sync_all())
            .map_err(|_| IO)?;
    }
    let owner = Arc::new(Owner {
        engine,
        binding: binding(&path).map_err(|_| CANTOPEN)?,
        locks: Mutex::new(BTreeMap::new()),
    });
    registry.insert(path, Arc::downgrade(&owner));
    Ok(owner)
}
unsafe fn file<'a>(raw: *mut UpgFile) -> Result<&'a mut UpgFile> {
    let file = unsafe { raw.as_mut() }.ok_or(MISUSE)?;
    if file.creator != process_id() || file.creator == 0 {
        return Err(MISUSE);
    }
    Ok(file)
}
impl UpgFile {
    fn live(&self) -> Result<()> {
        if self.fatal { Err(IO) } else { Ok(()) }
    }
    fn current_length(&self) -> Result<u64> {
        self.live()?;
        if let Some(delta) = &self.delta {
            return Ok(delta.length);
        }
        if let Some(read) = &self.reader {
            read_length(read)
        } else {
            read_length(&self.owner.engine.begin_read())
        }
    }
    fn start_write(&mut self) -> Result<()> {
        self.live()?;
        if self.readonly {
            return Err(READONLY);
        }
        if self.lock != EXCLUSIVE {
            return Err(BUSY);
        }
        if self.delta.is_none() {
            if self.reader.is_none() {
                self.reader = Some(self.owner.engine.begin_read());
            }
            let length = read_length(self.reader.as_ref().ok_or(IO)?)?;
            self.delta = Some(Delta {
                pages: BTreeMap::new(),
                length,
                cleared_from: None,
            });
        }
        Ok(())
    }
    fn read_page_range(&self, block: u64, within: usize, out: &mut [u8]) -> Result<()> {
        if let Some(delta) = &self.delta {
            if let Some(page) = delta.pages.get(&block) {
                out.copy_from_slice(&page[within..within + out.len()]);
                return Ok(());
            }
            if delta.cleared_from.is_some_and(|first| block >= first) {
                out.fill(0);
                return Ok(());
            }
        }
        let temporary;
        let read = if let Some(read) = &self.reader {
            read
        } else {
            temporary = self.owner.engine.begin_read();
            &temporary
        };
        if let Some(bytes) = read.get(NAME, &page_key(block)).map_err(error)? {
            if bytes.len() == BLOCK {
                out.copy_from_slice(&bytes[within..within + out.len()]);
            } else if within == 0 && out.len() == BLOCK {
                pages_codec::decode_into(bytes, out.try_into().map_err(|_| IO)?)
                    .map_err(|_| CORRUPT)?;
            } else {
                let mut page = [0; BLOCK];
                pages_codec::decode_into(bytes, &mut page).map_err(|_| CORRUPT)?;
                out.copy_from_slice(&page[within..within + out.len()]);
            }
        } else {
            out.fill(0);
        }
        Ok(())
    }
    fn load_page(&self, block: u64) -> Result<Box<[u8; BLOCK]>> {
        let mut out = Box::new([0; BLOCK]);
        self.read_page_range(block, 0, out.as_mut())?;
        Ok(out)
    }
    fn commit(&mut self) -> Result<()> {
        self.live()?;
        let Some(delta) = self.delta.take() else {
            return Ok(());
        };
        let result = (|| {
            let mut write = self.owner.engine.begin_write().map_err(error)?;
            if let Some(first) = delta.cleared_from {
                let old = self.reader.as_ref().ok_or(IO)?;
                let length = read_length(old)?;
                for block in first..blocks(length) {
                    if !delta.pages.contains_key(&block)
                        && old.get(NAME, &page_key(block)).map_err(error)?.is_some()
                    {
                        write.delete(NAME, &page_key(block)).map_err(error)?;
                    }
                }
            }
            write
                .put(NAME, &META, &metadata(delta.length))
                .map_err(error)?;
            for (block, bytes) in &delta.pages {
                if *block < blocks(delta.length) {
                    let stored = pages_codec::encode(bytes);
                    write
                        .put(NAME, &page_key(*block), stored.as_ref())
                        .map_err(error)?;
                }
            }
            write.commit().map_err(error)?;
            Ok(())
        })();
        if result.is_ok() {
            self.reader = Some(self.owner.engine.begin_read());
        } else if result != Err(FULL) {
            self.fatal = true;
        }
        result
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn upg_open(
    path: *const c_char,
    flags: c_int,
    out: *mut *mut UpgFile,
) -> c_int {
    boundary(|| unsafe {
        check_registry_process().map_err(|_| MISUSE)?;
        if out.is_null() || path.is_null() || (flags & 3 != 1 && flags & 3 != 2) {
            return Err(MISUSE);
        }
        let path = PathBuf::from(CStr::from_ptr(path).to_str().map_err(|_| CANTOPEN)?);
        let owner = owner(path, flags & 4 != 0)?;
        let id = NEXT_FILE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| FULL)?;
        owner.locks.lock().map_err(|_| IO)?.insert(id, 0);
        *out = Box::into_raw(Box::new(UpgFile {
            creator: process_id(),
            id,
            owner,
            reader: None,
            delta: None,
            atomic: false,
            readonly: flags & 1 != 0,
            lock: 0,
            fatal: false,
        }));
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn upg_close(raw: *mut UpgFile) -> c_int {
    boundary(|| unsafe {
        let file = file(raw)?;
        file.owner
            .locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&file.id);
        drop(Box::from_raw(raw));
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn upg_read(
    raw: *mut UpgFile,
    output: *mut c_void,
    count: c_int,
    offset: i64,
) -> c_int {
    boundary(|| unsafe {
        let file = file(raw)?;
        file.live()?;
        if count < 0 || offset < 0 || (count > 0 && output.is_null()) {
            return Err(MISUSE);
        }
        if count == 0 {
            return Ok(());
        }
        let out = std::slice::from_raw_parts_mut(output.cast::<u8>(), count as usize);
        let length = file.current_length()?;
        let offset = offset as u64;
        let available = length.saturating_sub(offset).min(count as u64) as usize;
        out[available..].fill(0);
        let mut at = 0;
        while at < available {
            let position = offset + at as u64;
            let block = position / BLOCK as u64;
            let within = position as usize % BLOCK;
            let take = (BLOCK - within).min(available - at);
            file.read_page_range(block, within, &mut out[at..at + take])?;
            at += take;
        }
        if available < count as usize {
            Err(SHORT_READ)
        } else {
            Ok(())
        }
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn upg_write(
    raw: *mut UpgFile,
    input: *const c_void,
    count: c_int,
    offset: i64,
) -> c_int {
    boundary(|| unsafe {
        let file = file(raw)?;
        if count < 0 || offset < 0 || (count > 0 && input.is_null()) {
            return Err(MISUSE);
        }
        let end = (offset as u64).checked_add(count as u64).ok_or(FULL)?;
        if end > LIMIT {
            return Err(FULL);
        }
        file.start_write()?;
        if count == 0 {
            return Ok(());
        }
        let input = std::slice::from_raw_parts(input.cast::<u8>(), count as usize);
        let mut at = 0;
        while at < input.len() {
            let position = offset as u64 + at as u64;
            let block = position / BLOCK as u64;
            let within = position as usize % BLOCK;
            let take = (BLOCK - within).min(input.len() - at);
            let delta = file.delta.as_mut().ok_or(IO)?;
            if let Some(page) = delta.pages.get_mut(&block) {
                page[within..within + take].copy_from_slice(&input[at..at + take]);
                delta.length = delta.length.max(position + take as u64);
                at += take;
                continue;
            }
            if (delta.pages.len() + 1) * (BLOCK + ENTRY_CHARGE) > LIMIT as usize {
                return Err(FULL);
            }
            let page = if within == 0 && take == BLOCK {
                // Box-from-slice copies a fully supplied page without first
                // zero-initializing it; no uninitialized bytes are exposed.
                Box::<[u8]>::from(&input[at..at + BLOCK])
                    .try_into()
                    .map_err(|_| IO)?
            } else {
                let mut page = file.load_page(block)?;
                page[within..within + take].copy_from_slice(&input[at..at + take]);
                page
            };
            let delta = file.delta.as_mut().ok_or(IO)?;
            delta.pages.insert(block, page);
            delta.length = delta.length.max(position + take as u64);
            at += take;
        }
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn upg_truncate(raw: *mut UpgFile, length: i64) -> c_int {
    boundary(|| unsafe {
        let file = file(raw)?;
        if length < 0 || length as u64 > LIMIT {
            return Err(FULL);
        }
        file.start_write()?;
        let length = length as u64;
        if length < file.current_length()? {
            let first = blocks(length);
            if length % BLOCK as u64 != 0 {
                let block = length / BLOCK as u64;
                let delta = file.delta.as_mut().ok_or(IO)?;
                if let Some(page) = delta.pages.get_mut(&block) {
                    page[length as usize % BLOCK..].fill(0);
                } else {
                    let retained = delta.pages.range(..first).count();
                    if (retained + 1) * (BLOCK + ENTRY_CHARGE) > LIMIT as usize {
                        return Err(FULL);
                    }
                    let mut page = file.load_page(block)?;
                    page[length as usize % BLOCK..].fill(0);
                    file.delta.as_mut().ok_or(IO)?.pages.insert(block, page);
                }
            }
            let delta = file.delta.as_mut().ok_or(IO)?;
            delta.pages.retain(|block, _| *block < first);
            delta.cleared_from = Some(delta.cleared_from.map_or(first, |old| old.min(first)));
        }
        file.delta.as_mut().ok_or(IO)?.length = length;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn upg_size(raw: *mut UpgFile, output: *mut i64) -> c_int {
    boundary(|| unsafe {
        if output.is_null() {
            return Err(MISUSE);
        }
        *output = file(raw)?.current_length()? as i64;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn upg_lock(raw: *mut UpgFile, level: c_int) -> c_int {
    boundary(|| unsafe {
        let file = file(raw)?;
        file.live()?;
        if !(SHARED..=EXCLUSIVE).contains(&level) {
            return Err(MISUSE);
        }
        if level <= file.lock {
            return Ok(());
        }
        let mut locks = file.owner.locks.lock().map_err(|_| IO)?;
        if level == EXCLUSIVE {
            if locks
                .iter()
                .any(|(id, other)| *id != file.id && *other >= RESERVED)
            {
                return Err(BUSY);
            }
            // A failed EXCLUSIVE upgrade must stop admission of new readers.
            locks.insert(file.id, PENDING);
            file.lock = PENDING;
        }
        let blocked = locks.iter().any(|(id, other)| {
            *id != file.id
                && match level {
                    SHARED => *other >= PENDING,
                    RESERVED | PENDING => *other >= RESERVED,
                    EXCLUSIVE => *other >= SHARED,
                    _ => true,
                }
        });
        if blocked {
            return Err(BUSY);
        }
        if file.reader.is_none() {
            file.reader = Some(file.owner.engine.begin_read());
        }
        locks.insert(file.id, level);
        file.lock = level;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn upg_unlock(raw: *mut UpgFile, level: c_int) -> c_int {
    boundary(|| unsafe {
        let file = file(raw)?;
        if level != 0 && level != SHARED {
            return Err(MISUSE);
        }
        if level > file.lock {
            return Err(MISUSE);
        }
        file.owner
            .locks
            .lock()
            .map_err(|_| IO)?
            .insert(file.id, level);
        file.lock = level;
        if level == 0 {
            file.delta = None;
            file.atomic = false;
            file.reader = None;
        }
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn upg_reserved(raw: *mut UpgFile, output: *mut c_int) -> c_int {
    boundary(|| unsafe {
        if output.is_null() {
            return Err(MISUSE);
        }
        let file = file(raw)?;
        *output = i32::from(
            file.owner
                .locks
                .lock()
                .map_err(|_| IO)?
                .values()
                .any(|level| *level >= RESERVED),
        );
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn upg_lockstate(raw: *mut UpgFile, output: *mut c_int) -> c_int {
    boundary(|| unsafe {
        if output.is_null() {
            return Err(MISUSE);
        }
        *output = file(raw)?.lock;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn upg_begin_atomic(raw: *mut UpgFile) -> c_int {
    boundary(|| unsafe {
        let file = file(raw)?;
        if file.atomic || file.delta.is_some() {
            return Err(IO);
        }
        file.start_write()?;
        file.atomic = true;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn upg_commit_atomic(raw: *mut UpgFile) -> c_int {
    boundary(|| unsafe {
        let file = file(raw)?;
        if !file.atomic {
            return Err(MISUSE);
        }
        let result = file.commit();
        file.atomic = false;
        result
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn upg_rollback_atomic(raw: *mut UpgFile) -> c_int {
    boundary(|| unsafe {
        let file = file(raw)?;
        file.delta = None;
        file.atomic = false;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn upg_sync(raw: *mut UpgFile) -> c_int {
    boundary(|| unsafe {
        let file = file(raw)?;
        if file.atomic {
            file.live()
        } else {
            file.commit()
        }
    })
}

/// Diagnostic counters only; not compiled into ordinary VFS builds.
#[cfg(feature = "vfs-profile")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn upg_metrics(output: *mut u64, count: c_int) -> c_int {
    boundary(|| unsafe {
        check_registry_process().map_err(|_| MISUSE)?;
        if output.is_null() || count != 16 {
            return Err(MISUSE);
        }
        let mut totals = [0u64; 16];
        if let Some(registry) = OWNERS.get() {
            for owner in registry
                .lock()
                .map_err(|_| IO)?
                .values()
                .filter_map(Weak::upgrade)
            {
                let stats = owner.engine.stats().map_err(error)?;
                let m = stats.metrics;
                let j = stats.journal;
                let values = [
                    m.put_ns,
                    m.prepare_ns,
                    m.encode_ns,
                    m.checkpoint_ns,
                    m.append_ns,
                    m.publish_ns,
                    m.staged_clear_ns,
                    m.put_calls,
                    m.commits,
                    j.sync_calls,
                    j.append_frames,
                    j.append_bytes,
                    j.checkpoints,
                    j.checkpoint_bytes,
                    j.codec_attempts,
                    j.compressed_frames,
                ];
                for (sum, value) in totals.iter_mut().zip(values) {
                    *sum = sum.saturating_add(value);
                }
            }
        }
        std::ptr::copy_nonoverlapping(totals.as_ptr(), output, 16);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::ptr;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        path: PathBuf,
        raw: *mut UpgFile,
    }
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "ulmdb-pages-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            let mut raw = ptr::null_mut();
            let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
            assert_eq!(unsafe { upg_open(name.as_ptr(), 6, &mut raw) }, OK);
            Self { path, raw }
        }
        fn other(&self) -> *mut UpgFile {
            let name = CString::new(self.path.as_os_str().as_encoded_bytes()).unwrap();
            let mut raw = ptr::null_mut();
            assert_eq!(unsafe { upg_open(name.as_ptr(), 2, &mut raw) }, OK);
            raw
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if !self.raw.is_null() {
                assert_eq!(unsafe { upg_close(self.raw) }, OK);
            }
            std::fs::remove_dir_all(&self.path).unwrap();
        }
    }
    unsafe fn write(raw: *mut UpgFile, offset: i64, bytes: &[u8]) -> i32 {
        unsafe { upg_write(raw, bytes.as_ptr().cast(), bytes.len() as i32, offset) }
    }
    unsafe fn read(raw: *mut UpgFile, offset: i64, bytes: &mut [u8]) -> i32 {
        unsafe { upg_read(raw, bytes.as_mut_ptr().cast(), bytes.len() as i32, offset) }
    }
    #[test]
    fn page_atomic_abort_sync_truncate_and_durable_reopen() {
        let mut f = Fixture::new();
        unsafe {
            let mut out = [7; 100];
            assert_eq!(read(f.raw, 0, &mut out), SHORT_READ);
            assert_eq!(out, [0; 100]);
            assert_eq!(upg_lock(f.raw, EXCLUSIVE), OK);
            assert_eq!(upg_begin_atomic(f.raw), OK);
            let original = vec![3; BLOCK * 2];
            assert_eq!(write(f.raw, 0, &original), OK);
            assert_eq!(upg_sync(f.raw), OK); // Atomic xSync must not publish.
            let peer = f.other();
            let mut out = [9; 8];
            assert_eq!(read(peer, 0, &mut out), SHORT_READ);
            assert_eq!(out, [0; 8]);
            assert_eq!(upg_close(peer), OK);
            assert_eq!(upg_rollback_atomic(f.raw), OK);
            let mut length = -1;
            assert_eq!(upg_size(f.raw, &mut length), OK);
            assert_eq!(length, 0);
            assert_eq!(upg_begin_atomic(f.raw), OK);
            assert_eq!(write(f.raw, 0, &original), OK);
            assert_eq!(upg_commit_atomic(f.raw), OK);
            assert_eq!(upg_begin_atomic(f.raw), OK);
            assert_eq!(upg_truncate(f.raw, 17), OK);
            assert_eq!(upg_truncate(f.raw, (BLOCK * 2) as i64), OK);
            assert_eq!(write(f.raw, (BLOCK + 2) as i64, b"new"), OK);
            assert_eq!(upg_commit_atomic(f.raw), OK);
            let mut bytes = vec![1; BLOCK * 2];
            assert_eq!(read(f.raw, 0, &mut bytes), OK);
            assert_eq!(&bytes[..17], &original[..17]);
            assert!(bytes[17..BLOCK + 2].iter().all(|b| *b == 0));
            assert_eq!(&bytes[BLOCK + 2..BLOCK + 5], b"new");
            assert!(bytes[BLOCK + 5..].iter().all(|b| *b == 0));
            assert_eq!(upg_unlock(f.raw, 0), OK);
            assert_eq!(upg_close(f.raw), OK);
            f.raw = ptr::null_mut();
            let name = CString::new(f.path.as_os_str().as_encoded_bytes()).unwrap();
            assert_eq!(upg_open(name.as_ptr(), 1, &mut f.raw), OK);
            let mut reopened = vec![0; BLOCK * 2];
            assert_eq!(read(f.raw, 0, &mut reopened), OK);
            assert_eq!(reopened, bytes);
            assert_eq!(upg_lock(f.raw, EXCLUSIVE), OK);
            assert_eq!(write(f.raw, 0, b"no"), READONLY);
        }
    }
    #[test]
    fn page_partial_and_occupied_writes_preserve_snapshot_and_short_read_bytes() {
        let f = Fixture::new();
        unsafe {
            assert_eq!(upg_lock(f.raw, EXCLUSIVE), OK);
            assert_eq!(upg_begin_atomic(f.raw), OK);
            let original = vec![9; BLOCK * 2];
            assert_eq!(write(f.raw, 0, &original), OK);
            assert_eq!(upg_commit_atomic(f.raw), OK);
            assert_eq!(upg_begin_atomic(f.raw), OK);
            assert_eq!(write(f.raw, 0, &[3; BLOCK]), OK);
            assert_eq!(write(f.raw, 0, &[4; BLOCK]), OK);
            assert_eq!(write(f.raw, 5, b"patch"), OK);
            assert_eq!(write(f.raw, (BLOCK - 3) as i64, b"across"), OK);
            assert_eq!(write(f.raw, (BLOCK + 1) as i64, b"XX"), OK);
            let mut expected = vec![4; BLOCK];
            expected.extend_from_slice(&original[BLOCK..]);
            expected[5..10].copy_from_slice(b"patch");
            expected[BLOCK - 3..BLOCK + 3].copy_from_slice(b"across");
            expected[BLOCK + 1..BLOCK + 3].copy_from_slice(b"XX");
            let mut staged = vec![0; BLOCK * 2];
            assert_eq!(read(f.raw, 0, &mut staged), OK);
            assert_eq!(staged, expected);
            let peer = f.other();
            let mut committed = vec![0; BLOCK * 2];
            assert_eq!(read(peer, 0, &mut committed), OK);
            assert_eq!(committed, original);
            assert_eq!(upg_close(peer), OK);
            assert_eq!(upg_rollback_atomic(f.raw), OK);
            assert_eq!(read(f.raw, 0, &mut staged), OK);
            assert_eq!(staged, original);
            assert_eq!(upg_begin_atomic(f.raw), OK);
            assert_eq!(upg_truncate(f.raw, (BLOCK + 3) as i64), OK);
            assert_eq!(write(f.raw, (BLOCK - 2) as i64, b"edge!"), OK);
            assert_eq!(upg_commit_atomic(f.raw), OK);
            let mut short = [0x77; 12];
            assert_eq!(read(f.raw, (BLOCK - 4) as i64, &mut short), SHORT_READ);
            assert_eq!(&short[..7], &[9, 9, b'e', b'd', b'g', b'e', b'!']);
            assert_eq!(&short[7..], &[0; 5]);
        }
    }
    #[test]
    fn page_legacy_raw_store_migrates_to_mixed_codec_pages_on_write() {
        let mut f = Fixture::new();
        let mut raw_page = [0; BLOCK];
        let mut state = 91u64;
        for chunk in raw_page.chunks_exact_mut(8) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            chunk.copy_from_slice(&state.to_le_bytes());
        }
        let legacy_page = [2; BLOCK];
        let mut legacy_metadata = metadata((BLOCK * 2) as u64);
        legacy_metadata[..8].copy_from_slice(LEGACY_MAGIC);
        unsafe {
            // Reproduce a persisted store written by the original raw-page VFS.
            let mut writer = (&*f.raw).owner.engine.begin_write().unwrap();
            writer.put(NAME, &META, &legacy_metadata).unwrap();
            writer.put(NAME, &page_key(0), &raw_page).unwrap();
            writer.put(NAME, &page_key(1), &legacy_page).unwrap();
            writer.commit().unwrap();
            assert_eq!(upg_lock(f.raw, EXCLUSIVE), OK);
            let mut out = [0; BLOCK];
            assert_eq!(read(f.raw, BLOCK as i64, &mut out), OK);
            assert_eq!(out, legacy_page);
            assert_eq!(upg_begin_atomic(f.raw), OK);
            assert_eq!(write(f.raw, (BLOCK + 3) as i64, b"new"), OK);
            assert_eq!(upg_commit_atomic(f.raw), OK);
            let persisted = (&*f.raw).owner.engine.begin_read();
            assert_eq!(&persisted.get(NAME, &META).unwrap().unwrap()[..8], MAGIC);
            assert_eq!(
                persisted.get(NAME, &page_key(0)).unwrap().unwrap().len(),
                BLOCK
            );
            assert!(persisted.get(NAME, &page_key(1)).unwrap().unwrap().len() < BLOCK);
            drop(persisted);
            assert_eq!(upg_unlock(f.raw, 0), OK);
            assert_eq!(upg_close(f.raw), OK);
            f.raw = ptr::null_mut();
            let name = CString::new(f.path.as_os_str().as_encoded_bytes()).unwrap();
            assert_eq!(upg_open(name.as_ptr(), 2, &mut f.raw), OK);
            let mut bytes = vec![0; BLOCK * 2];
            assert_eq!(read(f.raw, 0, &mut bytes), OK);
            assert_eq!(&bytes[..BLOCK], &raw_page);
            let mut changed = legacy_page;
            changed[3..6].copy_from_slice(b"new");
            assert_eq!(&bytes[BLOCK..], &changed);
        }
    }
    #[test]
    fn page_corrupt_codec_record_is_not_returned_as_short_raw_data() {
        let f = Fixture::new();
        unsafe {
            let mut writer = (&*f.raw).owner.engine.begin_write().unwrap();
            writer.put(NAME, &META, &metadata(BLOCK as u64)).unwrap();
            writer
                .put(NAME, &page_key(0), b"not a raw or compressed page")
                .unwrap();
            writer.commit().unwrap();
            assert_eq!(upg_lock(f.raw, SHARED), OK);
            let mut bytes = [0; BLOCK];
            assert_eq!(read(f.raw, 0, &mut bytes), CORRUPT);
            let mut partial = [0; 17];
            assert_eq!(read(f.raw, 3, &mut partial), CORRUPT);
        }
    }
    #[test]
    fn page_lock_conflicts_and_snapshot_renewal() {
        let f = Fixture::new();
        let peer = f.other();
        unsafe {
            assert_eq!(upg_lock(f.raw, SHARED), OK);
            assert_eq!(upg_lock(peer, SHARED), OK);
            assert_eq!(upg_lock(f.raw, RESERVED), OK);
            assert_eq!(upg_lock(peer, RESERVED), BUSY);
            assert_eq!(upg_lock(f.raw, EXCLUSIVE), BUSY);
            let newcomer = f.other();
            assert_eq!(upg_lock(newcomer, SHARED), BUSY);
            let mut level = 0;
            assert_eq!(upg_lockstate(f.raw, &mut level), OK);
            assert_eq!(level, PENDING);
            assert_eq!(upg_close(newcomer), OK);
            assert_eq!(upg_unlock(peer, 0), OK);
            assert_eq!(upg_lock(f.raw, EXCLUSIVE), OK);
            assert_eq!(write(f.raw, 0, b"first"), OK);
            assert_eq!(upg_sync(f.raw), OK);
            assert_eq!(upg_unlock(f.raw, 0), OK);
            assert_eq!(upg_lock(peer, SHARED), OK);
            let mut bytes = [0; 5];
            assert_eq!(read(peer, 0, &mut bytes), OK);
            assert_eq!(&bytes, b"first");
            assert_eq!(upg_lock(f.raw, EXCLUSIVE), BUSY);
            assert_eq!(upg_unlock(peer, 0), OK);
            assert_eq!(upg_lock(f.raw, EXCLUSIVE), OK);
            assert_eq!(upg_begin_atomic(f.raw), OK);
            assert_eq!(write(f.raw, 0, b"later"), OK);
            assert_eq!(upg_commit_atomic(f.raw), OK);
            assert_eq!(upg_unlock(f.raw, 0), OK);
            assert_eq!(upg_lock(peer, SHARED), OK);
            assert_eq!(read(peer, 0, &mut bytes), OK);
            assert_eq!(&bytes, b"later");
            assert_eq!(upg_close(peer), OK);
        }
    }
    #[test]
    fn page_capacity_refusal_and_bad_namespace_leave_data_unchanged() {
        let f = Fixture::new();
        assert_eq!(
            std::fs::metadata(&f.path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        unsafe {
            assert_eq!(upg_lock(f.raw, EXCLUSIVE), OK);
            assert_eq!(upg_begin_atomic(f.raw), OK);
            assert_eq!(write(f.raw, 0, b"keep"), OK);
            assert_eq!(upg_commit_atomic(f.raw), OK);
            assert_eq!(upg_begin_atomic(f.raw), OK);
            assert_eq!(write(f.raw, LIMIT as i64, b"refused"), FULL);
            assert_eq!(upg_rollback_atomic(f.raw), OK);
            let mut bytes = [0; 4];
            assert_eq!(read(f.raw, 0, &mut bytes), OK);
            assert_eq!(&bytes, b"keep");
            let other = f.path.with_extension("ordinary");
            std::fs::write(&other, b"ordinary SQLite bytes must survive").unwrap();
            let name = CString::new(other.as_os_str().as_encoded_bytes()).unwrap();
            let mut refused = ptr::null_mut();
            assert_eq!(upg_open(name.as_ptr(), 6, &mut refused), CANTOPEN);
            assert!(refused.is_null());
            assert_eq!(
                std::fs::read(&other).unwrap(),
                b"ordinary SQLite bytes must survive"
            );
            std::fs::remove_file(other).unwrap();
            let unsafe_dir = f.path.with_extension("public-directory");
            std::fs::DirBuilder::new()
                .mode(0o755)
                .create(&unsafe_dir)
                .unwrap();
            std::fs::set_permissions(&unsafe_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            let name = CString::new(unsafe_dir.as_os_str().as_encoded_bytes()).unwrap();
            let mut refused = ptr::null_mut();
            assert_eq!(upg_open(name.as_ptr(), 6, &mut refused), CANTOPEN);
            assert!(refused.is_null());
            assert!(std::fs::read_dir(&unsafe_dir).unwrap().next().is_none());
            std::fs::remove_dir(unsafe_dir).unwrap();
        }
    }
    #[test]
    fn page_handles_are_send_and_fork_fails_before_locks_or_mutation() {
        fn send<T: Send + Sync>() {}
        send::<UpgFile>();
        let f = Fixture::new();
        unsafe {
            assert_eq!(upg_lock(f.raw, EXCLUSIVE), OK);
            assert_eq!(write(f.raw, 0, b"parent"), OK);
            assert_eq!(upg_sync(f.raw), OK);
            for raw_fork in [false, true] {
                let child = if raw_fork {
                    libc::syscall(libc::SYS_fork) as libc::pid_t
                } else {
                    libc::fork()
                };
                assert!(child >= 0);
                if child == 0 {
                    let mut out = [0; 6];
                    let failed = read(f.raw, 0, &mut out) == MISUSE && upg_close(f.raw) == MISUSE;
                    libc::_exit(if failed { 0 } else { 1 });
                }
                let mut status = 0;
                assert_eq!(libc::waitpid(child, &mut status, 0), child);
                assert_eq!(status, 0);
            }
            let mut bytes = [0; 6];
            assert_eq!(read(f.raw, 0, &mut bytes), OK);
            assert_eq!(&bytes, b"parent");
        }
    }
    #[test]
    fn page_close_releases_native_owner_after_lock_poison() {
        let mut f = Fixture::new();
        let owner = unsafe { (&*f.raw).owner.clone() };
        let thread_owner = owner.clone();
        assert!(
            std::thread::spawn(move || {
                let _lock = thread_owner.locks.lock().unwrap();
                panic!("test lock poison");
            })
            .join()
            .is_err()
        );
        unsafe {
            assert_eq!(upg_close(f.raw), OK);
        }
        f.raw = ptr::null_mut();
        drop(owner);
        let engine = native::Environment::open(&f.path, options()).unwrap();
        assert_eq!(read_length(&engine.begin_read()).unwrap(), 0);
        drop(engine);
    }
}
