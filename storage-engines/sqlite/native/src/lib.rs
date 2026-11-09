//! Private, opt-in SQLite owner over the existing synchronous snapshot-log engine.
//! No borrowed native writer survives an FFI call. Pending records are the bounded
//! SQL transaction's actual write set, not a cache of committed query results.
#![allow(non_camel_case_types)]
mod delta;
use delta::{Pending, Value};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::{CStr, c_char, c_int, c_void},
    ops::Bound,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    ptr,
    rc::Rc,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering},
    },
    thread::{self, ThreadId},
};
use ulmdb::{DatabaseKind, Error, snapshot_log as native};

const NOTFOUND: i32 = -30798;
const KEYEXIST: i32 = -30799;
const BAD_TXN: i32 = -30782;
const BAD_DBI: i32 = -30780;
const MAP_FULL: i32 = -30792;
const TXN_FULL: i32 = -30788;
const RDONLY: u32 = 0x20000;
const NOTLS: u32 = 0x200000;
const CREATE: u32 = 0x40000;
const INVALID: i32 = 22;
const BUSY: i32 = 16;
const IO: i32 = 5;
const DBI: u32 = 1;
const NAME: &[u8] = b"sql_kv_v1";
const MAX_KEYS: usize = 65536;
const MAX_BYTES: usize = 64 << 20;
// Covers ordered-map nodes and up to twice the live undo-entry capacity.
const ENTRY_CHARGE: usize = 256;
const MAX_DEPTH: usize = 1024;
type Key = [u8; 8];
type FfiResult<T> = std::result::Result<T, i32>;
type SharedRoot = Rc<RefCell<Root>>;
thread_local! { static THREAD_ID: ThreadId = thread::current().id(); }
fn current_thread() -> ThreadId {
    THREAD_ID.with(|id| *id)
}

#[repr(C)]
pub struct MDB_val {
    pub mv_size: usize,
    pub mv_data: *mut c_void,
}
struct Owner {
    engine: native::Environment,
    writing: AtomicBool,
    creator: u32,
    binding: Binding,
}
#[derive(PartialEq, Eq)]
struct Binding {
    directory: (u64, u64),
    journal: (u64, u64),
    length: u64,
}
struct Registry {
    creator: u32,
    owners: Mutex<BTreeMap<PathBuf, Weak<Owner>>>,
}
static REGISTRY: OnceLock<Registry> = OnceLock::new();
static REGISTRY_PROCESS: AtomicU32 = AtomicU32::new(0);

// A private anonymous page is cleared by the kernel in every fork child,
// including raw SYS_fork. No user-space atfork hook or copied init lock is used.
// The successful page lives for the process lifetime (one native system page).
#[repr(C)]
struct ProcessGuard {
    valid: AtomicU8,
    creator: u32,
}
static PROCESS_GUARD: AtomicUsize = AtomicUsize::new(0);
const PID_FALLBACK: usize = 1;
const PID_INVALID: usize = 2;
fn process_id() -> u32 {
    let guard = PROCESS_GUARD.load(Ordering::Acquire);
    if guard == PID_FALLBACK {
        return std::process::id();
    }
    if guard == PID_INVALID {
        return 0;
    }
    if guard != 0 {
        // SAFETY: initialization publishes one process-lifetime mmap. Kernel
        // MADV_WIPEONFORK preserves the mapping while zeroing its child bytes.
        let guard = unsafe { &*(guard as *const ProcessGuard) };
        return if guard.valid.load(Ordering::Acquire) == 1 {
            guard.creator
        } else {
            0
        };
    }
    initialize_process_guard()
}
fn initialize_process_guard() -> u32 {
    let creator = std::process::id();
    // No allocator or initialization mutex crosses the fork boundary.
    unsafe {
        let page = libc::sysconf(libc::_SC_PAGESIZE);
        if page <= 0 {
            let _ = PROCESS_GUARD.compare_exchange(
                0,
                PID_FALLBACK,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            return process_id();
        }
        let length = page as usize;
        let address = libc::mmap(
            ptr::null_mut(),
            length,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );
        if address == libc::MAP_FAILED {
            let _ = PROCESS_GUARD.compare_exchange(
                0,
                PID_FALLBACK,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            return process_id();
        }
        if libc::madvise(address, length, libc::MADV_WIPEONFORK) != 0 {
            libc::munmap(address, length);
            let _ = PROCESS_GUARD.compare_exchange(
                0,
                PID_FALLBACK,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            return process_id();
        }
        address.cast::<ProcessGuard>().write(ProcessGuard {
            valid: AtomicU8::new(1),
            creator,
        });
        // The token never changes in its creator; only the kernel clears it.
        let _ = libc::mprotect(address, length, libc::PROT_READ);
        match PROCESS_GUARD.compare_exchange(
            0,
            address as usize,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                // Also cover a fork during initial page setup before publication.
                if creator != std::process::id() {
                    PROCESS_GUARD.store(PID_INVALID, Ordering::Release);
                    return 0;
                }
                process_id()
            }
            Err(_) => {
                libc::munmap(address, length);
                process_id()
            }
        }
    }
}

fn check_registry_process() -> FfiResult<()> {
    let process = process_id();
    if process == 0 {
        return Err(BAD_TXN);
    }
    match REGISTRY_PROCESS.compare_exchange(0, process, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => Ok(()),
        Err(creator) if creator == process => Ok(()),
        Err(_) => Err(BAD_TXN),
    }
}
fn binding(path: &std::path::Path) -> FfiResult<Binding> {
    use std::os::unix::fs::MetadataExt;
    let directory = std::fs::symlink_metadata(path).map_err(|e| e.raw_os_error().unwrap_or(IO))?;
    let journal = std::fs::symlink_metadata(path.join("SNAPSHOTLOG"))
        .map_err(|e| e.raw_os_error().unwrap_or(IO))?;
    if !directory.is_dir()
        || directory.file_type().is_symlink()
        || !journal.is_file()
        || journal.file_type().is_symlink()
        || journal.nlink() != 1
    {
        return Err(INVALID);
    }
    Ok(Binding {
        directory: (directory.dev(), directory.ino()),
        journal: (journal.dev(), journal.ino()),
        length: journal.len(),
    })
}

pub struct MDB_env {
    creator: u32,
    owner: Option<Arc<Owner>>,
}
struct Reservation(Arc<Owner>);
impl Drop for Reservation {
    fn drop(&mut self) {
        // Only creator-checked FFI paths can destroy a reservation. This is
        // private process memory, never an OS lock or shared mapped flag.
        self.0.writing.store(false, Ordering::Release);
    }
}
struct Scope {
    id: u64,
    undo_start: usize,
}
enum Undo {
    Value(Key, Option<Option<Value>>),
    Database(bool),
}
struct Root {
    owner: Arc<Owner>,
    base: native::ReadTxn,
    pending: Pending<Option<Value>>,
    undo: Vec<Undo>,
    scopes: Vec<Scope>,
    next_scope: u64,
    has_database: bool,
    pending_bytes: usize,
    undo_bytes: usize,
    reservation: Option<Reservation>,
}
enum TxnState {
    Idle,
    // View borrows the genuine snapshot's heap allocation, which remains owned
    // by base through all cursor invalidations. No write-delta structures exist.
    Read {
        view: Option<native::KvRead<'static>>,
        _base: native::ReadTxn,
        _owner: Arc<Owner>,
    },
    Write(SharedRoot),
}
pub struct MDB_txn {
    state: TxnState,
    creator: u32,
    thread: ThreadId,
    scope: u64,
    parent: *mut MDB_txn,
    child: *mut MDB_txn,
    cursors: Vec<*mut MDB_cursor>,
}
pub struct MDB_cursor {
    // Destruction order is explicit: iterator before its genuine snapshot owner.
    iter: Option<native::Cursor<'static>>,
    txn: *mut MDB_txn,
    creator: u32,
    thread: ThreadId,
    last: Option<Key>,
    writer: bool,
}
impl Drop for MDB_cursor {
    fn drop(&mut self) {
        self.iter.take();
    }
}
impl MDB_txn {
    fn write_root(&self) -> FfiResult<&SharedRoot> {
        match &self.state {
            TxnState::Write(root) => Ok(root),
            _ => Err(BAD_TXN),
        }
    }
    fn has_database(&self) -> bool {
        match &self.state {
            TxnState::Idle => false,
            TxnState::Read { view, .. } => view.is_some(),
            TxnState::Write(root) => root.borrow().has_database,
        }
    }
}
fn read_state(owner: &Arc<Owner>) -> FfiResult<TxnState> {
    let base = owner.engine.begin_read();
    let view = match base.kv(NAME) {
        Ok(view) => {
            // SAFETY: the referenced Snapshot heap allocation is retained by
            // base. Every cursor is closed before recycling this Read state.
            Some(unsafe {
                std::mem::transmute::<native::KvRead<'_>, native::KvRead<'static>>(view)
            })
        }
        Err(Error::MissingDatabase) => None,
        Err(error) => return Err(code(error)),
    };
    Ok(TxnState::Read {
        view,
        _base: base,
        _owner: owner.clone(),
    })
}
fn code(e: Error) -> i32 {
    match e {
        Error::MissingDatabase => NOTFOUND,
        Error::Busy => BUSY,
        Error::LimitExceeded => MAP_FULL,
        Error::InvalidInput(_) => INVALID,
        Error::Io(e) => e.raw_os_error().unwrap_or(IO),
        _ => IO,
    }
}
fn consumed_commit_code(rc: i32) -> i32 {
    // SQLite treats these as retryable COMMIT errors. The private writer has
    // already been consumed, so native failures must instead force rollback.
    match rc {
        BUSY | libc::EAGAIN => IO,
        _ => rc,
    }
}
fn boundary(action: impl FnOnce() -> FfiResult<()>) -> i32 {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(Ok(())) => 0,
        Ok(Err(rc)) => rc,
        Err(_) => IO,
    }
}
fn options() -> native::Options {
    native::Options {
        journal_capacity: 64 << 20,
        transaction_bytes: (64 << 20) - 4096,
        max_operations: MAX_KEYS + 1,
        max_databases: 1,
        versioned_values: cfg!(feature = "versioned"),
        ..Default::default()
    }
}
unsafe fn env_ref<'a>(env: *mut MDB_env) -> FfiResult<&'a MDB_env> {
    let env = unsafe { env.as_ref() }.ok_or(INVALID)?;
    if env.creator != process_id()
        || env
            .owner
            .as_ref()
            .is_some_and(|owner| owner.creator != env.creator)
    {
        return Err(BAD_TXN);
    }
    Ok(env)
}
unsafe fn txn_ref<'a>(txn: *mut MDB_txn, require_top: bool) -> FfiResult<&'a MDB_txn> {
    let txn = unsafe { txn.as_ref() }.ok_or(INVALID)?;
    if txn.creator != process_id() || txn.thread != current_thread() {
        return Err(BAD_TXN);
    }
    if matches!(txn.state, TxnState::Idle) {
        return Err(BAD_TXN);
    }
    if require_top {
        if !txn.child.is_null() {
            return Err(BAD_TXN);
        }
        if let TxnState::Write(root) = &txn.state {
            if root.borrow().scopes.last().map(|s| s.id) != Some(txn.scope) {
                return Err(BAD_TXN);
            }
        }
    }
    Ok(txn)
}
unsafe fn idle_mut<'a>(txn: *mut MDB_txn) -> FfiResult<&'a mut MDB_txn> {
    unsafe { idle_for_creator(txn, process_id()) }
}
unsafe fn idle_for_creator<'a>(txn: *mut MDB_txn, creator: u32) -> FfiResult<&'a mut MDB_txn> {
    let txn = unsafe { txn.as_mut() }.ok_or(INVALID)?;
    if txn.creator != creator
        || creator == 0
        || !matches!(txn.state, TxnState::Idle)
        || !txn.parent.is_null()
        || !txn.child.is_null()
        || !txn.cursors.is_empty()
    {
        return Err(BAD_TXN);
    }
    Ok(txn)
}
fn install_read(txn: &mut MDB_txn, state: TxnState) {
    txn.state = state;
    txn.thread = current_thread();
    txn.scope = 0;
}
fn sql_row_id(key: &[u8], value: &[u8]) -> FfiResult<i64> {
    let key: Key = key.try_into().map_err(|_| IO)?;
    if value.len() > i32::MAX as usize {
        return Err(IO);
    }
    Ok((u64::from_be_bytes(key) ^ (1u64 << 63)) as i64)
}
unsafe fn key_copy(value: *const MDB_val) -> FfiResult<Key> {
    let value = unsafe { value.as_ref() }.ok_or(INVALID)?;
    if value.mv_size != 8 || value.mv_data.is_null() {
        return Err(INVALID);
    }
    let mut key = [0u8; 8];
    unsafe {
        ptr::copy_nonoverlapping(value.mv_data.cast::<u8>(), key.as_mut_ptr(), 8);
    }
    Ok(key)
}
unsafe fn input_bytes<'a>(value: *const MDB_val) -> FfiResult<&'a [u8]> {
    let value = unsafe { value.as_ref() }.ok_or(INVALID)?;
    if value.mv_size > 16 << 20 || (value.mv_size > 0 && value.mv_data.is_null()) {
        return Err(INVALID);
    }
    if value.mv_size == 0 {
        return Ok(&[]);
    }
    Ok(unsafe { std::slice::from_raw_parts(value.mv_data.cast(), value.mv_size) })
}
unsafe fn set_val(out: *mut MDB_val, bytes: &[u8]) -> FfiResult<()> {
    let out = unsafe { out.as_mut() }.ok_or(INVALID)?;
    *out = MDB_val {
        mv_size: bytes.len(),
        mv_data: bytes.as_ptr().cast_mut().cast(),
    };
    Ok(())
}
impl Root {
    fn writable(&self) -> FfiResult<()> {
        if self.reservation.is_none() {
            Err(BAD_TXN)
        } else {
            Ok(())
        }
    }
    fn lookup(&self, key: &Key) -> FfiResult<Option<&[u8]>> {
        if !self.has_database {
            return Err(BAD_DBI);
        }
        if let Some(value) = self.pending.get(key) {
            return Ok(value.as_ref().map(Value::as_slice));
        }
        match self.base.get(NAME, key) {
            Ok(value) => Ok(value),
            Err(Error::MissingDatabase) => Ok(None),
            Err(error) => Err(code(error)),
        }
    }
    fn undo_charge(undo: &Undo) -> usize {
        match undo {
            Undo::Value(_, previous) => {
                ENTRY_CHARGE
                    + previous
                        .as_ref()
                        .and_then(|v| v.as_ref())
                        .map_or(0, Value::len)
            }
            Undo::Database(_) => ENTRY_CHARGE,
        }
    }
    fn value_charge(value: &Option<Value>) -> usize {
        ENTRY_CHARGE + value.as_ref().map_or(0, Value::len)
    }
    fn stage(&mut self, key: Key, value: Option<Value>) -> FfiResult<()> {
        self.writable()?;
        if !self.has_database {
            return Err(BAD_DBI);
        }
        let old = self.pending.get(&key);
        if old.is_none() && self.pending.len() >= MAX_KEYS {
            return Err(TXN_FULL);
        }
        let pending_bytes =
            self.pending_bytes - old.map_or(0, Self::value_charge) + Self::value_charge(&value);
        let undo_charge = if self.scopes.len() > 1 {
            ENTRY_CHARGE + old.and_then(|v| v.as_ref()).map_or(0, Value::len)
        } else {
            0
        };
        if pending_bytes
            .checked_add(self.undo_bytes)
            .and_then(|n| n.checked_add(undo_charge))
            .is_none_or(|n| n > MAX_BYTES)
        {
            return Err(TXN_FULL);
        }
        if self.scopes.len() > 1 {
            self.undo.try_reserve(1).map_err(|_| TXN_FULL)?;
        }
        let previous = self.pending.insert(key, value);
        if self.scopes.len() > 1 {
            // Transfer the old write-set value into undo rather than cloning it.
            self.undo.push(Undo::Value(key, previous));
            self.undo_bytes += undo_charge;
        }
        self.pending_bytes = pending_bytes;
        Ok(())
    }
    fn restore(&mut self, marker: usize) {
        while self.undo.len() > marker {
            let undo = self.undo.pop().unwrap();
            self.undo_bytes -= Self::undo_charge(&undo);
            match undo {
                Undo::Value(key, previous) => {
                    if let Some(current) = self.pending.remove(&key) {
                        self.pending_bytes -= Self::value_charge(&current);
                    }
                    if let Some(previous) = previous {
                        self.pending_bytes += Self::value_charge(&previous);
                        self.pending.insert(key, previous);
                    }
                }
                Undo::Database(previous) => self.has_database = previous,
            }
        }
    }
    fn commit(&self) -> FfiResult<()> {
        if self.reservation.is_none() {
            return Ok(());
        }
        let mut write = self.owner.engine.begin_write().map_err(code)?;
        let missing = matches!(self.base.kv(NAME), Err(Error::MissingDatabase));
        if missing && self.has_database {
            write
                .create_database(NAME, DatabaseKind::Kv)
                .map_err(code)?;
        }
        for (key, value) in &self.pending {
            match value {
                Some(value) => write.put(NAME, key, value.as_slice()).map_err(code)?,
                None => {
                    write.delete(NAME, key).map_err(code)?;
                }
            }
        }
        write.commit().map_err(code)?;
        Ok(())
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_env_create(out: *mut *mut MDB_env) -> c_int {
    boundary(|| unsafe {
        let out = out.as_mut().ok_or(INVALID)?;
        let creator = process_id();
        if creator == 0 {
            return Err(BAD_TXN);
        }
        *out = Box::into_raw(Box::new(MDB_env {
            creator,
            owner: None,
        }));
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_env_set_maxdbs(env: *mut MDB_env, count: u32) -> c_int {
    boundary(|| unsafe {
        let env = env_ref(env)?;
        if env.owner.is_some() || count == 0 {
            Err(INVALID)
        } else {
            Ok(())
        }
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_env_open(
    env: *mut MDB_env,
    path: *const c_char,
    flags: u32,
    _mode: u32,
) -> c_int {
    boundary(|| unsafe {
        env_ref(env)?;
        if flags & !NOTLS != 0 || path.is_null() || (*env).owner.is_some() {
            return Err(INVALID);
        }
        // Check before filesystem changes, OnceLock initialization or mutexes.
        check_registry_process()?;
        use std::os::unix::ffi::OsStrExt;
        let path = PathBuf::from(std::ffi::OsStr::from_bytes(CStr::from_ptr(path).to_bytes()));
        if !path.is_absolute() {
            return Err(INVALID);
        }
        if !path.exists() {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&path)
                .map_err(|e| e.raw_os_error().unwrap_or(IO))?;
        }
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.raw_os_error().unwrap_or(IO))?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(INVALID);
        }
        let path = path
            .canonicalize()
            .map_err(|e| e.raw_os_error().unwrap_or(IO))?;
        let registry = REGISTRY.get_or_init(|| Registry {
            creator: (*env).creator,
            owners: Mutex::new(BTreeMap::new()),
        });
        if registry.creator != (*env).creator {
            return Err(BAD_TXN);
        }
        let mut owners = registry.owners.lock().map_err(|_| BAD_TXN)?;
        owners.retain(|_, owner| owner.strong_count() > 0);
        let owner = if let Some(owner) = owners.get(&path).and_then(Weak::upgrade) {
            if binding(&path)? != owner.binding {
                return Err(INVALID);
            }
            owner
        } else {
            let exists = path.join("SNAPSHOTLOG").exists();
            if !exists {
                if std::fs::read_dir(&path)
                    .map_err(|e| e.raw_os_error().unwrap_or(IO))?
                    .next()
                    .is_some()
                {
                    return Err(INVALID);
                }
            } else if path.join("CURRENT").exists() || path.join("RESIDENT.wal").exists() {
                return Err(INVALID);
            }
            let engine = if exists {
                native::Environment::open(&path, options())
            } else {
                native::Environment::create(&path, options())
            }
            .map_err(code)?;
            let owner = Arc::new(Owner {
                engine,
                writing: AtomicBool::new(false),
                creator: (*env).creator,
                binding: binding(&path)?,
            });
            owners.insert(path, Arc::downgrade(&owner));
            owner
        };
        (*env).owner = Some(owner);
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_env_close(env: *mut MDB_env) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if env_ref(env).is_ok() {
            drop(Box::from_raw(env));
        }
    }));
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_txn_begin(
    env: *mut MDB_env,
    parent: *mut MDB_txn,
    flags: u32,
    out: *mut *mut MDB_txn,
) -> c_int {
    boundary(|| unsafe {
        let env = env_ref(env)?;
        let owner = env.owner.as_ref().ok_or(INVALID)?;
        let out = out.as_mut().ok_or(INVALID)?;
        *out = ptr::null_mut();
        if flags & !RDONLY != 0 {
            return Err(INVALID);
        }
        let (state, scope) = if parent.is_null() && flags & RDONLY != 0 {
            (read_state(owner)?, 0)
        } else if parent.is_null() {
            owner
                .writing
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .map_err(|_| BUSY)?;
            let reservation = Some(Reservation(owner.clone()));
            let base = owner.engine.begin_read();
            let has_database = match base.kv(NAME) {
                Ok(_) => true,
                Err(Error::MissingDatabase) => false,
                Err(e) => return Err(code(e)),
            };
            (
                TxnState::Write(Rc::new(RefCell::new(Root {
                    owner: owner.clone(),
                    base,
                    pending: Pending::new(),
                    undo: Vec::new(),
                    scopes: vec![Scope {
                        id: 0,
                        undo_start: 0,
                    }],
                    next_scope: 1,
                    has_database,
                    pending_bytes: 0,
                    undo_bytes: 0,
                    reservation,
                }))),
                0,
            )
        } else {
            let parent_ref = txn_ref(parent, true)?;
            let mut root = parent_ref.write_root()?.borrow_mut();
            root.writable()?;
            if flags != 0 || !Arc::ptr_eq(&root.owner, owner) || root.scopes.len() > MAX_DEPTH {
                return Err(BAD_TXN);
            }
            let scope = root.next_scope;
            root.next_scope = scope.checked_add(1).ok_or(TXN_FULL)?;
            let undo_start = root.undo.len();
            root.scopes.push(Scope {
                id: scope,
                undo_start,
            });
            (TxnState::Write(parent_ref.write_root()?.clone()), scope)
        };
        let txn = Box::into_raw(Box::new(MDB_txn {
            state,
            creator: env.creator,
            thread: current_thread(),
            scope,
            parent,
            child: ptr::null_mut(),
            cursors: Vec::new(),
        }));
        if !parent.is_null() {
            (*parent).child = txn;
        }
        *out = txn;
        Ok(())
    })
}
unsafe fn invalidate_cursors(txn: &mut MDB_txn) {
    for cursor in txn.cursors.drain(..) {
        let cursor = unsafe { &mut *cursor };
        cursor.iter.take();
        cursor.txn = ptr::null_mut();
    }
}
unsafe fn abort_inner(txn: *mut MDB_txn) -> FfiResult<()> {
    unsafe {
        txn_ref(txn, false)?;
        if !(*txn).child.is_null() {
            abort_inner((*txn).child)?;
        }
        let mut txn = Box::from_raw(txn);
        invalidate_cursors(&mut txn);
        if !txn.parent.is_null() {
            let mut root = txn.write_root()?.borrow_mut();
            let scope = root.scopes.pop().ok_or(BAD_TXN)?;
            if scope.id != txn.scope {
                return Err(BAD_TXN);
            }
            root.restore(scope.undo_start);
            drop(root);
            (*txn.parent).child = ptr::null_mut();
        }
        Ok(())
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_txn_abort(txn: *mut MDB_txn) {
    if txn.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        let _ = abort_inner(txn);
    }));
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_txn_commit(txn: *mut MDB_txn) -> c_int {
    boundary(|| unsafe {
        txn_ref(txn, false)?;
        if !(*txn).child.is_null() {
            let rc = usql_txn_commit((*txn).child);
            if rc != 0 {
                let _ = abort_inner(txn);
                return Err(rc);
            }
        }
        txn_ref(txn, true)?;
        let mut txn = Box::from_raw(txn);
        invalidate_cursors(&mut txn);
        if txn.parent.is_null() {
            match &txn.state {
                TxnState::Read { .. } => Ok(()),
                // The staged writer has already been consumed. A BUSY here
                // cannot be retried as SQLite COMMIT: reservation contention
                // was rejected before consumption. Native mutex poison (or
                // another consumed-commit BUSY/EAGAIN) must force SQL rollback.
                TxnState::Write(root) => root.borrow().commit().map_err(consumed_commit_code),
                TxnState::Idle => Err(BAD_TXN),
            }
        } else {
            let mut root = txn.write_root()?.borrow_mut();
            root.scopes.pop().ok_or(BAD_TXN)?;
            if root.scopes.len() == 1 {
                root.undo.clear();
                root.undo_bytes = 0;
            }
            drop(root);
            (*txn.parent).child = ptr::null_mut();
            Ok(())
        }
    })
}
/// End a read without retaining its snapshot, owner or any cursor capability.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_read_recycle(txn: *mut MDB_txn) -> c_int {
    boundary(|| unsafe {
        let active = txn_ref(txn, true)?;
        if !matches!(active.state, TxnState::Read { .. })
            || !active.parent.is_null()
            || !active.cursors.is_empty()
        {
            return Err(BAD_TXN);
        }
        let finished = std::mem::replace(&mut (*txn).state, TxnState::Idle);
        drop(finished);
        // The empty carrier is fixed-size, without a past cursor-list buffer.
        (*txn).cursors = Vec::new();
        Ok(())
    })
}
/// Renew only an empty carrier; capture a fresh native head before installation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_read_renew(env: *mut MDB_env, txn: *mut MDB_txn) -> c_int {
    boundary(|| unsafe {
        let env = env_ref(env)?;
        let idle = idle_for_creator(txn, env.creator)?;
        let owner = env.owner.as_ref().ok_or(INVALID)?;
        let state = read_state(owner)?;
        install_read(idle, state);
        Ok(())
    })
}
/// An empty carrier owns no thread-bound capability and may be freed on any thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_read_discard(txn: *mut MDB_txn) -> c_int {
    boundary(|| unsafe {
        idle_mut(txn)?;
        drop(Box::from_raw(txn));
        Ok(())
    })
}
/// Capture and look up one signed SQL key in a single checked ABI boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_read_point(
    env: *mut MDB_env,
    slot: *mut *mut MDB_txn,
    id: i64,
    out: *mut MDB_val,
) -> c_int {
    boundary(|| unsafe {
        let env = env_ref(env)?;
        let slot = slot.as_mut().ok_or(INVALID)?;
        if out.is_null() {
            return Err(INVALID);
        }
        // The empty carrier has no thread capability. The current process was
        // checked above, and installation binds this new read to this thread.
        if !slot.is_null() {
            idle_for_creator(*slot, env.creator)?;
        }
        let owner = env.owner.as_ref().ok_or(INVALID)?;
        let state = read_state(owner)?;
        let key = ((id as u64) ^ (1u64 << 63)).to_be_bytes();
        let value = match &state {
            TxnState::Read { view, .. } => view.as_ref().ok_or(BAD_DBI)?.get(&key).map_err(code)?,
            _ => return Err(BAD_TXN),
        };
        if let Some(value) = value {
            sql_row_id(&key, value)?;
        }
        // The view's references point into its genuine immutable Snapshot heap
        // allocation; moving its owner into the carrier preserves those bytes.
        if slot.is_null() {
            *slot = Box::into_raw(Box::new(MDB_txn {
                state,
                creator: env.creator,
                thread: current_thread(),
                scope: 0,
                parent: ptr::null_mut(),
                child: ptr::null_mut(),
                cursors: Vec::new(),
            }));
        } else {
            install_read(&mut **slot, state);
        }
        set_val(out, value.ok_or(NOTFOUND)?)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_dbi_open(
    txn: *mut MDB_txn,
    name: *const c_char,
    flags: u32,
    out: *mut u32,
) -> c_int {
    boundary(|| unsafe {
        let txn = txn_ref(txn, true)?;
        if out.is_null()
            || name.is_null()
            || CStr::from_ptr(name).to_bytes() != NAME
            || flags & !CREATE != 0
        {
            return Err(INVALID);
        }
        if let TxnState::Read { view, .. } = &txn.state {
            if view.is_none() {
                return Err(NOTFOUND);
            }
            *out = DBI;
            return Ok(());
        }
        let mut root = txn.write_root()?.borrow_mut();
        if !root.has_database {
            if flags & CREATE == 0 {
                return Err(NOTFOUND);
            }
            root.writable()?;
            if root.scopes.len() > 1 {
                root.undo.push(Undo::Database(false));
                root.undo_bytes += ENTRY_CHARGE;
            }
            root.has_database = true;
        }
        *out = DBI;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_dbi_close(_env: *mut MDB_env, _dbi: u32) {}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_get(
    txn: *mut MDB_txn,
    dbi: u32,
    key: *const MDB_val,
    out: *mut MDB_val,
) -> c_int {
    boundary(|| unsafe {
        let txn = txn_ref(txn, true)?;
        if dbi != DBI || out.is_null() {
            return Err(BAD_DBI);
        }
        let key = key_copy(key)?;
        match &txn.state {
            TxnState::Read { view, .. } => {
                let value = view
                    .as_ref()
                    .ok_or(BAD_DBI)?
                    .get(&key)
                    .map_err(code)?
                    .ok_or(NOTFOUND)?;
                set_val(out, value)
            }
            TxnState::Write(root) => {
                let root = root.borrow();
                set_val(out, root.lookup(&key)?.ok_or(NOTFOUND)?)
            }
            TxnState::Idle => Err(BAD_TXN),
        }
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_put(
    txn: *mut MDB_txn,
    dbi: u32,
    key: *const MDB_val,
    value: *const MDB_val,
    flags: u32,
) -> c_int {
    boundary(|| unsafe {
        let txn = txn_ref(txn, true)?;
        if dbi != DBI || flags & !0x10 != 0 {
            return Err(INVALID);
        }
        let key = key_copy(key)?;
        value.as_ref().ok_or(INVALID)?;
        let mut root = txn.write_root()?.borrow_mut();
        if flags & 0x10 != 0 && root.lookup(&key)?.is_some() {
            return Err(KEYEXIST);
        }
        let value = input_bytes(value)?;
        root.stage(key, Some(Value::from_slice(value)))
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_del(
    txn: *mut MDB_txn,
    dbi: u32,
    key: *const MDB_val,
    value: *const MDB_val,
) -> c_int {
    boundary(|| unsafe {
        let txn = txn_ref(txn, true)?;
        if dbi != DBI || !value.is_null() {
            return Err(INVALID);
        }
        let key = key_copy(key)?;
        let mut root = txn.write_root()?.borrow_mut();
        root.writable()?;
        if root.lookup(&key)?.is_none() {
            return Err(NOTFOUND);
        }
        root.stage(key, None)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_cursor_open(
    txn: *mut MDB_txn,
    dbi: u32,
    out: *mut *mut MDB_cursor,
) -> c_int {
    boundary(|| unsafe {
        let txn_ref = txn_ref(txn, true)?;
        if dbi != DBI || out.is_null() || !txn_ref.has_database() {
            return Err(BAD_DBI);
        }
        let writer = matches!(txn_ref.state, TxnState::Write(_));
        let cursor = Box::into_raw(Box::new(MDB_cursor {
            iter: None,
            txn,
            creator: txn_ref.creator,
            thread: txn_ref.thread,
            last: None,
            writer,
        }));
        (*txn).cursors.push(cursor);
        *out = cursor;
        Ok(())
    })
}
unsafe fn cursor_live<'a>(cursor: *mut MDB_cursor) -> FfiResult<&'a mut MDB_cursor> {
    let cursor = unsafe { cursor.as_mut() }.ok_or(INVALID)?;
    if cursor.creator != process_id() || cursor.thread != current_thread() || cursor.txn.is_null() {
        return Err(BAD_TXN);
    }
    let txn = unsafe { &*cursor.txn };
    if !txn.child.is_null() {
        return Err(BAD_TXN);
    }
    if let TxnState::Write(root) = &txn.state {
        if root.borrow().scopes.last().map(|s| s.id) != Some(txn.scope) {
            return Err(BAD_TXN);
        }
    }
    Ok(cursor)
}
unsafe fn native_iter(txn: &MDB_txn, bound: Bound<&[u8]>) -> FfiResult<native::Cursor<'static>> {
    let TxnState::Read { view, .. } = &txn.state else {
        return Err(BAD_TXN);
    };
    let iter = view
        .as_ref()
        .ok_or(BAD_DBI)?
        .cursor(bound, Bound::Unbounded, false)
        .map_err(code)?;
    // The transaction's boxed Read state owns the real native snapshot. All
    // transaction-ending paths invalidate every iterator before dropping it.
    Ok(iter)
}
#[inline]
unsafe fn readonly_step(
    cursor: &mut MDB_cursor,
    operation: c_int,
    seek: Option<Key>,
) -> FfiResult<(i64, &'static [u8], &'static [u8])> {
    if cursor.writer {
        return Err(BAD_TXN);
    }
    if operation != 0 && operation != 8 && operation != 17 {
        return Err(INVALID);
    }
    if operation != 8 || cursor.iter.is_none() {
        let bound = if operation == 17 {
            Bound::Included(seek.as_ref().ok_or(INVALID)?.as_slice())
        } else {
            Bound::Unbounded
        };
        // Both ABI entry points first call cursor_live, which checks process,
        // thread and parent liveness before this single parent dereference.
        cursor.iter = Some(unsafe { native_iter(&*cursor.txn, bound)? });
    }
    let (key, value) = cursor
        .iter
        .as_mut()
        .unwrap()
        .next()
        .ok_or(NOTFOUND)?
        .map_err(code)?;
    let id = sql_row_id(key, value)?;
    Ok((id, key, value))
}
/// Return one signed SQL row without generic key marshaling or duplicate decoding.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_cursor_row(
    cursor: *mut MDB_cursor,
    id: *mut i64,
    out: *mut MDB_val,
    operation: c_int,
) -> c_int {
    boundary(|| unsafe {
        if id.is_null() || out.is_null() {
            return Err(INVALID);
        }
        let cursor = cursor_live(cursor)?;
        let seek = if operation == 17 {
            Some(((*id as u64) ^ (1u64 << 63)).to_be_bytes())
        } else {
            None
        };
        let (row_id, _, value) = readonly_step(cursor, operation, seek)?;
        set_val(out, value)?;
        *id = row_id;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_cursor_get(
    cursor: *mut MDB_cursor,
    key: *mut MDB_val,
    value: *mut MDB_val,
    operation: c_int,
) -> c_int {
    boundary(|| unsafe {
        if key.is_null() || value.is_null() {
            return Err(INVALID);
        }
        let cursor = cursor_live(cursor)?;
        let bound_key = match operation {
            17 => Some(key_copy(key)?), // MDB_SET_RANGE
            8 => cursor.last,           // MDB_NEXT
            0 => None,                  // MDB_FIRST
            _ => return Err(INVALID),
        };
        if !cursor.writer {
            let (_, k, v) = readonly_step(cursor, operation, bound_key)?;
            set_val(key, k)?;
            set_val(value, v)?;
            return Ok(());
        }
        // Writer scans borrow only within this call. Merge the genuine base
        // snapshot with the actual transaction delta, without materializing rows.
        let root = (&*cursor.txn).write_root()?.borrow();
        let mut bound = match bound_key {
            Some(k) => {
                if operation == 8 {
                    Bound::Excluded(k)
                } else {
                    Bound::Included(k)
                }
            }
            None => Bound::Unbounded,
        };
        loop {
            let base = match root.base.kv(NAME) {
                Ok(view) => {
                    let borrowed = match &bound {
                        Bound::Included(k) => Bound::Included(k.as_slice()),
                        Bound::Excluded(k) => Bound::Excluded(k.as_slice()),
                        Bound::Unbounded => Bound::Unbounded,
                    };
                    view.cursor(borrowed, Bound::Unbounded, false)
                        .map_err(code)?
                        .next()
                        .transpose()
                        .map_err(code)?
                }
                Err(Error::MissingDatabase) => None,
                Err(error) => return Err(code(error)),
            };
            let delta = root.pending.first_from(bound);
            let use_delta = match (&base, &delta) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some((k, _)), Some((d, _))) => d.as_slice() <= *k,
            };
            if use_delta {
                let (k, v) = delta.unwrap();
                if let Some(v) = v {
                    cursor.last = Some(*k);
                    set_val(key, k)?;
                    set_val(value, v.as_slice())?;
                    return Ok(());
                }
                bound = Bound::Excluded(*k);
            } else if let Some((k, v)) = base {
                cursor.last = Some(k.try_into().map_err(|_| IO)?);
                set_val(key, k)?;
                set_val(value, v)?;
                return Ok(());
            } else {
                return Err(NOTFOUND);
            }
        }
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_cursor_close(cursor: *mut MDB_cursor) {
    if cursor.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if (*cursor).creator != process_id() || (*cursor).thread != current_thread() {
            return;
        }
        if !(*cursor).txn.is_null() {
            let txn = &mut *(*cursor).txn;
            txn.cursors.retain(|c| *c != cursor);
        }
        drop(Box::from_raw(cursor));
    }));
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usql_strerror(code: c_int) -> *mut c_char {
    let message: &'static [u8] = match code {
        0 => b"success\0",
        NOTFOUND => b"key/database not found\0",
        KEYEXIST => b"duplicate key\0",
        BAD_TXN => b"invalid, suspended, foreign-process or wrong-thread transaction\0",
        BAD_DBI => b"invalid SQL database\0",
        TXN_FULL => b"SQL transaction exceeds 65536 keys or 64 MiB including savepoint undo\0",
        MAP_FULL => b"native snapshot-log index, journal or transaction capacity exhausted\0",
        BUSY => b"exclusive native owner or writer is busy\0",
        INVALID => b"unsupported native SQL operation or storage format\0",
        _ => b"native snapshot-log I/O, corruption or panic failure\0",
    };
    message.as_ptr().cast_mut().cast()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::CString, sync::atomic::AtomicU64};
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    struct Fixture {
        directory: PathBuf,
        env: *mut MDB_env,
    }
    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "usql-native-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            let env = unsafe { open(&directory) };
            Self { directory, env }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            unsafe {
                usql_env_close(self.env);
            }
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
    unsafe fn open(path: &std::path::Path) -> *mut MDB_env {
        unsafe {
            let mut env = ptr::null_mut();
            assert_eq!(usql_env_create(&mut env), 0);
            let path = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
            assert_eq!(usql_env_open(env, path.as_ptr(), NOTLS, 0o600), 0);
            env
        }
    }
    unsafe fn begin(env: *mut MDB_env, parent: *mut MDB_txn, read: bool) -> *mut MDB_txn {
        unsafe {
            let mut txn = ptr::null_mut();
            assert_eq!(
                usql_txn_begin(env, parent, if read { RDONLY } else { 0 }, &mut txn),
                0
            );
            txn
        }
    }
    fn key(id: i64) -> Key {
        ((id as u64) ^ (1 << 63)).to_be_bytes()
    }
    fn val(bytes: &[u8]) -> MDB_val {
        MDB_val {
            mv_size: bytes.len(),
            mv_data: bytes.as_ptr().cast_mut().cast(),
        }
    }
    unsafe fn create(txn: *mut MDB_txn) {
        unsafe {
            let mut dbi = 0;
            assert_eq!(
                usql_dbi_open(txn, c"sql_kv_v1".as_ptr(), CREATE, &mut dbi),
                0
            );
            assert_eq!(dbi, DBI);
        }
    }
    unsafe fn put(txn: *mut MDB_txn, id: i64, bytes: &[u8]) -> i32 {
        unsafe { usql_put(txn, DBI, &val(&key(id)), &val(bytes), 0) }
    }
    unsafe fn get(txn: *mut MDB_txn, id: i64) -> Option<Vec<u8>> {
        unsafe {
            let mut out = val(&[]);
            let rc = usql_get(txn, DBI, &val(&key(id)), &mut out);
            if rc == NOTFOUND {
                return None;
            }
            assert_eq!(rc, 0);
            Some(std::slice::from_raw_parts(out.mv_data.cast::<u8>(), out.mv_size).to_vec())
        }
    }
    #[test]
    fn native_savepoints_snapshots_scans_and_reopen() {
        unsafe {
            let f = Fixture::new();
            let w = begin(f.env, ptr::null_mut(), false);
            create(w);
            assert_eq!(put(w, -7, b"old"), 0);
            let child = begin(f.env, w, false);
            assert_eq!(put(w, 5, b"suspended"), BAD_TXN);
            assert_eq!(put(child, -7, b"child"), 0);
            assert_eq!(put(child, 2, b"two"), 0);
            let grandchild = begin(f.env, child, false);
            assert_eq!(put(grandchild, -7, b"grandchild"), 0);
            assert_eq!(usql_txn_commit(grandchild), 0);
            assert_eq!(get(child, -7), Some(b"grandchild".to_vec()));
            usql_txn_abort(child);
            assert_eq!(get(w, -7), Some(b"old".to_vec()));
            assert_eq!(get(w, 2), None);
            let child = begin(f.env, w, false);
            assert_eq!(put(child, i64::MAX, b"last"), 0);
            assert_eq!(usql_txn_commit(child), 0);
            assert_eq!(usql_txn_commit(w), 0);
            let old = begin(f.env, ptr::null_mut(), true);
            let mut cursor = ptr::null_mut();
            assert_eq!(usql_cursor_open(old, DBI, &mut cursor), 0);
            let mut k = val(&key(i64::MIN));
            let mut v = val(&[]);
            assert_eq!(usql_cursor_get(cursor, &mut k, &mut v, 17), 0);
            assert_eq!(
                std::slice::from_raw_parts(k.mv_data.cast::<u8>(), 8),
                key(-7)
            );
            let w = begin(f.env, ptr::null_mut(), false);
            assert_eq!(put(w, -7, b"new"), 0);
            assert_eq!(put(w, 0, b"middle"), 0);
            assert_eq!(usql_txn_commit(w), 0);
            assert_eq!(get(old, -7), Some(b"old".to_vec()));
            assert_eq!(usql_cursor_get(cursor, &mut k, &mut v, 8), 0);
            assert_eq!(
                std::slice::from_raw_parts(k.mv_data.cast::<u8>(), 8),
                key(i64::MAX)
            );
            usql_txn_abort(old);
            assert_eq!(usql_cursor_get(cursor, &mut k, &mut v, 8), BAD_TXN);
            usql_cursor_close(cursor);
            usql_env_close(f.env);
            let reopened = open(&f.directory);
            let read = begin(reopened, ptr::null_mut(), true);
            assert_eq!(get(read, -7), Some(b"new".to_vec()));
            assert_eq!(get(read, 0), Some(b"middle".to_vec()));
            usql_txn_abort(read);
            // Avoid dropping the already consumed original environment.
            let mut f = f;
            f.env = reopened;
        }
    }
    #[test]
    fn writer_reservation_and_wrong_thread_checks() {
        unsafe {
            let f = Fixture::new();
            let other = open(&f.directory);
            let w = begin(f.env, ptr::null_mut(), false);
            create(w);
            let mut blocked = ptr::null_mut();
            assert_eq!(
                usql_txn_begin(other, ptr::null_mut(), 0, &mut blocked),
                BUSY
            );
            assert!(blocked.is_null());
            let address = w as usize;
            assert_eq!(
                thread::spawn(move || {
                    let mut out = val(&[]);
                    usql_get(address as *mut MDB_txn, DBI, &val(&key(0)), &mut out)
                })
                .join()
                .unwrap(),
                BAD_TXN
            );
            usql_txn_abort(w);
            let next = begin(other, ptr::null_mut(), false);
            usql_txn_abort(next);
            usql_env_close(other);
        }
    }
    #[test]
    fn bounded_delta_refusal_preserves_previous_writes() {
        unsafe {
            assert!(ENTRY_CHARGE >= 2 * std::mem::size_of::<Undo>());
            assert!(ENTRY_CHARGE >= 2 * std::mem::size_of::<(Key, Option<Value>)>());
            let f = Fixture::new();
            let w = begin(f.env, ptr::null_mut(), false);
            create(w);
            for id in 0..MAX_KEYS {
                assert_eq!(put(w, id as i64, b"x"), 0);
            }
            assert_eq!(put(w, MAX_KEYS as i64, b"refused"), TXN_FULL);
            assert_eq!(get(w, 0), Some(b"x".to_vec()));
            assert_eq!(get(w, MAX_KEYS as i64), None);
            let child = begin(f.env, w, false);
            for id in 0..2 {
                assert_eq!(put(child, id, &vec![7; 16 << 20]), 0);
            }
            assert_eq!(put(child, 2, &vec![7; 16 << 20]), TXN_FULL);
            usql_txn_abort(child);
            assert_eq!(get(w, 0), Some(b"x".to_vec()));
            usql_txn_abort(w);
        }
    }
    #[test]
    fn empty_read_carrier_renews_fresh_and_moves_only_while_idle() {
        unsafe {
            let f = Fixture::new();
            let write = begin(f.env, ptr::null_mut(), false);
            create(write);
            assert_eq!(put(write, 1, b"old"), 0);
            assert_eq!(usql_txn_commit(write), 0);
            let read = begin(f.env, ptr::null_mut(), true);
            let mut cursor = ptr::null_mut();
            assert_eq!(usql_cursor_open(read, DBI, &mut cursor), 0);
            assert_eq!(usql_read_recycle(read), BAD_TXN);
            assert_eq!(get(read, 1), Some(b"old".to_vec()));
            usql_cursor_close(cursor);
            assert_eq!(usql_read_discard(read), BAD_TXN);
            assert_eq!(usql_read_recycle(read), 0);
            let mut output = val(&[]);
            assert_eq!(usql_get(read, DBI, &val(&key(1)), &mut output), BAD_TXN);
            assert_eq!(usql_cursor_open(read, DBI, &mut cursor), BAD_TXN);
            assert_eq!(usql_txn_commit(read), BAD_TXN);
            usql_txn_abort(read); // Idle is not an active transaction; preserve it.
            let mut unopened = ptr::null_mut();
            assert_eq!(usql_env_create(&mut unopened), 0);
            assert_eq!(usql_read_renew(unopened, read), INVALID);
            usql_env_close(unopened);
            let write = begin(f.env, ptr::null_mut(), false);
            assert_eq!(put(write, 1, b"new"), 0);
            assert_eq!(usql_txn_commit(write), 0);
            assert_eq!(usql_read_renew(f.env, read), 0);
            assert_eq!(get(read, 1), Some(b"new".to_vec()));
            let address = read as usize;
            let env_address = f.env as usize;
            assert_eq!(
                thread::spawn(move || {
                    let read = address as *mut MDB_txn;
                    let env = env_address as *mut MDB_env;
                    (
                        usql_read_recycle(read),
                        usql_read_renew(env, read),
                        usql_read_discard(read),
                    )
                })
                .join()
                .unwrap(),
                (BAD_TXN, BAD_TXN, BAD_TXN)
            );
            assert_eq!(get(read, 1), Some(b"new".to_vec()));
            assert_eq!(usql_read_recycle(read), 0);
            let address = read as usize;
            let env_address = f.env as usize;
            thread::spawn(move || {
                let read = address as *mut MDB_txn;
                assert_eq!(usql_read_renew(env_address as *mut MDB_env, read), 0);
                assert_eq!(get(read, 1), Some(b"new".to_vec()));
                assert_eq!(usql_read_recycle(read), 0);
            })
            .join()
            .unwrap();
            assert_eq!(usql_read_renew(f.env, read), 0);
            assert_eq!(get(read, 1), Some(b"new".to_vec()));
            assert_eq!(usql_read_recycle(read), 0);
            let address = read as usize;
            assert_eq!(
                thread::spawn(move || usql_read_discard(address as *mut MDB_txn))
                    .join()
                    .unwrap(),
                0
            );
        }
    }
    #[test]
    fn idle_carrier_keeps_no_owner_fence_after_final_environment_close() {
        unsafe {
            let mut f = Fixture::new();
            let write = begin(f.env, ptr::null_mut(), false);
            create(write);
            assert_eq!(put(write, 1, b"persisted"), 0);
            assert_eq!(usql_txn_commit(write), 0);
            let read = begin(f.env, ptr::null_mut(), true);
            assert_eq!(usql_read_recycle(read), 0);
            usql_env_close(f.env);
            f.env = ptr::null_mut();
            // A separate genuine native owner can acquire the disk lease even
            // though the empty C carrier has deliberately not been discarded.
            let owner = native::Environment::open(&f.directory, options()).unwrap();
            assert_eq!(
                owner.begin_read().get(NAME, &key(1)).unwrap(),
                Some(b"persisted".as_slice())
            );
            drop(owner);
            f.env = open(&f.directory);
            assert_eq!(usql_read_renew(f.env, read), 0);
            assert_eq!(get(read, 1), Some(b"persisted".to_vec()));
            assert_eq!(usql_read_recycle(read), 0);
            assert_eq!(usql_read_discard(read), 0);
        }
    }
    #[test]
    fn consumed_commit_busy_is_io_and_releases_the_writer_reservation() {
        assert_eq!(code(Error::Busy), BUSY);
        for os_error in [libc::EBUSY, libc::EAGAIN] {
            let ordinary = code(Error::Io(std::io::Error::from_raw_os_error(os_error)));
            assert_eq!(ordinary, os_error);
            assert_eq!(consumed_commit_code(ordinary), IO);
        }
        assert_eq!(consumed_commit_code(MAP_FULL), MAP_FULL);
        unsafe {
            let mut f = Fixture::new();
            let writer = begin(f.env, ptr::null_mut(), false);
            create(writer);
            assert_eq!(put(writer, 1, b"old"), 0);
            assert_eq!(usql_txn_commit(writer), 0);
            let owner = (*f.env).owner.as_ref().unwrap().clone();
            let poisoned = catch_unwind(AssertUnwindSafe(|| {
                let _native_writer = owner.engine.begin_write().unwrap();
                panic!("injected panic while native writer mutex is held");
            }));
            assert!(poisoned.is_err());
            // Reservation acquisition still succeeds; this is native mutex
            // poison at consuming commit, not ordinary writer contention.
            let writer = begin(f.env, ptr::null_mut(), false);
            assert_eq!(put(writer, 2, b"staged"), 0);
            assert_eq!(usql_txn_commit(writer), IO);
            assert!(!owner.writing.load(Ordering::Acquire));
            let read = begin(f.env, ptr::null_mut(), true);
            assert_eq!(get(read, 1), Some(b"old".to_vec()));
            assert_eq!(get(read, 2), None);
            usql_txn_abort(read);
            // A second reservation is not leaked or falsely BUSY. The poisoned
            // native owner consistently refuses consuming commits with IO.
            let writer = begin(f.env, ptr::null_mut(), false);
            assert_eq!(put(writer, 3, b"also staged"), 0);
            assert_eq!(usql_txn_commit(writer), IO);
            assert!(!owner.writing.load(Ordering::Acquire));
            usql_env_close(f.env);
            f.env = ptr::null_mut();
            drop(owner);
            f.env = open(&f.directory);
            let read = begin(f.env, ptr::null_mut(), true);
            assert_eq!(get(read, 1), Some(b"old".to_vec()));
            assert_eq!(get(read, 2), None);
            assert_eq!(get(read, 3), None);
            usql_txn_abort(read);
            let writer = begin(f.env, ptr::null_mut(), false);
            assert_eq!(put(writer, 4, b"reopened"), 0);
            assert_eq!(usql_txn_commit(writer), 0);
        }
    }
    #[test]
    fn typed_points_and_rows_keep_freshness_bounds_and_output_ownership() {
        unsafe {
            let f = Fixture::new();
            let w = begin(f.env, ptr::null_mut(), false);
            create(w);
            for id in [i64::MIN, -7, 0, i64::MAX] {
                assert_eq!(put(w, id, &id.to_be_bytes()), 0);
            }
            // Duplicate handling must precede payload-size validation.
            let oversized = MDB_val {
                mv_size: (16 << 20) + 1,
                mv_data: ptr::null_mut(),
            };
            assert_eq!(usql_put(w, DBI, &val(&key(0)), &oversized, 0x10), KEYEXIST);
            assert_eq!(usql_put(w, DBI, &val(&key(1)), &oversized, 0x10), INVALID);
            assert_eq!(usql_txn_commit(w), 0);
            let mut read = ptr::null_mut();
            let mut result = val(&[]);
            assert_eq!(usql_read_point(f.env, &mut read, i64::MIN, &mut result), 0);
            assert_eq!(
                std::slice::from_raw_parts(result.mv_data.cast::<u8>(), result.mv_size),
                i64::MIN.to_be_bytes()
            );
            let original = read;
            let previous = (result.mv_size, result.mv_data);
            assert_eq!(usql_read_point(f.env, &mut read, 0, &mut result), BAD_TXN);
            assert_eq!(read, original);
            assert_eq!((result.mv_size, result.mv_data), previous);
            let mut cursor = ptr::null_mut();
            assert_eq!(usql_cursor_open(read, DBI, &mut cursor), 0);
            let mut id = -8;
            assert_eq!(usql_cursor_row(cursor, &mut id, &mut result, 17), 0);
            assert_eq!(id, -7);
            let cursor_address = cursor as usize;
            assert_eq!(
                thread::spawn(move || {
                    let mut id = 123;
                    let mut value = val(&[]);
                    usql_cursor_row(cursor_address as *mut MDB_cursor, &mut id, &mut value, 8)
                })
                .join()
                .unwrap(),
                BAD_TXN
            );
            assert_eq!(usql_cursor_row(cursor, &mut id, &mut result, 8), 0);
            assert_eq!(id, 0);
            assert_eq!(usql_cursor_row(cursor, &mut id, &mut result, 8), 0);
            assert_eq!(id, i64::MAX);
            let previous = (result.mv_size, result.mv_data);
            assert_eq!(usql_cursor_row(cursor, &mut id, &mut result, 8), NOTFOUND);
            assert_eq!(id, i64::MAX);
            assert_eq!((result.mv_size, result.mv_data), previous);
            assert_eq!(usql_cursor_row(cursor, &mut id, &mut result, 99), INVALID);
            assert_eq!((result.mv_size, result.mv_data), previous);
            assert_eq!(usql_cursor_row(cursor, &mut id, &mut result, 0), 0);
            assert_eq!(id, i64::MIN);
            usql_cursor_close(cursor);
            assert_eq!(usql_read_recycle(read), 0);
            let w = begin(f.env, ptr::null_mut(), false);
            assert_eq!(put(w, 0, b"new"), 0);
            assert_eq!(usql_txn_commit(w), 0);
            assert_eq!(usql_read_point(f.env, &mut read, 0, &mut result), 0);
            assert_eq!(
                std::slice::from_raw_parts(result.mv_data.cast::<u8>(), result.mv_size),
                b"new"
            );
            assert_eq!(usql_read_recycle(read), 0);
            let previous = (result.mv_size, result.mv_data);
            assert_eq!(usql_read_point(f.env, &mut read, 1, &mut result), NOTFOUND);
            assert_eq!((result.mv_size, result.mv_data), previous);
            assert_eq!(get(read, 0), Some(b"new".to_vec()));
            assert_eq!(usql_read_recycle(read), 0);
            let env_address = f.env as usize;
            let read_address = read as usize;
            thread::spawn(move || {
                let mut slot = read_address as *mut MDB_txn;
                let mut value = val(&[]);
                assert_eq!(
                    usql_read_point(env_address as *mut MDB_env, &mut slot, i64::MAX, &mut value),
                    0
                );
                assert_eq!(slot as usize, read_address);
                assert_eq!(usql_read_recycle(slot), 0);
            })
            .join()
            .unwrap();
            assert_eq!(usql_read_discard(read), 0);
        }
    }
    #[test]
    fn typed_row_rejects_non_sql_native_keys_without_writing_outputs() {
        unsafe {
            let mut f = Fixture::new();
            usql_env_close(f.env);
            f.env = ptr::null_mut();
            let owner = native::Environment::open(&f.directory, options()).unwrap();
            let mut write = owner.begin_write().unwrap();
            write.create_database(NAME, DatabaseKind::Kv).unwrap();
            write.put(NAME, b"bad", b"value").unwrap();
            write.commit().unwrap();
            drop(owner);
            f.env = open(&f.directory);
            let read = begin(f.env, ptr::null_mut(), true);
            let mut cursor = ptr::null_mut();
            assert_eq!(usql_cursor_open(read, DBI, &mut cursor), 0);
            let mut id = 77;
            let mut value = val(b"sentinel");
            let previous = (value.mv_size, value.mv_data);
            assert_eq!(usql_cursor_row(cursor, &mut id, &mut value, 0), IO);
            assert_eq!(id, 77);
            assert_eq!((value.mv_size, value.mv_data), previous);
            usql_cursor_close(cursor);
            usql_txn_abort(read);
        }
    }
    #[test]
    fn namespace_replacement_and_foreign_process_fail_closed() {
        unsafe {
            let f = Fixture::new();
            let w = begin(f.env, ptr::null_mut(), false);
            create(w);
            assert_eq!(put(w, 1, b"parent"), 0);
            assert_eq!(usql_txn_commit(w), 0);
            let read = begin(f.env, ptr::null_mut(), true);
            let idle = begin(f.env, ptr::null_mut(), true);
            assert_eq!(usql_read_recycle(idle), 0);
            let child_path = f.directory.with_extension("child");
            let child_path_c = CString::new(child_path.as_os_str().as_encoded_bytes()).unwrap();
            let active_writer = begin(f.env, ptr::null_mut(), false);
            assert_eq!(put(active_writer, 2, b"pending"), 0);
            let mut cursor = ptr::null_mut();
            assert_eq!(usql_cursor_open(read, DBI, &mut cursor), 0);
            for raw in [false, true] {
                let child = if raw {
                    libc::syscall(libc::SYS_fork) as libc::pid_t
                } else {
                    libc::fork()
                };
                assert!(child >= 0);
                if child == 0 {
                    let mut out = val(&[]);
                    let mut k = val(&key(i64::MIN));
                    let wrong_handle = usql_get(read, DBI, &val(&key(1)), &mut out);
                    let wrong_writer = put(active_writer, 2, b"child");
                    let wrong_cursor = usql_cursor_get(cursor, &mut k, &mut out, 17);
                    let wrong_renew = usql_read_renew(f.env, idle);
                    let wrong_discard = usql_read_discard(idle);
                    let mut point_slot = idle;
                    let wrong_point = usql_read_point(f.env, &mut point_slot, 1, &mut out);
                    let mut row_id = 0;
                    let wrong_row = usql_cursor_row(cursor, &mut row_id, &mut out, 0);
                    let mut fresh = ptr::null_mut();
                    let created = usql_env_create(&mut fresh);
                    let refused = if created == 0 {
                        usql_env_open(fresh, child_path_c.as_ptr(), NOTLS, 0o600)
                    } else {
                        created
                    };
                    let untouched = !child_path.exists();
                    usql_txn_abort(active_writer);
                    usql_txn_abort(read);
                    usql_cursor_close(cursor);
                    usql_env_close(f.env); // must not release parent's lock
                    libc::_exit(
                        if wrong_handle == BAD_TXN
                            && wrong_writer == BAD_TXN
                            && wrong_cursor == BAD_TXN
                            && wrong_renew == BAD_TXN
                            && wrong_discard == BAD_TXN
                            && wrong_point == BAD_TXN
                            && wrong_row == BAD_TXN
                            && refused == BAD_TXN
                            && untouched
                        {
                            0
                        } else {
                            1
                        },
                    );
                }
                let mut status = 0;
                assert_eq!(libc::waitpid(child, &mut status, 0), child);
                assert_eq!(status, 0);
                assert_eq!(get(read, 1), Some(b"parent".to_vec()));
                assert_eq!(get(active_writer, 2), Some(b"pending".to_vec()));
            }
            usql_txn_abort(active_writer);
            assert_eq!(usql_read_discard(idle), 0);
            usql_cursor_close(cursor);
            usql_txn_abort(read);
            let moved = f.directory.with_extension("old");
            std::fs::rename(&f.directory, &moved).unwrap();
            std::fs::create_dir(&f.directory).unwrap();
            let mut other = ptr::null_mut();
            assert_eq!(usql_env_create(&mut other), 0);
            let path = CString::new(f.directory.as_os_str().as_encoded_bytes()).unwrap();
            assert_ne!(usql_env_open(other, path.as_ptr(), NOTLS, 0o600), 0);
            assert!(!f.directory.join("SNAPSHOTLOG").exists());
            usql_env_close(other);
            std::fs::remove_dir(&f.directory).unwrap();
            std::fs::rename(&moved, &f.directory).unwrap();
        }
    }
}
