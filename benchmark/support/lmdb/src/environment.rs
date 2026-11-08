use crate::{
    Error, Result, check, ffi,
    transaction::{ReadTransaction, Transaction, WriteTransaction},
};
use std::{
    cell::Cell,
    collections::BTreeSet,
    ffi::CString,
    marker::PhantomData,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::Path,
    rc::Rc,
    sync::{Mutex, OnceLock},
};

type Identity = (u64, u64);
static OPEN: OnceLock<Mutex<BTreeSet<Identity>>> = OnceLock::new();
struct Reservation(Identity);
impl Reservation {
    fn acquire(path: &Path) -> Result<Self> {
        let metadata = std::fs::metadata(path)?;
        if !metadata.is_dir() {
            return Err(Error::InvalidInput("LMDB environment must be a directory"));
        }
        let identity = (metadata.dev(), metadata.ino());
        let mut set = OPEN
            .get_or_init(|| Mutex::new(BTreeSet::new()))
            .lock()
            .map_err(|_| Error::InvalidInput("open registry poisoned"))?;
        if !set.insert(identity) {
            return Err(Error::AlreadyOpen);
        }
        Ok(Self(identity))
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut set) = OPEN.get().unwrap().lock() {
            set.remove(&self.0);
        }
    }
}

/// One thread-affine handle per environment inode, with normal LMDB locking/durability.
pub struct Environment {
    pub(crate) raw: *mut ffi::Env,
    pub(crate) next_transaction: Cell<u64>,
    pub(crate) notls_reads: bool,
    flags: u32,
    max_key: usize,
    _reservation: Reservation,
    _thread: PhantomData<Rc<()>>,
}

impl Environment {
    pub fn open(path: &Path, map_bytes: usize) -> Result<Self> {
        Self::open_mode(path, map_bytes, false, false)
    }
    /// Explicit smoke-only configuration permitting retained readers plus a writer.
    /// Normal locking and commit synchronization remain enabled.
    pub fn open_notls(path: &Path, map_bytes: usize) -> Result<Self> {
        Self::open_mode(path, map_bytes, true, false)
    }
    pub fn open_read_only(path: &Path, map_bytes: usize) -> Result<Self> {
        Self::open_mode(path, map_bytes, false, true)
    }
    fn open_mode(
        path: &Path,
        map_bytes: usize,
        notls_reads: bool,
        read_only: bool,
    ) -> Result<Self> {
        if map_bytes < 1_048_576 {
            return Err(Error::InvalidInput("comparison map must be at least 1 MiB"));
        }
        let path = path.canonicalize()?;
        let name = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| Error::InvalidInput("NUL in environment path"))?;
        let reservation = Reservation::acquire(&path)?;
        let mut raw = std::ptr::null_mut();
        // SAFETY: valid out pointer; subsequent calls own this environment until close.
        check(unsafe { ffi::mdb_env_create(&mut raw) })?;
        if raw.is_null() {
            return Err(Error::InvalidInput("LMDB returned null environment"));
        }
        let mut env = Self {
            raw,
            next_transaction: Cell::new(1),
            notls_reads,
            flags: if notls_reads { ffi::NOTLS } else { 0 }
                | if read_only { ffi::RDONLY } else { 0 },
            max_key: 0,
            _reservation: reservation,
            _thread: PhantomData,
        };
        // SAFETY: newly created owned handle, called before open. No relaxed flags.
        unsafe {
            check(ffi::mdb_env_set_mapsize(raw, map_bytes))?;
            check(ffi::mdb_env_set_maxdbs(raw, 64))?;
            check(ffi::mdb_env_open(raw, name.as_ptr(), env.flags, 0o600))?;
            let max = ffi::mdb_env_get_maxkeysize(raw);
            if max <= 0 {
                return Err(Error::InvalidInput("invalid LMDB maximum key size"));
            }
            env.max_key = max as usize;
        }
        Ok(env)
    }
    pub fn max_key_size(&self) -> usize {
        self.max_key
    }
    pub fn flags(&self) -> u32 {
        self.flags
    }
    pub fn begin_read(&self) -> Result<ReadTransaction<'_>> {
        Ok(ReadTransaction {
            inner: Transaction::new(self, true)?,
        })
    }
    pub fn begin_write(&self) -> Result<WriteTransaction<'_>> {
        Ok(WriteTransaction {
            inner: Transaction::new(self, false)?,
        })
    }
}
impl Drop for Environment {
    fn drop(&mut self) {
        // SAFETY: transaction/DB borrows prevent use after this owner ends.
        unsafe { ffi::mdb_env_close(self.raw) };
    }
}
