//! C ABI boundary. Raw handles follow LMDB's caller lifetime contract.
//! All unsafe code is isolated here; the database engine remains safe Rust.
#![allow(non_camel_case_types, clippy::missing_safety_doc)]
#![deny(unsafe_op_in_unsafe_fn)]
mod alias;
mod alias_finalize;
mod alias_index;
#[cfg(all(test, target_os = "linux"))]
mod alias_profile_tests;
#[cfg(test)]
mod alias_recovery_tests;
mod alias_write;
mod catalog;
mod chunks;
mod compat_features;
#[cfg(test)]
mod coordination_tests;
mod copy;
mod crypto;
mod crypto_module;
mod cursor;
mod cursor_stream;
mod dbi_state;
mod durability;
mod environment;
mod incremental;
mod limits;
mod maintenance;
mod map_size;
mod names;
mod native_order;
mod operations;
mod prepared_metadata;
#[cfg(all(test, target_os = "linux"))]
mod prepared_tests;
mod previous;
#[cfg(all(test, target_os = "linux"))]
mod previous_tests;
mod private_read;
mod process;
mod reader_slots;
mod rollback;
#[cfg(test)]
mod rollback_tests;
mod stats;
mod storage_path;
mod transaction;
mod view_alias;
mod visibility;
use alias::{AliasHandle, AliasId, BoundAlias, DataView, LogicalAlias, NativeId, ResolvedAlias};
use std::{
    ffi::{CStr, c_char, c_int, c_uint, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::{Arc, Mutex},
    thread::ThreadId,
};
use ulmdb::{
    CommitIntent, DatabaseHandle, DatabaseKind, Environment, Error, PreparedWriteTxn, ReadTxn,
    Savepoint, WriteTxn,
};
const NOTFOUND: i32 = -30798;
const KEYEXIST: i32 = -30799;
const BAD_TXN: i32 = -30782;
const TXN_PENDING: i32 = -30775;
const CANT_ROLLBACK: i32 = -30774;
const BAD_DBI: i32 = -30780;
const BAD_VALSIZE: i32 = -30781;
const INCOMPATIBLE: i32 = -30784;
const RDONLY: u32 = 0x20000;
const CREATE: u32 = 0x40000;
const DUPSORT: u32 = 4;
const MAIN: &[u8] = b"\0lmdb-main";
type FileHandle = ulmdb_platform::RawFileHandle;
#[cfg(windows)]
type FileMode = c_int;
#[cfg(not(windows))]
type FileMode = libc::mode_t;
fn requested_file_mode(mode: FileMode) -> u32 {
    #[cfg(windows)]
    {
        let _ = mode;
        0o600
    } // LMDB ignores its mode argument on Windows.
    #[cfg(not(windows))]
    {
        (mode & 0o777).into()
    }
}
type FfiResult<T> = Result<T, i32>;
type Pair = (Vec<u8>, Vec<u8>);
type Transform = Option<Arc<ulmdb_storage::transform::TransformContext>>;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MDB_val {
    pub mv_size: usize,
    pub mv_data: *mut c_void,
}
#[repr(C)]
pub struct MDB_stat {
    pub ms_psize: u32,
    pub ms_depth: u32,
    pub ms_branch_pages: usize,
    pub ms_leaf_pages: usize,
    pub ms_overflow_pages: usize,
    pub ms_entries: usize,
}
#[repr(C)]
pub struct MDB_envinfo {
    pub me_mapaddr: *mut c_void,
    pub me_mapsize: usize,
    pub me_last_pgno: usize,
    pub me_last_txnid: usize,
    pub me_maxreaders: u32,
    pub me_numreaders: u32,
}
struct Shared {
    cache: chunks::Options,
    directory: std::path::PathBuf,
    transform: Transform,
    // Shared with MDB_env: one runtime policy authority for all owners.
    flags: Arc<std::sync::atomic::AtomicU32>,
    options: Mutex<std::collections::BTreeMap<AliasId, compat_features::DbOptions>>,
    handles: Mutex<Vec<Option<AliasHandle>>>,
    alias_ids: alias::AliasIds,
    aliases: Mutex<dbi_state::AliasStates>,
    writer_thread: Mutex<Option<ThreadId>>,
    // Short setup fence; native writers retain their separate ownership lease.
    lifecycle: Mutex<usize>,
    maintenance: Mutex<maintenance::Owner>,
    map_bounds: Mutex<map_size::Bounds>,
    database_id_floor: std::sync::atomic::AtomicU64,
    rollback_invalid: Mutex<std::collections::BTreeSet<NativeId>>,
    maxdbs: u32,
    maxreaders: u32,
    page_size: ulmdb::PageSize,
    readers: std::sync::atomic::AtomicU32,
    reader_threads: Mutex<Vec<reader_slots::ThreadSlot>>,
    poisoned: std::sync::atomic::AtomicBool,
    catalog_terminator: bool,
    profile: alias::Profile,
    creator: u32,
    visibility: Option<visibility::Visibility>,
    // Drop retained native ownership before releasing the C environment lease.
    previous: previous::PreviousState,
    coordination: Option<ulmdb::CoordinatedEnvironment>,
    external: Option<ulmdb::ExternalSynchronization>,
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fixed_map: Option<Arc<ulmdb::FixedMap>>,
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fixed_declaration: Option<Arc<ulmdb::FixedMapDeclaration>>,
    _environment_lease: Option<process::EnvironmentLease>,
}
fn read_ahead(flags: u32) -> ulmdb::ReadAhead {
    if cfg!(target_os = "linux") && flags & 0x800000 != 0 {
        ulmdb::ReadAhead::Random
    } else {
        ulmdb::ReadAhead::Normal
    }
}
impl Shared {
    fn max_key_size(&self) -> usize {
        max_key_size(self.page_size, self.catalog_terminator)
    }
    fn read_ahead(&self) -> ulmdb::ReadAhead {
        read_ahead(self.flags.load(std::sync::atomic::Ordering::Acquire))
    }
    fn open_snapshot(&self, empty_main: bool) -> FfiResult<ReadTxn> {
        previous::metadata_snapshot(self, false, empty_main)
    }
    fn check_process(&self) -> FfiResult<()> {
        if self.creator != std::process::id() {
            return Err(libc::EINVAL);
        }
        Ok(())
    }
    fn native_snapshot(&self, empty_main: bool) -> FfiResult<ReadTxn> {
        self.check_process()?;
        if let Some(external) = &self.external {
            let read = external
                .begin_read_with_cache_options(
                    &self.directory,
                    self.read_ahead(),
                    self.transform.clone(),
                    self.cache.clone(),
                )
                .map_err(code)?;
            return if empty_main {
                read.project_empty_kv(MAIN).map_err(code)
            } else {
                Ok(read)
            };
        }
        match &self.coordination {
            Some(coordination) => if empty_main {
                coordination.begin_read_with_empty_kv(MAIN)
            } else {
                coordination.begin_read()
            }
            .map_err(code),
            None => client_snapshot(
                &self.directory,
                true,
                empty_main,
                self.read_ahead(),
                self.transform.clone(),
                self.cache.clone(),
            ),
        }
    }
    fn readonly_error(&self) -> c_int {
        if self.catalog_terminator {
            -30770 // MDB_IS_READONLY, introduced by LMDB 1.0.
        } else {
            libc::EACCES
        }
    }
}
fn client_snapshot(
    directory: &std::path::Path,
    nolock: bool,
    empty_main: bool,
    advice: ulmdb::ReadAhead,
    transform: Transform,
    cache: chunks::Options,
) -> FfiResult<ReadTxn> {
    let nolock = nolock || !cfg!(any(target_os = "linux", target_os = "macos", windows));
    let read = if nolock {
        if cfg!(any(target_os = "linux", target_os = "macos", windows)) {
            ReadTxn::open_externally_synchronized_with_cache_options(
                directory, advice, transform, cache,
            )
        } else {
            ReadTxn::open_snapshot_with_cache_options(directory, advice, transform, cache)
        }
    } else {
        ReadTxn::open_coordinated_snapshot_with_cache_options(directory, advice, transform, cache)
    }
    .map_err(code)?;
    if empty_main {
        read.project_empty_kv(MAIN).map_err(code)
    } else {
        Ok(read)
    }
}
#[repr(C)]
pub struct MDB_env {
    shared: Option<Arc<Shared>>,
    path: Option<std::ffi::CString>,
    flags: Arc<std::sync::atomic::AtomicU32>,
    mapsize: usize,
    mapsize_explicit: bool,
    page_size: ulmdb::PageSize,
    shared_group: Option<u32>,
    crypto: crypto::Configuration,
    maxdbs: u32,
    maxreaders: u32,
    userctx: *mut c_void,
    data_file: Option<std::fs::File>,
    assert_callback: Option<unsafe extern "C" fn(*mut MDB_env, *const c_char)>,
}
// The separately allocated writer and savepoint remain stable while nested
// transactions borrow them. Raw ownership also keeps a reference to the parent
// handle from aliasing a child's exclusive borrow of the engine transaction.
struct Owned<T>(*mut T);
impl<T> Owned<T> {
    fn new(value: T) -> Self {
        Self(Box::into_raw(Box::new(value)))
    }
    fn take(self) -> T {
        let pointer = self.0;
        std::mem::forget(self);
        // SAFETY: this allocation is uniquely owned and consumed once.
        *unsafe { Box::from_raw(pointer) }
    }
}
impl<T> std::ops::Deref for Owned<T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.0 }
    }
}
impl<T> std::ops::DerefMut for Owned<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.0 }
    }
}
impl<T> Drop for Owned<T> {
    fn drop(&mut self) {
        unsafe {
            drop(Box::from_raw(self.0));
        }
    }
}
enum State {
    Read(ReadTxn),
    PrivateRead(private_read::PrivateReadView),
    Write(Owned<WriteTxn<'static>>),
    Child(Owned<Savepoint<'static, 'static>>),
    Prepared(prepared_metadata::PreparedOwner),
}
#[repr(C)]
pub struct MDB_txn {
    // State must be destroyed before the Arc that owns its borrowed Environment.
    state: Option<State>,
    shared: Arc<Shared>,
    env: *mut MDB_env,
    parent: *mut MDB_txn,
    child: *mut MDB_txn,
    // Created lazily: ordinary transactions allocate no private-reader state.
    private_readers: Option<Arc<private_read::PrivateReaders>>,
    thread: ThreadId,
    readonly: bool,
    txnid: usize,
    notls: bool,
    output: Vec<Box<[u8]>>,
    cursors: Vec<*mut MDB_cursor>,
    revision: u64,
    retired_rows: Vec<Vec<Pair>>,
    // Immutable, transaction-local catalog metadata, retained until reset/end
    // so every returned catalog pointer keeps its read-snapshot lifetime.
    catalog_index: Mutex<Option<Arc<catalog::CatalogIndex>>>,
    // Registry slots are process-local capabilities. Transactional catalog
    // changes decide whether their identities survive commit or abort.
    created_ids: std::collections::BTreeSet<NativeId>,
    dropped_dbis: Vec<(u32, AliasId)>,
    closed_dbis: Vec<dbi_state::ClosedDbi>,
    activated_dbis: std::collections::BTreeMap<u32, AliasId>,
    dbis: Mutex<dbi_state::TxnDbis>,
    pending_reserves: Vec<compat_features::Reserve>,
    writer_environment: Option<Arc<rollback::WriterEnvironment>>,
    writer_lease: Option<process::WriterLease>,
    reader_lease: Option<process::ReaderLease>,
    write_role: previous::WriteRole,
    quota_used: Option<usize>,
    capacity_failed: bool,
}
#[repr(C)]
pub struct MDB_cursor {
    txn: *mut MDB_txn,
    dbi: u32,
    revision: u64,
    position: Option<Pair>,
    deleted: bool,
    deleted_from: Option<Pair>,
    shared: Arc<Shared>,
    multiple: Vec<u8>,
    continuation: Option<cursor_stream::ReadContinuation>,
}
fn code(error: Error) -> i32 {
    match error {
        Error::MissingDatabase => NOTFOUND,
        Error::StaleHandle => BAD_DBI,
        Error::WrongKind => INCOMPATIBLE,
        Error::BadChecksum => -30778,
        Error::CryptoFailed => -30777,
        Error::CryptoMismatch => -30776,
        Error::ReadOnly => libc::EACCES,
        Error::Busy => libc::EBUSY,
        Error::Corrupt(_) => -30796,
        Error::OutcomeUnknown => -30795,
        Error::InvalidInput(_) => libc::EINVAL,
        Error::LimitExceeded => -30792,
        Error::Io(e) => e.raw_os_error().unwrap_or(libc::EIO),
    }
}
fn boundary(body: impl FnOnce() -> FfiResult<()>) -> i32 {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => 0,
        Ok(Err(e)) => e,
        Err(_) => -30795,
    }
}
unsafe fn get_mut<'a, T>(p: *mut T) -> FfiResult<&'a mut T> {
    // SAFETY: non-null foreign handles must be live, aligned objects of T.
    unsafe { p.as_mut().ok_or(libc::EINVAL) }
}
unsafe fn get_ref<'a, T>(p: *const T) -> FfiResult<&'a T> {
    // SAFETY: foreign handles must be live, aligned objects of T.
    unsafe { p.as_ref().ok_or(libc::EINVAL) }
}
unsafe fn bytes<'a>(p: *const MDB_val) -> FfiResult<&'a [u8]> {
    let v = unsafe { p.as_ref() }.ok_or(libc::EINVAL)?;
    if v.mv_size > isize::MAX as usize || (v.mv_size != 0 && v.mv_data.is_null()) {
        return Err(libc::EINVAL);
    }
    if v.mv_size == 0 {
        return Ok(&[]);
    }
    // SAFETY: LMDB callers supply a readable buffer with mv_size bytes.
    Ok(unsafe { std::slice::from_raw_parts(v.mv_data.cast(), v.mv_size) })
}
unsafe fn string<'a>(p: *const c_char) -> FfiResult<&'a str> {
    if p.is_null() {
        return Err(libc::EINVAL);
    }
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .map_err(|_| libc::EINVAL)
}
unsafe fn filesystem_path<'a>(
    p: *const c_char,
) -> FfiResult<std::borrow::Cow<'a, std::path::Path>> {
    if p.is_null() {
        return Err(libc::EINVAL);
    }
    let bytes = unsafe { CStr::from_ptr(p) }.to_bytes();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(std::borrow::Cow::Borrowed(std::path::Path::new(
            std::ffi::OsStr::from_bytes(bytes),
        )))
    }
    #[cfg(windows)]
    {
        Ok(std::borrow::Cow::Owned(
            ulmdb_platform::path_from_utf8(bytes).map_err(|e| code(e.into()))?,
        ))
    }
}
unsafe fn name<'a>(p: *const c_char) -> FfiResult<&'a [u8]> {
    if p.is_null() {
        Ok(MAIN)
    } else {
        Ok(unsafe { CStr::from_ptr(p) }.to_bytes())
    }
}
fn set_value(out: *mut MDB_val, value: &[u8]) -> FfiResult<()> {
    let out = unsafe { get_mut(out)? };
    *out = MDB_val {
        mv_size: value.len(),
        mv_data: value.as_ptr().cast_mut().cast(),
    };
    Ok(())
}
impl MDB_txn {
    fn correct_thread(&self) -> bool {
        self.shared.creator == std::process::id()
            && ((self.readonly && self.notls) || self.thread == std::thread::current().id())
    }
    fn live(&self) -> FfiResult<()> {
        if !self.correct_thread()
            || self.state.is_none()
            || matches!(self.state, Some(State::Prepared(_)))
            || !self.child.is_null()
            || self
                .private_readers
                .as_ref()
                .is_some_and(|readers| readers.active())
            || self.capacity_failed
        {
            Err(BAD_TXN)
        } else {
            Ok(())
        }
    }
    fn preflight_consume(&self) -> FfiResult<()> {
        let mut current = self;
        loop {
            if let Some(readers) = &current.private_readers {
                if readers.active() {
                    return Err(BAD_TXN);
                }
                // Check cleanup bookkeeping before any ancestor is consumed.
                // With zero leases, only the writer-owning thread can use it.
                drop(readers.opened_slots()?);
            }
            if current.child.is_null() {
                return Ok(());
            }
            // SAFETY: an exclusive child is retained until it detaches. No
            // native owner is moved or dropped during this read-only preflight.
            current = unsafe { get_ref(current.child)? };
        }
    }
    fn writer(&mut self) -> FfiResult<&mut WriteTxn<'static>> {
        self.live()?;
        let flags = self.shared.flags.load(std::sync::atomic::Ordering::Acquire);
        match self.state.as_mut() {
            Some(State::Write(w)) => {
                write_map::sample_initialization(w, flags)?;
                Ok(w)
            }
            Some(State::Child(w)) => {
                write_map::sample_initialization(w, flags)?;
                Ok(w)
            }
            _ => Err(self.shared.readonly_error()),
        }
    }
    fn peek_handle(&self, dbi: u32) -> FfiResult<AliasHandle> {
        self.peek_public_alias(dbi).map(|(alias, _)| alias)
    }
    fn handle(&mut self, dbi: u32) -> FfiResult<ResolvedAlias> {
        self.load_public_alias(dbi)
    }
    fn lookup(&self, h: &ResolvedAlias, key: &[u8]) -> FfiResult<Option<&[u8]>> {
        if let Some(value) = self.reserve_value(h, key) {
            return Ok(Some(value));
        }
        // Clearing an INTEGERKEY database releases its learned width. The
        // empty native branch can still carry its former integer ordering;
        // there is no key to compare until the next successful put learns and
        // installs the new width. Prove emptiness on this exact alias without
        // submitting the new key to the former width's validator.
        let settings = self.settings(h)?;
        if settings.flags & 8 != 0
            && settings.key_width == 0
            && matches!(
                self.actual_native_order(h)?.keys,
                ulmdb::ordering::OrderingSpec::UnsignedInteger { .. }
            )
            && self
                .seek_database_pair(h, None, false, false)
                .map_err(code)?
                .is_none()
        {
            return Ok(None);
        }
        if h.kind() == DatabaseKind::Kv {
            // Non-integer keys cannot enter the seek above. Nothing can invoke
            // a callback or mutate these txn-local settings between their read
            // and binding; keep the general re-read for the integer seek path.
            let callbacks = self.bound_ordering_callbacks_with_settings(
                h,
                (settings.flags & 8 == 0).then_some(settings),
            )?;
            let result = match (&h.binding.view, self.state.as_ref()) {
                (DataView::Writable(view), Some(State::Write(w))) => {
                    w.view_get_with_ordering(view, key, &callbacks)
                }
                (DataView::Writable(view), Some(State::Child(w))) => {
                    w.view_get_with_ordering(view, key, &callbacks)
                }
                (DataView::Readonly(view), Some(State::PrivateRead(r))) => {
                    r.read_scope().get_with_ordering(view, key, &callbacks)
                }
                (DataView::Direct, _) => {
                    if let Some(detached) = self.detached_database(h.id())? {
                        match self.state.as_ref() {
                            Some(State::Write(w)) => {
                                w.detached_get_with_ordering(&detached, key, &callbacks)
                            }
                            Some(State::Child(w)) => {
                                w.detached_get_with_ordering(&detached, key, &callbacks)
                            }
                            _ => Err(Error::StaleHandle),
                        }
                    } else {
                        self.check_native_actual(h.actual()).map_err(code)?;
                        match self.state.as_ref() {
                            Some(State::Read(r)) => {
                                r.get_with_ordering(h.actual().name(), key, &callbacks)
                            }
                            Some(State::PrivateRead(r)) => {
                                r.get_with_ordering(h.actual().name(), key, &callbacks)
                            }
                            Some(State::Write(w)) => {
                                w.get_with_ordering(h.actual().name(), key, &callbacks)
                            }
                            Some(State::Child(w)) => {
                                w.get_with_ordering(h.actual().name(), key, &callbacks)
                            }
                            _ => Err(Error::InvalidInput("inactive alias source")),
                        }
                    }
                }
                _ => Err(Error::InvalidInput("alias read source mismatch")),
            };
            result.map_err(code)
        } else if matches!(h.kind(), DatabaseKind::Duplicates { .. }) {
            self.seek_database_pair(h, Some((key, None)), true, false)
                .map(|pair| {
                    pair.filter(|(stored, _)| self.key_order(h, stored, key).is_eq())
                        .map(|(_, value)| value)
                })
                .map_err(code)
        } else {
            Err(INCOMPATIBLE)
        }
    }
    fn pairs(&self, h: &ResolvedAlias) -> FfiResult<Vec<Pair>> {
        let mut rows = Vec::new();
        let mut previous: Option<(&[u8], &[u8])> = None;
        loop {
            let bound = previous.map(|(key, value)| (key, Some(value)));
            let Some((key, value)) = self
                .seek_database_pair(h, bound, false, false)
                .map_err(code)?
            else {
                break;
            };
            rows.push((key.to_vec(), value.to_vec()));
            previous = Some((key, value));
        }
        if h.is_main() {
            rows.extend(self.catalog_pairs()?);
            rows.sort();
        }
        Ok(rows)
    }
    fn owned_output(&mut self, out: *mut MDB_val, value: Vec<u8>) -> FfiResult<()> {
        self.output.clear();
        self.output.push(value.into_boxed_slice());
        set_value(out, self.output.last().unwrap())
    }
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}
fn max_key_size(size: ulmdb::PageSize, modern: bool) -> usize {
    if size == ulmdb::PageSize::DEFAULT && !modern {
        511
    } else {
        size.lmdb_key_limit()
    }
}
fn validate_key(key: &[u8], max_key: usize) -> FfiResult<()> {
    if key.is_empty() || key.len() > max_key {
        Err(BAD_VALSIZE)
    } else {
        Ok(())
    }
}

mod write_map;

mod fixed_map;
