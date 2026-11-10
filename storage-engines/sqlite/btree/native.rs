//! Private process-owned B-tree profile. One real snapshot owns every read
//! slice; actual SQL mutations and undo are bounded. Native writers exist
//! only inside final commit. This module does not modify the public C SDK.
use super::{current_thread, process_id};
use self_cell::self_cell;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::{CStr, c_char, c_void},
    io::{Read, Write},
    ops::Bound,
    path::{Path, PathBuf},
    ptr,
    rc::Rc,
    sync::{Arc, Mutex, OnceLock, Weak, atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering}},
    thread::ThreadId,
};
use ulmdb::{DatabaseKind, Error, snapshot_log as native};

const NOTFOUND: i32 = -30798;
const KEYEXIST: i32 = -30799;
const BAD_TXN: i32 = -30782;
const BAD_DBI: i32 = -30780;
const READERS_FULL: i32 = -30790;
const MAP_FULL: i32 = -30792;
const TXN_FULL: i32 = -30788;
const INCOMPATIBLE: i32 = -30784;
const NOTADB: i32 = -30793;
const INVALID: i32 = 22;
const IO: i32 = 5;
const BUSY: i32 = 16;
const RDONLY: u32 = 0x20000;
const NOTLS: u32 = 0x200000;
const CREATE: u32 = 0x40000;
#[cfg(not(feature = "btree-blocks"))]
const DATA: &[u8] = b"sqlite_btree_native_v1";
#[cfg(feature = "btree-blocks")]
const DATA: &[u8] = b"sqlite_btree_blocks_v1";
const MARKER: &str = "ULMDB-SQLITE-BTREE";
#[cfg(not(feature = "btree-blocks"))]
const FORMAT: &[u8] = b"ULMDB-SQLITE-NATIVE-BTREE-01\n";
#[cfg(feature = "btree-blocks")]
const FORMAT: &[u8] = b"ULMDB-SQLITE-NATIVE-BLOCKS-01\n";
#[cfg(feature = "btree-blocks")]
#[path = "blocks.rs"]
mod blocks;
const MAX_KEYS: usize = 65_536;
const MAX_BYTES: usize = 64 << 20;
const MAX_ROW: usize = 16 << 20;
const MAX_DEPTH: usize = 1024;
const CHARGE: usize = 256;
const CATALOG: u64 = u64::MAX;
type Key = [u8; 16];
type Result<T> = std::result::Result<T, i32>;
type SharedRoot = Rc<RefCell<Root>>;

#[repr(C)]
pub struct Val { size: usize, data: *mut c_void }
#[repr(C)]
pub struct Stat {
    page_size: u32, depth: u32, branch: usize, leaf: usize,
    overflow: usize, entries: usize,
}
#[repr(C)]
pub struct Info {
    address: *mut c_void, map_size: usize, last_page: usize,
    sequence: usize, max_readers: u32, readers: u32,
}
pub struct Env { creator: u32, owner: Option<Arc<Owner>>, flags: u32 }
struct Owner {
    engine: native::Environment,
    writing: AtomicBool,
    readers: AtomicUsize,
    creator: u32,
    generation: u64,
    binding: Binding,
}
#[derive(PartialEq, Eq)]
struct Binding { directory: (u64, u64), journal: (u64, u64), marker: (u64, u64) }
static REGISTRY: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Owner>>>> = OnceLock::new();
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
#[cfg(feature = "btree-read-hash")]
#[path = "read_hash.rs"]
mod read_hash;
#[cfg(feature = "btree-read-hash")]
type ReadLookup<'a> = read_hash::Lookup<'a>;
#[cfg(feature = "btree-read-hash")]
self_cell!(
    struct EpochRead {
        owner: native::ReadTxn,
        #[not_covariant]
        dependent: ReadLookup,
    }
);
#[cfg(feature = "btree-read-hash")]
impl EpochRead {
    fn get(&self, database: &[u8], key: &[u8]) -> ulmdb::Result<Option<&[u8]>> { self.borrow_owner().get(database, key) }
    fn kv(&self, database: &[u8]) -> ulmdb::Result<native::KvRead<'_>> { self.borrow_owner().kv(database) }
    fn sequence(&self) -> u64 { self.borrow_owner().sequence() }
}
#[cfg(not(feature = "btree-read-hash"))]
type EpochRead = native::ReadTxn;
fn epoch_read(read: native::ReadTxn) -> Result<EpochRead> {
    #[cfg(feature = "btree-read-hash")]
    { EpochRead::try_new(read, |read| read_hash::Lookup::new(read)) }
    #[cfg(not(feature = "btree-read-hash"))]
    { Ok(read) }
}
struct Epoch {
    read: EpochRead, owner: Arc<Owner>, _ticket: ReaderTicket,
    // Active handles are thread-bound. Only successful catalog admissions
    // belong here, and only for this actual immutable read snapshot.
    admitted: RefCell<Vec<u32>>,
    #[cfg(test)]
    catalog_checks: std::cell::Cell<usize>,
}
struct ReaderTicket(Arc<Owner>);
impl Drop for ReaderTicket {
    fn drop(&mut self) { self.0.readers.fetch_sub(1, Ordering::Release); }
}
fn new_epoch(owner: &Arc<Owner>) -> Result<Arc<Epoch>> {
    owner.readers.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| (count < 254).then_some(count + 1)).map_err(|_| READERS_FULL)?;
    let ticket = ReaderTicket(owner.clone());
    Ok(Arc::new(Epoch { read: epoch_read(owner.engine.begin_read())?, owner: owner.clone(), _ticket: ticket,
        admitted: RefCell::new(Vec::new()),
        #[cfg(test)]
        catalog_checks: std::cell::Cell::new(0),
    }))
}
struct Reservation(Arc<Owner>);
impl Drop for Reservation {
    fn drop(&mut self) { self.0.writing.store(false, Ordering::Release); }
}
struct Scope { id: u64, undo: usize }
struct Undo { key: Key, previous: Option<Option<Vec<u8>>> }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tail { Empty, Maximum(Key) }
struct DropScope<'a> { root: &'a mut Root, scope: u64, finished: bool }
impl Drop for DropScope<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.root.admitted.get_mut().clear();
            self.root.tails.get_mut().clear();
            // Poison remains set. Removing this internal marker allocates
            // nothing; the real child/outer abort restores/drops its undo.
            if self.root.scopes.last().is_some_and(|scope| scope.id == self.scope) {
                self.root.scopes.pop();
            }
        }
    }
}
struct Root {
    base: Arc<Epoch>,
    // Positive logical DBIs belong to this immutable base plus staged catalog.
    // Catalog writes and every undo restore invalidate them before mutation.
    admitted: RefCell<Vec<u32>>,
    // Only an actual LAST establishes these key-only facts. No returned
    // pointer or cursor position belongs to this bounded optional cache.
    tails: RefCell<Vec<(u32, Tail)>>,
    #[cfg(test)]
    catalog_checks: std::cell::Cell<usize>,
    #[cfg(test)]
    writer_seeks: std::cell::Cell<usize>,
    #[cfg(test)]
    admissions: std::cell::Cell<usize>,
    #[cfg(test)]
    undo_reserve: std::cell::Cell<usize>,
    pending: BTreeMap<Key, Option<Vec<u8>>>,
    undo: Vec<Undo>,
    scopes: Vec<Scope>,
    next_scope: u64,
    pending_bytes: usize,
    undo_bytes: usize,
    failed: bool,
    _reservation: Reservation,
}
// This exclusive loan keeps the admitted delta/scope unchanged while input
// bytes are validated/copied. No native writer or borrowed row is retained.
struct PreparedStage<'a> {
    root: &'a mut Root,
    key: Key,
    value_size: Option<usize>,
    pending: usize,
    undo_charge: usize,
}
enum State { Read(Option<Arc<Epoch>>), Write(SharedRoot) }
pub struct Txn {
    owner: Arc<Owner>, creator: u32, thread: ThreadId, state: State,
    scope: u64, parent: *mut Txn, child: *mut Txn,
    cursors: Vec<*mut Cursor>,
}
#[cfg(not(feature = "btree-blocks"))]
type NativeCursor<'a> = native::Cursor<'a>;
#[cfg(feature = "btree-blocks")]
type NativeCursor<'a> = RecordCursor<'a>;
self_cell!(
    struct ReadCursor {
        owner: Arc<Epoch>,
        #[covariant]
        dependent: NativeCursor,
    }
);
pub struct Cursor {
    txn: *mut Txn, creator: u32, thread: ThreadId, dbi: u32,
    last: Option<Key>, deleted: bool, cell: Option<ReadCursor>, reverse: bool,
}

fn code(error: Error) -> i32 {
    match error {
        Error::MissingDatabase => NOTFOUND,
        Error::Busy => BUSY,
        Error::LimitExceeded => MAP_FULL,
        Error::InvalidInput(_) => INVALID,
        Error::Io(error) => error.raw_os_error().unwrap_or(IO),
        _ => IO,
    }
}
fn boundary(action: impl FnOnce() -> Result<()>) -> i32 {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)) {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => error,
        Err(_) => IO,
    }
}
fn options() -> native::Options {
    native::Options { index_bytes: MAX_BYTES, journal_capacity: MAX_BYTES,
        transaction_bytes: MAX_BYTES - 4096, max_operations: MAX_KEYS,
        max_databases: 1, versioned_values: false, ..Default::default() }
}
fn physical(database: u64, key: [u8; 8]) -> Key {
    let mut out = [0; 16];
    out[..8].copy_from_slice(&database.to_be_bytes());
    out[8..].copy_from_slice(&key);
    out
}
fn catalog(dbi: u32) -> Key { physical(CATALOG, (dbi as u64).to_be_bytes()) }
fn format_key() -> Key { physical(CATALOG, u64::MAX.to_be_bytes()) }
fn lower(dbi: u32) -> Key { physical(dbi as u64, [0; 8]) }
fn upper(dbi: u32) -> Key { physical(dbi as u64 + 1, [0; 8]) }
fn checkpoint(epoch: &Epoch) -> [u8; 64] {
    let mut out = [0; 64];
    #[cfg(not(feature = "btree-blocks"))]
    out[..16].copy_from_slice(b"USQLBT-NATIVE-01");
    #[cfg(feature = "btree-blocks")]
    out[..16].copy_from_slice(b"USQLBT-BLOCKS-01");
    out[16..24].copy_from_slice(&epoch.owner.generation.to_be_bytes());
    out[24..32].copy_from_slice(&epoch.read.sequence().to_be_bytes());
    out[32..40].copy_from_slice(&epoch.owner.binding.directory.0.to_be_bytes());
    out[40..48].copy_from_slice(&epoch.owner.binding.directory.1.to_be_bytes());
    out[48..56].copy_from_slice(&epoch.owner.binding.journal.1.to_be_bytes());
    out[56..60].copy_from_slice(&epoch.owner.creator.to_be_bytes());
    out
}
fn binding(path: &Path) -> Result<Binding> {
    use std::os::unix::fs::MetadataExt;
    let directory = std::fs::symlink_metadata(path).map_err(|e| e.raw_os_error().unwrap_or(IO))?;
    let journal = std::fs::symlink_metadata(path.join("SNAPSHOTLOG")).map_err(|e| e.raw_os_error().unwrap_or(IO))?;
    let marker = std::fs::symlink_metadata(path.join(MARKER)).map_err(|e| e.raw_os_error().unwrap_or(IO))?;
    if !directory.is_dir() || directory.file_type().is_symlink()
        || directory.mode() & 0o077 != 0 || directory.uid() != unsafe { libc::geteuid() }
        || !journal.is_file() || journal.file_type().is_symlink() || journal.nlink() != 1
        || !marker.is_file() || marker.file_type().is_symlink() || marker.nlink() != 1
        || journal.mode() & 0o077 != 0 || marker.mode() & 0o077 != 0 {
        return Err(INVALID);
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path.join(MARKER)).and_then(|file| file.take((FORMAT.len() + 1) as u64).read_to_end(&mut bytes))
        .map_err(|e| e.raw_os_error().unwrap_or(IO))?;
    if bytes != FORMAT { return Err(NOTADB); }
    Ok(Binding { directory: (directory.dev(), directory.ino()), journal: (journal.dev(), journal.ino()), marker: (marker.dev(), marker.ino()) })
}
fn validate_marker(path: &Path) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let marker = std::fs::symlink_metadata(path.join(MARKER)).map_err(|e| e.raw_os_error().unwrap_or(IO))?;
    if !marker.is_file() || marker.file_type().is_symlink() || marker.nlink() != 1
        || marker.uid() != unsafe { libc::geteuid() } || marker.mode() & 0o077 != 0 { return Err(NOTADB); }
    let mut bytes = Vec::new();
    std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path.join(MARKER)).and_then(|file| file.take((FORMAT.len() + 1) as u64).read_to_end(&mut bytes))
        .map_err(|e| e.raw_os_error().unwrap_or(IO))?;
    if bytes != FORMAT { return Err(NOTADB); } Ok(())
}
fn open_owner(path: &Path, readonly: bool) -> Result<Arc<Owner>> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, MetadataExt};
    let creator = process_id();
    if creator == 0 || !path.is_absolute() { return Err(BAD_TXN); }
    super::check_registry_process()?;
    if readonly && (!path.is_dir() || !path.join(MARKER).is_file()
                    || !path.join("SNAPSHOTLOG").is_file()) { return Err(libc::ENOENT); }
    let created = !path.exists();
    if created { std::fs::DirBuilder::new().mode(0o700).create(path).map_err(|e| e.raw_os_error().unwrap_or(IO))?; }
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.raw_os_error().unwrap_or(IO))?;
    if !meta.is_dir() || meta.file_type().is_symlink() || meta.mode() & 0o077 != 0
        || meta.uid() != unsafe { libc::geteuid() } { return Err(INVALID); }
    let path = path.canonicalize().map_err(|e| e.raw_os_error().unwrap_or(IO))?;
    let mut owners = REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new())).lock().map_err(|_| IO)?;
    owners.retain(|_, owner| owner.strong_count() > 0);
    if let Some(owner) = owners.get(&path).and_then(Weak::upgrade) {
        if owner.creator != creator || binding(&path)? != owner.binding { return Err(INVALID); }
        return Ok(owner);
    }
    let exists = path.join("SNAPSHOTLOG").exists();
    if !path.join(MARKER).exists() {
        if std::fs::read_dir(&path).map_err(|e| e.raw_os_error().unwrap_or(IO))?.next().is_some() { return Err(NOTADB); }
        let mut marker = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(path.join(MARKER)).map_err(|e| e.raw_os_error().unwrap_or(IO))?;
        marker.write_all(FORMAT).and_then(|_| marker.sync_all()).map_err(|e| e.raw_os_error().unwrap_or(IO))?;
        std::fs::File::open(&path).and_then(|file| file.sync_all()).map_err(|e| e.raw_os_error().unwrap_or(IO))?;
    }
    validate_marker(&path)?; // Admit format before opening/creating the engine.
    if path.join("CURRENT").exists() || path.join("RESIDENT.wal").exists() { return Err(NOTADB); }
    let engine = if exists { native::Environment::open(&path, options()) } else { native::Environment::create(&path, options()) }.map_err(code)?;
    if exists {
        let read = engine.begin_read();
        if read.get(DATA, &format_key()).map_err(code)? != Some(FORMAT) { return Err(NOTADB); }
    } else {
        let mut write = engine.begin_write().map_err(code)?;
        write.create_database(DATA, DatabaseKind::Kv).map_err(code)?;
        write.put(DATA, &format_key(), FORMAT).map_err(code)?;
        write.commit().map_err(code)?;
    }
    if created { std::fs::File::open(path.parent().ok_or(INVALID)?).and_then(|file| file.sync_all()).map_err(|e| e.raw_os_error().unwrap_or(IO))?; }
    let generation = NEXT_OWNER.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1)).map_err(|_| IO)?;
    let owner = Arc::new(Owner { engine, writing: AtomicBool::new(false), readers: AtomicUsize::new(0), creator, generation, binding: binding(&path)? });
    owners.insert(path, Arc::downgrade(&owner));
    Ok(owner)
}
unsafe fn env_ref<'a>(env: *mut Env) -> Result<&'a Env> {
    let env = unsafe { env.as_ref() }.ok_or(INVALID)?;
    if env.creator == 0 || env.creator != process_id() { return Err(BAD_TXN); }
    Ok(env)
}
unsafe fn txn_ref<'a>(txn: *mut Txn, top: bool) -> Result<&'a Txn> {
    let txn = unsafe { txn.as_ref() }.ok_or(INVALID)?;
    if txn.creator != process_id() || txn.thread != current_thread() { return Err(BAD_TXN); }
    if matches!(txn.state, State::Read(None)) || (top && !txn.child.is_null()) { return Err(BAD_TXN); }
    if top && let State::Write(root) = &txn.state {
        let root = root.borrow();
        if root.failed || root.scopes.last().map(|s| s.id) != Some(txn.scope) { return Err(BAD_TXN); }
    }
    Ok(txn)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_txn_validate(txn: *mut Txn) -> i32 {
    boundary(|| unsafe { txn_ref(txn, true).map(|_| ()) })
}
#[unsafe(no_mangle)]
pub extern "C" fn ubt_process_id_fast() -> u32 { process_id() }
unsafe fn bytes<'a>(value: *const Val) -> Result<&'a [u8]> {
    let value = unsafe { value.as_ref() }.ok_or(INVALID)?;
    if value.size > isize::MAX as usize || (value.size != 0 && value.data.is_null()) { return Err(INVALID); }
    Ok(if value.size == 0 { &[] } else { unsafe { std::slice::from_raw_parts(value.data.cast(), value.size) } })
}
unsafe fn key(dbi: u32, value: *const Val) -> Result<Key> {
    let bytes = unsafe { bytes(value)? };
    let mut suffix = [0; 8];
    if dbi == 0 && bytes.len() == 1 { suffix[7] = bytes[0]; }
    else if dbi != 0 && bytes.len() == 8 { suffix.copy_from_slice(bytes); }
    else { return Err(INVALID); }
    Ok(physical(dbi as u64, suffix))
}
unsafe fn set(out: *mut Val, value: &[u8]) -> Result<()> {
    let out = unsafe { out.as_mut() }.ok_or(INVALID)?;
    out.size = value.len(); out.data = value.as_ptr().cast_mut().cast(); Ok(())
}
fn copy_value(value: &[u8]) -> Result<Vec<u8>> {
    let mut owned = Vec::new();
    owned.try_reserve_exact(value.len()).map_err(|_| TXN_FULL)?;
    owned.extend_from_slice(value);
    Ok(owned)
}
impl Root {
    fn tail(&self, dbi: u32) -> Option<Tail> {
        let tails = self.tails.borrow();
        tails.binary_search_by_key(&dbi, |entry| entry.0).ok().map(|at| tails[at].1)
    }
    fn take_tail(&self, dbi: u32) -> Option<Tail> {
        let mut tails = self.tails.borrow_mut();
        tails.binary_search_by_key(&dbi, |entry| entry.0).ok().map(|at| tails.remove(at).1)
    }
    fn remember_tail(&self, dbi: u32, tail: Tail) {
        if dbi == 0 { return; }
        let mut tails = self.tails.borrow_mut();
        match tails.binary_search_by_key(&dbi, |entry| entry.0) {
            Ok(at) => tails[at].1 = tail,
            Err(at) if tails.len() < MAX_DEPTH && tails.try_reserve(1).is_ok() => tails.insert(at, (dbi, tail)),
            Err(_) => {}, // Allocation/cap refusal keeps the actual seek path.
        }
    }
    fn lookup(&self, key: &Key) -> Result<Option<&[u8]>> {
        if let Some(value) = self.pending.get(key) { return Ok(value.as_deref()); }
        #[cfg(not(feature = "btree-blocks"))]
        { self.base.read.get(DATA, key).map_err(code) }
        #[cfg(feature = "btree-blocks")]
        { base_value(&self.base, key) }
    }
    fn admit(&self, key: &Key, value_size: Option<usize>) -> Result<(usize, usize)> {
        #[cfg(test)]
        self.admissions.set(self.admissions.get() + 1);
        if value_size.is_some_and(|size| size > MAX_ROW) { return Err(INVALID); }
        let old = self.pending.get(key);
        if old.is_none() && self.pending.len() >= MAX_KEYS { return Err(TXN_FULL); }
        let charge = |v: &Option<Vec<u8>>| CHARGE + v.as_ref().map_or(0, Vec::len);
        let pending = self.pending_bytes - old.map_or(0, charge) + CHARGE + value_size.unwrap_or(0);
        let undo_charge = if self.scopes.len() > 1 { CHARGE + old.and_then(Option::as_ref).map_or(0, Vec::len) } else { 0 };
        if pending.checked_add(self.undo_bytes).and_then(|n| n.checked_add(undo_charge)).is_none_or(|n| n > MAX_BYTES) { return Err(TXN_FULL); }
        // With no undo scope, the old value still exists while its replacement
        // is copied. Admit that physical peak before allocating the new Vec.
        if self.scopes.len() == 1 && value_size.is_some()
            && self.pending_bytes.checked_add(self.undo_bytes).and_then(|n| n.checked_add(value_size.unwrap())).is_none_or(|n| n > MAX_BYTES) { return Err(TXN_FULL); }
        Ok((pending, undo_charge))
    }
    fn prepare_stage(&mut self, key: Key, value_size: Option<usize>) -> Result<PreparedStage<'_>> {
        let (pending, undo_charge) = self.admit(&key, value_size)?;
        Ok(PreparedStage { root: self, key, value_size, pending, undo_charge })
    }
    fn stage(&mut self, key: Key, value: Option<Vec<u8>>) -> Result<()> {
        self.prepare_stage(key, value.as_ref().map(Vec::len))?.apply(value)
    }
    fn put(&mut self, key: Key, value: &[u8]) -> Result<()> {
        let prepared = self.prepare_stage(key, Some(value.len()))?;
        prepared.apply(Some(copy_value(value)?))
    }
    fn restore(&mut self, marker: usize) {
        self.admitted.get_mut().clear();
        self.tails.get_mut().clear();
        while self.undo.len() > marker {
            let undo = self.undo.pop().unwrap();
            self.undo_bytes -= CHARGE + undo.previous.as_ref().and_then(Option::as_ref).map_or(0, Vec::len);
            if let Some(value) = self.pending.remove(&undo.key) { self.pending_bytes -= CHARGE + value.as_ref().map_or(0, Vec::len); }
            if let Some(value) = undo.previous {
                self.pending_bytes += CHARGE + value.as_ref().map_or(0, Vec::len);
                self.pending.insert(undo.key, value);
            }
        }
    }
    fn commit(&self) -> Result<()> {
        #[cfg(feature = "btree-blocks")]
        { return self.commit_blocks(); }
        #[cfg(not(feature = "btree-blocks"))]
        {
        let mut write = self.base.owner.engine.begin_write().map_err(code)?;
        for (key, value) in &self.pending {
            match value { Some(value) => write.put(DATA, key, value).map_err(code)?, None => write.delete(DATA, key).map_err(code)? }
        }
        write.commit().map_err(code)?; Ok(())
        }
    }
}
impl PreparedStage<'_> {
    fn apply(self, value: Option<Vec<u8>>) -> Result<()> {
        if value.as_ref().map(Vec::len) != self.value_size { return Err(INVALID); }
        let PreparedStage { root, key, pending, undo_charge, .. } = self;
        if root.scopes.len() > 1 {
            #[cfg(not(test))]
            let additional = 1;
            #[cfg(test)]
            let additional = root.undo_reserve.get();
            root.undo.try_reserve(additional).map_err(|_| TXN_FULL)?;
        }
        let prior_tail = if key[..8] == CATALOG.to_be_bytes() {
            root.admitted.get_mut().clear();
            root.tails.get_mut().clear();
            None
        } else {
            let dbi = u64::from_be_bytes(key[..8].try_into().unwrap());
            u32::try_from(dbi).ok().filter(|dbi| *dbi != 0).map(|dbi| (dbi, root.take_tail(dbi)))
        };
        // Forget a fact BEFORE the potentially allocating mutation. A failed
        // or unwinding stage cannot leave an old maximum available for reuse.
        let inserting = value.is_some();
        let previous = root.pending.insert(key, value);
        if root.scopes.len() > 1 { root.undo.push(Undo { key, previous }); root.undo_bytes += undo_charge; }
        root.pending_bytes = pending;
        if let Some((dbi, Some(tail))) = prior_tail {
            let next = match (tail, inserting) {
                (Tail::Empty, true) => Some(Tail::Maximum(key)),
                (Tail::Maximum(old), true) => Some(Tail::Maximum(old.max(key))),
                (Tail::Empty, false) => Some(Tail::Empty),
                (Tail::Maximum(old), false) if old != key => Some(Tail::Maximum(old)),
                _ => None, // Deleting the maximum requires another real LAST.
            };
            if let Some(next) = next { root.remember_tail(dbi, next); }
        }
        Ok(())
    }
}
fn exists(txn: &Txn, dbi: u32) -> Result<bool> {
    let valid = |value: Option<&[u8]>| match value { None => Ok(false), Some([0]) => Ok(true), _ => Err(NOTADB) };
    match &txn.state {
        State::Read(Some(epoch)) => {
            if epoch.admitted.borrow().binary_search(&dbi).is_ok() { return Ok(true); }
            #[cfg(test)]
            epoch.catalog_checks.set(epoch.catalog_checks.get() + 1);
            let found = valid(epoch.read.get(DATA, &catalog(dbi)).map_err(code)?)?;
            if found {
                let mut admitted = epoch.admitted.borrow_mut();
                // Admission is an optional bounded accelerator. Exhaustion
                // falls back to the real catalog check, never an error or a
                // fabricated capability. Negative/malformed results stay out.
                if admitted.len() < MAX_DEPTH && admitted.try_reserve(1).is_ok() {
                    let at = admitted.binary_search(&dbi).unwrap_err();
                    admitted.insert(at, dbi);
                }
            }
            Ok(found)
        }
        State::Read(None) => Err(BAD_TXN),
        State::Write(root) => {
            let root = root.borrow();
            if root.admitted.borrow().binary_search(&dbi).is_ok() { return Ok(true); }
            #[cfg(test)]
            root.catalog_checks.set(root.catalog_checks.get() + 1);
            let found = valid(root.lookup(&catalog(dbi))?)?;
            if found {
                let mut admitted = root.admitted.borrow_mut();
                if admitted.len() < MAX_DEPTH && admitted.try_reserve(1).is_ok() {
                    let at = admitted.binary_search(&dbi).unwrap_err();
                    admitted.insert(at, dbi);
                }
            }
            Ok(found)
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_env_create(out: *mut *mut Env) -> i32 {
    boundary(|| unsafe {
        let out = out.as_mut().ok_or(INVALID)?;
        super::check_registry_process()?;
        let creator = process_id();
        if creator == 0 { return Err(BAD_TXN); }
        *out = Box::into_raw(Box::new(Env { creator, owner: None, flags: 0 })); Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_env_open(env: *mut Env, path: *const c_char, flags: u32, _mode: u32) -> i32 {
    boundary(|| unsafe {
        env_ref(env)?;
        if path.is_null() || (*env).owner.is_some() || flags & !(RDONLY | NOTLS) != 0 { return Err(INVALID); }
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(std::ffi::OsStr::from_bytes(CStr::from_ptr(path).to_bytes()));
        super::check_registry_process()?;
        (*env).owner = Some(open_owner(path, flags & RDONLY != 0)?); (*env).flags = flags; Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_env_close(env: *mut Env) {
    let _ = boundary(|| unsafe { env_ref(env)?; drop(Box::from_raw(env)); Ok(()) });
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_env_set_maxdbs(env: *mut Env, count: u32) -> i32 {
    boundary(|| unsafe { let env = env_ref(env)?; if count == 0 || env.owner.is_some() { Err(INVALID) } else { Ok(()) } })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_env_set_maxreaders(env: *mut Env, count: u32) -> i32 {
    unsafe { ubt_env_set_maxdbs(env, count) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_env_set_mapsize(env: *mut Env, size: usize) -> i32 {
    boundary(|| unsafe { env_ref(env)?; if size > MAX_BYTES { Err(INVALID) } else { Ok(()) } })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_env_set_flags(env: *mut Env, flags: u32, enabled: i32) -> i32 {
    // This profile always uses native FULL. It only accepts clearing relax flags.
    boundary(|| unsafe { env_ref(env)?; if enabled != 0 || flags & !(0x10000 | 0x40000) != 0 { Err(INVALID) } else { Ok(()) } })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_env_get_maxkeysize(env: *mut Env) -> i32 {
    if unsafe { env_ref(env) }.is_ok() { 8 } else { 0 }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_env_stat(env: *mut Env, out: *mut Stat) -> i32 {
    boundary(|| unsafe {
        let env = env_ref(env)?; let owner = env.owner.as_ref().ok_or(INVALID)?;
        let out = out.as_mut().ok_or(INVALID)?;
        let read = owner.engine.begin_read();
        #[cfg(not(feature = "btree-blocks"))]
        let count = read.kv(DATA).map_err(code)?.cursor(Bound::Unbounded, Bound::Unbounded, false).map_err(code)?
            .try_fold(0usize, |count, row| { row.map_err(code)?; count.checked_add(1).ok_or(MAP_FULL) })?;
        #[cfg(feature = "btree-blocks")]
        let count = block_env_count(&read)?;
        *out = Stat { page_size: 4096, depth: 1, branch: 0, leaf: count.div_ceil(32), overflow: 0, entries: count }; Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_env_info(env: *mut Env, out: *mut Info) -> i32 {
    boundary(|| unsafe {
        let env = env_ref(env)?; let owner = env.owner.as_ref().ok_or(INVALID)?;
        let out = out.as_mut().ok_or(INVALID)?;
        *out = Info { address: ptr::null_mut(), map_size: MAX_BYTES,
            last_page: MAX_BYTES / 4096, sequence: usize::try_from(owner.engine.begin_read().sequence()).map_err(|_| IO)?,
            max_readers: 254, readers: owner.readers.load(Ordering::Acquire) as u32 }; Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_txn_begin(env: *mut Env, parent: *mut Txn, flags: u32, out: *mut *mut Txn) -> i32 {
    boundary(|| unsafe {
        let env = env_ref(env)?; let owner = env.owner.as_ref().ok_or(INVALID)?;
        let out = out.as_mut().ok_or(INVALID)?;
        if flags & !RDONLY != 0 { return Err(INVALID); }
        let (state, scope) = if !parent.is_null() {
            let parent = txn_ref(parent, true)?;
            if flags != 0 || !Arc::ptr_eq(owner, &parent.owner) { return Err(BAD_TXN); }
            let State::Write(root) = &parent.state else { return Err(BAD_TXN); };
            let mut shared = root.borrow_mut();
            if shared.scopes.len() >= MAX_DEPTH { return Err(TXN_FULL); }
            let scope = shared.next_scope;
            shared.next_scope = scope.checked_add(1).ok_or(TXN_FULL)?;
            let undo = shared.undo.len(); shared.scopes.push(Scope { id: scope, undo });
            (State::Write(root.clone()), scope)
        } else if flags == RDONLY {
            (State::Read(Some(new_epoch(owner)?)), 0)
        } else {
            if env.flags & RDONLY != 0 { return Err(libc::EACCES); }
            owner.writing.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).map_err(|_| BUSY)?;
            let reservation = Reservation(owner.clone());
            let base = new_epoch(owner)?;
            (State::Write(Rc::new(RefCell::new(Root { base, admitted: RefCell::new(Vec::new()), tails: RefCell::new(Vec::new()),
                #[cfg(test)]
                catalog_checks: std::cell::Cell::new(0),
                #[cfg(test)]
                writer_seeks: std::cell::Cell::new(0),
                #[cfg(test)]
                admissions: std::cell::Cell::new(0),
                #[cfg(test)]
                undo_reserve: std::cell::Cell::new(1),
                pending: BTreeMap::new(), undo: Vec::new(),
                scopes: vec![Scope { id: 0, undo: 0 }], next_scope: 1,
                pending_bytes: 0, undo_bytes: 0, failed: false, _reservation: reservation }))), 0)
        };
        let pointer = Box::into_raw(Box::new(Txn { owner: owner.clone(), creator: env.creator,
            thread: current_thread(), state, scope, parent, child: ptr::null_mut(), cursors: Vec::new() }));
        if !parent.is_null() { (*parent).child = pointer; }
        *out = pointer; Ok(())
    })
}
unsafe fn invalidate(txn: &mut Txn) {
    for pointer in txn.cursors.drain(..) {
        let cursor = unsafe { &mut *pointer };
        // Drop every dependent cell BEFORE the transaction's actual Epoch.
        cursor.cell.take(); cursor.txn = ptr::null_mut(); cursor.last = None;
    }
}
unsafe fn abort(txn: *mut Txn) -> Result<()> {
    unsafe {
        let value = txn.as_ref().ok_or(INVALID)?;
        if value.creator != process_id() || value.thread != current_thread() { return Err(BAD_TXN); }
        if !value.child.is_null() { abort(value.child)?; }
        if !value.parent.is_null() {
            let State::Write(root) = &value.state else { return Err(BAD_TXN); };
            if !root.borrow().scopes.iter().any(|scope| scope.id == value.scope) { return Err(BAD_TXN); }
        }
        let mut value = Box::from_raw(txn);
        // Once consumed, never leave a parent pointing at this allocation,
        // even if a later cleanup path unexpectedly returns an error/unwinds.
        if !value.parent.is_null() { (*value.parent).child = ptr::null_mut(); }
        invalidate(&mut value);
        if !value.parent.is_null() {
            let State::Write(root) = &value.state else { return Err(BAD_TXN); };
            let mut root = root.borrow_mut();
            while root.scopes.last().is_some_and(|scope| scope.id != value.scope) {
                let scope = root.scopes.pop().ok_or(BAD_TXN)?; root.restore(scope.undo);
            }
            let scope = root.scopes.pop().ok_or(BAD_TXN)?;
            root.restore(scope.undo);
        }
        Ok(())
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_txn_abort(txn: *mut Txn) {
    if !txn.is_null() { let _ = boundary(|| unsafe { abort(txn) }); }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_txn_commit(txn: *mut Txn) -> i32 {
    boundary(|| unsafe {
        txn_ref(txn, true)?;
        let mut value = Box::from_raw(txn);
        if !value.parent.is_null() { (*value.parent).child = ptr::null_mut(); }
        invalidate(&mut value);
        match &value.state {
            State::Read(_) => Ok(()),
            State::Write(root) if value.parent.is_null() => {
                let result = root.borrow().commit();
                result.map_err(|rc| if rc == BUSY || rc == libc::EAGAIN { IO } else { rc })
            }
            State::Write(root) => {
                let mut root = root.borrow_mut(); let scope = root.scopes.pop().ok_or(BAD_TXN)?;
                if scope.id != value.scope { return Err(BAD_TXN); }
                if root.scopes.len() == 1 { root.undo.clear(); root.undo_bytes = 0; }
                (*value.parent).child = ptr::null_mut(); Ok(())
            }
        }
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_txn_reset(txn: *mut Txn) {
    let _ = boundary(|| unsafe {
        txn_ref(txn, true)?;
        if !matches!((*txn).state, State::Read(Some(_))) || !(*txn).parent.is_null() { return Err(BAD_TXN); }
        invalidate(&mut *txn);
        (*txn).state = State::Read(None); Ok(())
    });
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_txn_renew(txn: *mut Txn) -> i32 {
    boundary(|| unsafe {
        let txn = txn.as_mut().ok_or(INVALID)?;
        if txn.creator != process_id() || txn.thread != current_thread()
            || !matches!(txn.state, State::Read(None)) { return Err(BAD_TXN); }
        txn.state = State::Read(Some(new_epoch(&txn.owner)?)); Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_txn_base_checkpoint(txn: *mut Txn, out: *mut u8) -> i32 {
    boundary(|| unsafe {
        let txn = txn_ref(txn, true)?;
        if out.is_null() || !txn.parent.is_null() { return Err(INVALID); }
        let bytes = match &txn.state { State::Read(Some(epoch)) => checkpoint(epoch), State::Write(root) => checkpoint(&root.borrow().base), State::Read(None) => return Err(BAD_TXN) };
        ptr::copy_nonoverlapping(bytes.as_ptr(), out, 64); Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_dbi_open(txn: *mut Txn, name: *const c_char, flags: u32, out: *mut u32) -> i32 {
    boundary(|| unsafe {
        let txn = txn_ref(txn, true)?;
        let out = out.as_mut().ok_or(INVALID)?;
        if name.is_null() || flags & !CREATE != 0 { return Err(INVALID); }
        let name = CStr::from_ptr(name).to_bytes();
        let dbi = if name == b"meta" { 0 } else {
            if name.len() != 9 || name[0] != b't' { return Err(INCOMPATIBLE); }
            let mut id = 0u32;
            for byte in &name[1..] {
                let digit = match byte { b'0'..=b'9' => byte - b'0', b'a'..=b'f' => byte - b'a' + 10, _ => return Err(INVALID) };
                id = id.checked_mul(16).and_then(|n| n.checked_add(digit as u32)).ok_or(INVALID)?;
            }
            if id == 0 { return Err(INVALID); } id
        };
        if !exists(txn, dbi)? {
            if flags & CREATE == 0 { return Err(NOTFOUND); }
            let State::Write(root) = &txn.state else { return Err(libc::EACCES); };
            root.borrow_mut().put(catalog(dbi), &[0])?;
        }
        *out = dbi; Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_get(txn: *mut Txn, dbi: u32, input: *mut Val, out: *mut Val) -> i32 {
    boundary(|| unsafe {
        let txn = txn_ref(txn, true)?;
        if !exists(txn, dbi)? { return Err(BAD_DBI); }
        if out.is_null() { return Err(INVALID); }
        let key = key(dbi, input)?;
        match &txn.state {
            State::Read(Some(epoch)) => {
                #[cfg(feature = "btree-read-hash")]
                { *out = epoch.read.with_dependent(|_read, lookup| lookup.exact(dbi, &key))?.ok_or(NOTFOUND)?; }
                #[cfg(not(feature = "btree-read-hash"))]
                { set(out, epoch.read.get(DATA, &key).map_err(code)?.ok_or(NOTFOUND)?)?; }
            }
            State::Read(None) => return Err(BAD_TXN),
            State::Write(root) => set(out, root.borrow().lookup(&key)?.ok_or(NOTFOUND)?)?,
        }
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_put(txn: *mut Txn, dbi: u32, input: *mut Val, value: *mut Val, flags: u32) -> i32 {
    boundary(|| unsafe {
        let txn = txn_ref(txn, true)?;
        if flags & !(0x10 | 0x10000 | 0x20000) != 0 { return Err(INVALID); }
        if !exists(txn, dbi)? { return Err(BAD_DBI); }
        let State::Write(root) = &txn.state else { return Err(libc::EACCES); };
        let key = key(dbi, input)?;
        let value = value.as_mut().ok_or(INVALID)?;
        let mut root = root.borrow_mut();
        if flags & 0x10 != 0 && let Some(old) = root.lookup(&key)? {
            set(value, old)?; return Err(KEYEXIST);
        }
        let prepared = root.prepare_stage(key, Some(value.size))?;
        if flags & 0x10000 != 0 {
            let mut owned = Vec::new();
            owned.try_reserve_exact(value.size).map_err(|_| TXN_FULL)?;
            owned.resize(value.size, 0);
            prepared.apply(Some(owned))?;
            let entry = root.pending.get_mut(&key).and_then(Option::as_mut).ok_or(IO)?;
            value.data = entry.as_mut_ptr().cast();
        } else {
            // Finish borrowing/copying a possibly aliased staged buffer
            // before apply can replace and release the original allocation.
            let owned = copy_value(bytes(value)?)?;
            prepared.apply(Some(owned))?;
        }
        Ok(())
    })
}

struct Row { key: Key, key_data: *const u8, data: *mut c_void, size: usize }
fn row(key: &[u8], value: &[u8]) -> Result<Row> {
    if key.len() != 16 || value.len() > MAX_ROW { return Err(IO); }
    #[cfg(not(feature = "btree-blocks"))]
    let key_data = key.as_ptr();
    #[cfg(feature = "btree-blocks")]
    let key_data = key[if key[..8] == [0;8] {15} else {8}..].as_ptr();
    Ok(Row { key: key.try_into().map_err(|_| IO)?, key_data, data: value.as_ptr().cast_mut().cast(), size: value.len() })
}
fn epoch_exact(epoch: &Epoch, wanted: &Key) -> Result<Option<Row>> {
    #[cfg(feature = "btree-blocks")]
    if let Some(dbi) = block_dbi(wanted)? { return epoch.read.with_dependent(|_, lookup| lookup.block_exact(dbi, wanted)); }
    let mut cursor = epoch.read.kv(DATA).map_err(code)?.cursor(Bound::Included(wanted), Bound::Unbounded, false).map_err(code)?;
    match cursor.next().transpose().map_err(code)? {
        Some((key, value)) if key == wanted => Ok(Some(row(key, value)?)),
        _ => Ok(None),
    }
}
fn writer_exact(root: &Root, wanted: &Key) -> Result<Option<Row>> {
    if let Some((stored_key, value)) = root.pending.get_key_value(wanted) {
        return value.as_deref().map(|value| row(stored_key, value)).transpose();
    }
    epoch_exact(&root.base, wanted)
}
fn writer_seek(root: &Root, dbi: u32, bound: Bound<Key>, reverse: bool) -> Result<Option<Row>> {
    #[cfg(test)]
    root.writer_seeks.set(root.writer_seeks.get() + 1);
    let low = if reverse { Bound::Included(lower(dbi)) } else { bound };
    let high = if reverse { bound } else { Bound::Excluded(upper(dbi)) };
    #[cfg(not(feature = "btree-blocks"))]
    let mut base = root.base.read.kv(DATA).map_err(code)?.cursor(slice_bound(&low), slice_bound(&high), reverse).map_err(code)?;
    #[cfg(feature = "btree-blocks")]
    let mut base = record_cursor(&root.base, dbi, low, high, reverse)?;
    let mut base_row = None;
    for next in &mut base {
        #[cfg(not(feature = "btree-blocks"))]
        { let (key,value)=next.map_err(code)?; let owned_key:Key=key.try_into().map_err(|_|IO)?;
          if !root.pending.contains_key(&owned_key) {base_row=Some(row(key,value)?);break;} }
        #[cfg(feature = "btree-blocks")]
        {let found=next?;if !root.pending.contains_key(&found.key){base_row=Some(found);break;}}
    }
    let mut pending = root.pending.range((low, high));
    let staged = if reverse { pending.rev().find(|(_, v)| v.is_some()) } else { pending.find(|(_, v)| v.is_some()) };
    let staged = staged.map(|(key, value)| row(key, value.as_deref().unwrap())).transpose()?;
    Ok(match (base_row, staged) {
        (Some(a), Some(b)) => Some(if (a.key < b.key) != reverse { a } else { b }),
        (one, None) | (None, one) => one,
    })
}
fn writer_last(root: &Root, dbi: u32) -> Result<Option<Row>> {
    match root.tail(dbi) {
        Some(Tail::Empty) => return Ok(None),
        Some(Tail::Maximum(key)) => {
            if let Some(found) = writer_exact(root, &key)? { return Ok(Some(found)); }
            root.take_tail(dbi); // Defensive refusal: reacquire a genuine tail.
        }
        None => {},
    }
    let found = writer_seek(root, dbi, Bound::Excluded(upper(dbi)), true)?;
    root.remember_tail(dbi, found.as_ref().map_or(Tail::Empty, |row| Tail::Maximum(row.key)));
    Ok(found)
}
fn slice_bound(bound: &Bound<Key>) -> Bound<&[u8]> {
    match bound { Bound::Included(key) => Bound::Included(key), Bound::Excluded(key) => Bound::Excluded(key), Bound::Unbounded => Bound::Unbounded }
}
fn count_rows(txn: &Txn, dbi: u32) -> Result<usize> {
    if let State::Read(Some(epoch)) = &txn.state {
        #[cfg(feature = "btree-blocks")]
        if dbi != 0 { let read=epoch.read.borrow_owner(); return epoch.read.with_dependent(|_,lookup|Ok(lookup.rows_for(read,dbi)?.audit().rows)); }
        let low = lower(dbi); let high = upper(dbi);
        return epoch.read.kv(DATA).map_err(code)?.cursor(Bound::Included(&low), Bound::Excluded(&high), false).map_err(code)?
            .try_fold(0usize, |count, row| { row.map_err(code)?; count.checked_add(1).ok_or(MAP_FULL) });
    }
    let State::Write(root) = &txn.state else { return Err(BAD_TXN); };
    let root = root.borrow();
    let mut bound = Bound::Included(lower(dbi)); let mut count = 0usize;
    while let Some(row) = writer_seek(&root, dbi, bound, false)? {
        count = count.checked_add(1).ok_or(MAP_FULL)?; bound = Bound::Excluded(row.key);
    }
    Ok(count)
}
fn collect_keys(txn: &Txn, dbi: u32) -> Result<Vec<Key>> {
    let mut keys = Vec::new();
    let mut bound = Bound::Included(lower(dbi));
    loop {
        let next = match &txn.state {
            State::Write(root) => writer_seek(&root.borrow(), dbi, bound, false)?,
            State::Read(Some(epoch)) => {
                let start = match &bound { Bound::Included(k) => Bound::Included(k.as_slice()), Bound::Excluded(k) => Bound::Excluded(k.as_slice()), Bound::Unbounded => Bound::Unbounded };
                let end = upper(dbi);
                #[cfg(not(feature = "btree-blocks"))]
                { epoch.read.kv(DATA).map_err(code)?.cursor(start, Bound::Excluded(&end), false).map_err(code)?.next().transpose().map_err(code)?
                    .map(|(key, value)| row(key, value)).transpose()? }
                #[cfg(feature = "btree-blocks")]
                { let _ = (start, end); record_cursor(epoch, dbi, bound, Bound::Excluded(upper(dbi)), false)?.next().transpose()? }
            }
            State::Read(None) => return Err(BAD_TXN),
        };
        let Some(next) = next else { return Ok(keys); };
        if keys.len() == MAX_KEYS { return Err(TXN_FULL); }
        keys.try_reserve(1).map_err(|_| TXN_FULL)?;
        keys.push(next.key); bound = Bound::Excluded(next.key);
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_stat(txn: *mut Txn, dbi: u32, out: *mut Stat) -> i32 {
    boundary(|| unsafe {
        let txn = txn_ref(txn, true)?;
        if !exists(txn, dbi)? { return Err(BAD_DBI); }
        let out = out.as_mut().ok_or(INVALID)?;
        let count = count_rows(txn, dbi)?;
        *out = Stat { page_size: 4096, depth: 1, branch: 0, leaf: count.div_ceil(32), overflow: 0, entries: count }; Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_drop(txn: *mut Txn, dbi: u32, delete: i32) -> i32 {
    boundary(|| unsafe {
        let current = txn_ref(txn, true)?;
        if delete != 0 && delete != 1 { return Err(INVALID); }
        if !exists(current, dbi)? { return Err(BAD_DBI); }
        let State::Write(root) = &current.state else { return Err(libc::EACCES); };
        if dbi == 0 { return Err(INCOMPATIBLE); }
        let keys = collect_keys(current, dbi)?;
        let mut root = root.borrow_mut();
        if root.scopes.len() >= MAX_DEPTH { return Err(TXN_FULL); }
        let marker = root.undo.len();
        let scope = root.next_scope;
        root.next_scope = scope.checked_add(1).ok_or(TXN_FULL)?;
        root.scopes.push(Scope { id: scope, undo: marker });
        root.failed = true; // An unexpected unwind forbids a partial commit.
        let mut guard = DropScope { root: &mut root, scope, finished: false };
        let result = (|| {
            for key in keys {
                guard.root.stage(key, None)?;
                #[cfg(test)]
                PANIC_DROP.with(|control| { if control.replace(false) { panic!("injected DROP unwind"); } });
            }
            if delete != 0 { guard.root.stage(catalog(dbi), None)?; }
            Ok(())
        })();
        if result.is_err() { guard.root.restore(marker); }
        guard.root.scopes.pop();
        if guard.root.scopes.len() == 1 { guard.root.undo.clear(); guard.root.undo_bytes = 0; }
        guard.root.failed = false;
        guard.finished = true;
        result
    })
}

unsafe fn cursor_live<'a>(pointer: *mut Cursor) -> Result<(&'a mut Cursor, &'a Txn)> {
    let cursor = unsafe { pointer.as_mut() }.ok_or(INVALID)?;
    if cursor.creator != process_id() || cursor.thread != current_thread() || cursor.txn.is_null() { return Err(BAD_TXN); }
    let txn = unsafe { txn_ref(cursor.txn, true)? };
    Ok((cursor, txn))
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_cursor_open(txn: *mut Txn, dbi: u32, out: *mut *mut Cursor) -> i32 {
    boundary(|| unsafe {
        let current = txn_ref(txn, true)?;
        if !exists(current, dbi)? { return Err(BAD_DBI); }
        if current.cursors.len() >= 1024 { return Err(TXN_FULL); }
        let out = out.as_mut().ok_or(INVALID)?;
        (*txn).cursors.try_reserve(1).map_err(|_| TXN_FULL)?;
        let cursor = Box::into_raw(Box::new(Cursor { txn, creator: current.creator,
            thread: current.thread, dbi, last: None, deleted: false, cell: None, reverse: false }));
        (*txn).cursors.push(cursor); *out = cursor; Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_cursor_close(cursor: *mut Cursor) {
    let _ = boundary(|| unsafe {
        let current = cursor.as_ref().ok_or(INVALID)?;
        if current.creator != process_id() || current.thread != current_thread() { return Err(BAD_TXN); }
        let mut current = Box::from_raw(cursor);
        current.cell.take();
        if !current.txn.is_null() {
            let txn = &mut *current.txn;
            txn.cursors.retain(|pointer| *pointer != cursor);
        }
        Ok(())
    });
}
fn read_step(cursor: &mut Cursor, epoch: &Arc<Epoch>, bound: Bound<Key>, reverse: bool, reuse: bool) -> Result<Option<Row>> {
    if !reuse {
        let low = if reverse { Bound::Included(lower(cursor.dbi)) } else { bound };
        let high = if reverse { bound } else { Bound::Excluded(upper(cursor.dbi)) };
        cursor.cell = Some(ReadCursor::try_new(epoch.clone(), |epoch| {
            #[cfg(not(feature = "btree-blocks"))]
            { epoch.read.kv(DATA).map_err(code)?.cursor(slice_bound(&low), slice_bound(&high), reverse).map_err(code) }
            #[cfg(feature = "btree-blocks")]
            { record_cursor(epoch, cursor.dbi, low, high, reverse) }
        })?);
        cursor.reverse = reverse;
    }
    cursor.cell.as_mut().ok_or(BAD_TXN)?.with_dependent_mut(|_epoch, native| {
        #[cfg(not(feature = "btree-blocks"))]
        { native.next().map(|next| { let (key,value)=next.map_err(code)?; row(key,value) }).transpose() }
        #[cfg(feature = "btree-blocks")]
        { native.next().transpose() }
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_cursor_get(pointer: *mut Cursor, out_key: *mut Val, out_value: *mut Val, operation: i32) -> i32 {
    boundary(|| unsafe {
        if out_key.is_null() || out_value.is_null() { return Err(INVALID); }
        let (cursor, txn) = cursor_live(pointer)?;
        // Opening a read cursor admitted this DBI in its immutable Epoch.
        // A reset/renew cannot transfer the cursor to another Epoch: reset
        // clears its cell and transaction link first. Writers still recheck
        // their mutable catalog on every operation.
        if matches!(txn.state, State::Write(_)) && !exists(txn, cursor.dbi)? { return Err(BAD_DBI); }
        let reverse = operation == 6 || operation == 12;
        let bound = match operation {
            0 => Bound::Included(lower(cursor.dbi)),
            6 => Bound::Excluded(upper(cursor.dbi)),
            8 | 12 => cursor.last.map(Bound::Excluded).unwrap_or_else(|| if reverse { Bound::Excluded(upper(cursor.dbi)) } else { Bound::Included(lower(cursor.dbi)) }),
            15 | 16 | 17 => Bound::Included(key(cursor.dbi, out_key)?),
            4 => Bound::Included(cursor.last.ok_or(NOTFOUND)?),
            _ => return Err(INVALID),
        };
        let wanted = if matches!(operation, 4 | 15 | 16) {
            Some(match bound { Bound::Included(key) => key, _ => return Err(INVALID) })
        } else { None };
        let found = match &txn.state {
            State::Read(Some(epoch)) if operation == 4 => {
                if cursor.deleted { return Err(NOTFOUND); }
                epoch_exact(epoch, &wanted.unwrap())?
            }
            State::Read(Some(epoch)) => {
                let reuse = matches!(operation, 8 | 12) && cursor.cell.is_some()
                    && cursor.reverse == reverse && cursor.last.is_some();
                read_step(cursor, epoch, bound, reverse, reuse)?
            }
            State::Write(root) if operation == 4 => {
                if cursor.deleted { return Err(NOTFOUND); }
                writer_exact(&root.borrow(), &wanted.unwrap())?
            }
            State::Write(root) if operation == 6 => writer_last(&root.borrow(), cursor.dbi)?,
            State::Write(root) if operation == 17 => {
                let root = root.borrow();
                let beyond = match (root.tail(cursor.dbi), &bound) {
                    (Some(Tail::Empty), _) => true,
                    (Some(Tail::Maximum(maximum)), Bound::Included(target)) => *target > maximum,
                    _ => false,
                };
                if beyond { None } else {
                    #[cfg(feature = "btree-blocks")]
                    {
                        match &bound {
                            Bound::Included(target) if cursor.dbi != 0 => match writer_exact(&root, target)? {
                                Some(found) => Some(found),
                                None => writer_seek(&root, cursor.dbi, bound, false)?,
                            },
                            _ => writer_seek(&root, cursor.dbi, bound, false)?,
                        }
                    }
                    #[cfg(not(feature = "btree-blocks"))]
                    { writer_seek(&root, cursor.dbi, bound, false)? }
                }
            }
            State::Write(root) => writer_seek(&root.borrow(), cursor.dbi, bound, reverse)?,
            State::Read(None) => return Err(BAD_TXN),
        };
        let found = found.ok_or(NOTFOUND)?;
        if wanted.is_some_and(|wanted| wanted != found.key) {
            cursor.cell.take(); cursor.last = None; return Err(NOTFOUND);
        }
        cursor.last = Some(found.key); cursor.deleted = false;
        (*out_key).size = if cursor.dbi == 0 { 1 } else { 8 };
        #[cfg(not(feature = "btree-blocks"))]
        { (*out_key).data = found.key_data.add(if cursor.dbi == 0 {15} else {8}).cast_mut().cast(); }
        #[cfg(feature = "btree-blocks")]
        { (*out_key).data = found.key_data.cast_mut().cast(); }
        (*out_value).size = found.size; (*out_value).data = found.data; Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_cursor_put(pointer: *mut Cursor, input: *mut Val, value: *mut Val, flags: u32) -> i32 {
    boundary(|| unsafe {
        let (cursor, _txn) = cursor_live(pointer)?;
        let physical = key(cursor.dbi, input)?;
        if flags & 0x40 != 0 && cursor.last != Some(physical) { return Err(INVALID); }
        let rc = ubt_put(cursor.txn, cursor.dbi, input, value, flags & !0x40);
        if rc != 0 { return Err(rc); }
        cursor.cell.take(); cursor.last = Some(physical); cursor.deleted = false; Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ubt_cursor_del(pointer: *mut Cursor, flags: u32) -> i32 {
    boundary(|| unsafe {
        if flags != 0 { return Err(INVALID); }
        let (cursor, txn) = cursor_live(pointer)?;
        let State::Write(root) = &txn.state else { return Err(libc::EACCES); };
        let key = cursor.last.ok_or(NOTFOUND)?;
        let mut root = root.borrow_mut();
        if cursor.deleted || root.lookup(&key)?.is_none() { return Err(NOTFOUND); }
        root.stage(key, None)?; cursor.deleted = true; cursor.cell.take(); Ok(())
    })
}

#[cfg(test)]
std::thread_local! { static PANIC_DROP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;


#[cfg(feature = "btree-blocks")]
fn block_code(error: blocks::Error) -> i32 { match error { blocks::Error::Native(e)=>code(e), blocks::Error::Limit=>TXN_FULL, blocks::Error::Invalid=>INVALID, blocks::Error::Corrupt=>IO } }
#[cfg(feature = "btree-blocks")]
fn block_dbi(key:&Key)->Result<Option<u32>> { let db=u64::from_be_bytes(key[..8].try_into().unwrap()); if db==0||db==CATALOG {Ok(None)}else{u32::try_from(db).map(Some).map_err(|_|IO)} }
#[cfg(feature = "btree-blocks")]
fn block_row(row:blocks::Row<'_>)->Row {Row{key:row.logical_key,key_data:row.visible_key.as_ptr(),data:row.value.as_ptr().cast_mut().cast(),size:row.value.len()}}
#[cfg(feature = "btree-blocks")]
fn base_value<'a>(epoch:&'a Epoch,key:&Key)->Result<Option<&'a[u8]>> {
    let Some(dbi)=block_dbi(key)? else {return epoch.read.get(DATA,key).map_err(code)};
    let read=epoch.read.borrow_owner();
    epoch.read.with_dependent(|_,lookup|lookup.value_for(read,dbi,key))
}
#[cfg(feature = "btree-blocks")]
enum RecordCursor<'a>{Raw(native::Cursor<'a>),Grouped(blocks::LogicalCursor<'a>)}
#[cfg(feature = "btree-blocks")]
impl Iterator for RecordCursor<'_>{type Item=Result<Row>;fn next(&mut self)->Option<Self::Item>{match self{Self::Raw(c)=>c.next().map(|r|{let(k,v)=r.map_err(code)?;row(k,v)}),Self::Grouped(c)=>c.next().map(|r|r.map(block_row).map_err(block_code))}}}
#[cfg(feature = "btree-blocks")]
fn logical_bound(bound:Bound<Key>,dbi:u32)->Bound<[u8;8]>{match bound{Bound::Included(k) if k[..8]==(dbi as u64).to_be_bytes()=>Bound::Included(k[8..].try_into().unwrap()),Bound::Excluded(k) if k[..8]==(dbi as u64).to_be_bytes()=>Bound::Excluded(k[8..].try_into().unwrap()),_=>Bound::Unbounded}}
#[cfg(feature = "btree-blocks")]
fn record_cursor(epoch:&Epoch,dbi:u32,lo:Bound<Key>,hi:Bound<Key>,reverse:bool)->Result<RecordCursor<'_>>{
    if dbi==0{return Ok(RecordCursor::Raw(epoch.read.kv(DATA).map_err(code)?.cursor(slice_bound(&lo),slice_bound(&hi),reverse).map_err(code)?))}
    let read=epoch.read.borrow_owner();
    Ok(RecordCursor::Grouped(epoch.read.with_dependent(|_,lookup|lookup.cursor_for(read,dbi,logical_bound(lo,dbi),logical_bound(hi,dbi),reverse))?))
}
#[cfg(feature = "btree-blocks")]
fn block_env_count(read:&native::ReadTxn)->Result<usize>{
    let view=read.kv(DATA).map_err(code)?;let mut last=None;let mut count=0usize;
    for next in view.cursor(Bound::Unbounded,Bound::Unbounded,false).map_err(code)?{let(k,_)=next.map_err(code)?;if k.len()<8{return Err(IO)}let db=u64::from_be_bytes(k[..8].try_into().unwrap());
        if db==0||db==CATALOG{if k.len()!=16{return Err(IO)}count=count.checked_add(1).ok_or(MAP_FULL)?}
        else{let dbi=u32::try_from(db).map_err(|_|IO)?;if last!=Some(dbi){let rows=blocks::SnapshotRows::open_audited(read,DATA,dbi,blocks::Limits::default()).map_err(block_code)?;count=count.checked_add(rows.audit().rows).ok_or(MAP_FULL)?;last=Some(dbi)}}
    }Ok(count)
}
#[cfg(feature = "btree-blocks")]
impl Root {fn commit_blocks(&self)->Result<()>{
    let owner=&self.base.owner;
    if !Arc::ptr_eq(owner,&self._reservation.0){return Err(BAD_TXN)}
    let mut write=owner.engine.begin_write().map_err(code)?;
    // This genuine current read is transient and charged to final commit. The
    // writer mutex is already held; the actual base Epoch stays the data owner.
    {let current=owner.engine.begin_read();if current.sequence()!=self.base.read.sequence(){return Err(BAD_TXN)}}
    let mut operations=MAX_KEYS;let mut bytes=MAX_BYTES-4096;let mut last=None;
    for (key,value) in &self.pending{
        let Some(dbi)=block_dbi(key)? else {let size=16+value.as_ref().map_or(0,Vec::len);if operations==0||size>bytes{return Err(TXN_FULL)}operations-=1;bytes-=size;match value{Some(v)=>write.put(DATA,key,v).map_err(code)?,None=>write.delete(DATA,key).map_err(code)?}continue};
        if last==Some(dbi){continue}last=Some(dbi);
        let mut changes=Vec::new();let range=self.pending.range(lower(dbi)..upper(dbi));let count=range.clone().count();changes.try_reserve_exact(count).map_err(|_|TXN_FULL)?;
        for (key,value) in range{changes.push(blocks::Change{key:key[8..].try_into().unwrap(),value:value.as_deref()})}
        let projection=changes.capacity().checked_mul(std::mem::size_of::<blocks::Change<'_>>()).ok_or(TXN_FULL)?;
        let read=self.base.read.borrow_owner();
        let rows=self.base.read.with_dependent(|_,lookup|lookup.rows_for(read,dbi))?;
        let limits=blocks::Limits{operations,encoded_bytes:bytes,scratch_bytes:MAX_BYTES.checked_sub(projection).ok_or(TXN_FULL)?,..Default::default()};
        let plan=rows.plan(&changes,limits).map_err(block_code)?;operations=operations.checked_sub(plan.operations()).ok_or(TXN_FULL)?;bytes=bytes.checked_sub(plan.encoded_bytes()).ok_or(TXN_FULL)?;plan.apply(&mut write).map_err(block_code)?;
    }
    write.commit().map_err(code)?;Ok(())
}}
