use crate::thread_slot::Slot;
use crate::{Cursor, Environment, Error, Result, check, ffi};
use std::{cell::Cell, ffi::CString, marker::PhantomData, rc::Rc};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatabaseKind {
    Ordinary,
    SortedDuplicates,
}
#[derive(Clone, Copy)]
enum Status {
    Pending(u64),
    Committed,
    Aborted,
}
struct DatabaseState {
    id: u32,
    kind: DatabaseKind,
    status: Cell<Status>,
}

/// A handle has environment ownership and cannot become valid after an aborted open.
#[derive(Clone)]
pub struct Database<'env> {
    env: &'env Environment,
    state: Rc<DatabaseState>,
}
impl Database<'_> {
    pub fn kind(&self) -> DatabaseKind {
        self.state.kind
    }
}

pub(crate) struct Transaction<'env> {
    pub(crate) env: &'env Environment,
    pub(crate) raw: *mut ffi::Txn,
    id: u64,
    opened: Vec<Rc<DatabaseState>>,
    _slot: Slot,
}
impl<'env> Transaction<'env> {
    pub(crate) fn new(env: &'env Environment, readonly: bool) -> Result<Self> {
        // Reserve before native begin. Every early return/error drops this guard.
        // A second writer must fail here rather than deadlock in LMDB's writer mutex.
        let slot = Slot::acquire(readonly && env.notls_reads)?;
        let id = env.next_transaction.get();
        env.next_transaction
            .set(id.checked_add(1).ok_or(Error::ExhaustedIdentity)?);
        let mut raw = std::ptr::null_mut();
        // SAFETY: env is owned/live; parent is null (no nested write transactions).
        check(unsafe {
            ffi::mdb_txn_begin(
                env.raw,
                std::ptr::null_mut(),
                if readonly { ffi::RDONLY } else { 0 },
                &mut raw,
            )
        })?;
        if raw.is_null() {
            return Err(Error::InvalidInput("LMDB returned null transaction"));
        }
        Ok(Self {
            env,
            raw,
            id,
            opened: Vec::new(),
            _slot: slot,
        })
    }
    pub(crate) fn database(&self, db: &Database<'env>) -> Result<u32> {
        if !std::ptr::eq(self.env, db.env) {
            return Err(Error::StaleDatabase);
        }
        match db.state.status.get() {
            Status::Committed => Ok(db.state.id),
            Status::Pending(id) if id == self.id => Ok(db.state.id),
            _ => Err(Error::StaleDatabase),
        }
    }
    fn get<'a>(&'a self, db: &Database<'env>, key: &[u8]) -> Result<Option<&'a [u8]>> {
        let id = self.database(db)?;
        validate_key(self.env, key)?;
        let mut key = val(key);
        let mut data = val(&[]);
        // SAFETY: all pointers refer to live transaction/input/out structs; get does not mutate input bytes.
        let code = unsafe { ffi::mdb_get(self.raw, id, &mut key, &mut data) };
        if code == ffi::NOTFOUND {
            return Ok(None);
        }
        check(code)?;
        // SAFETY: mapped data lives through this transaction; the public return borrows self.
        unsafe { bytes(data) }.map(Some)
    }
}
impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: this transaction still owns the uncommitted handle exactly once.
            unsafe { ffi::mdb_txn_abort(self.raw) };
            for state in &self.opened {
                state.status.set(Status::Aborted);
            }
        }
    }
}

pub struct ReadTransaction<'env> {
    pub(crate) inner: Transaction<'env>,
}
impl<'env> ReadTransaction<'env> {
    /// Resolve ownership/DBI once for a fixed-snapshot point-read batch.
    pub fn database<'a>(&'a self, db: &Database<'env>) -> Result<ReadDatabase<'a>> {
        Ok(ReadDatabase {
            raw: self.inner.raw,
            id: self.inner.database(db)?,
            max_key: self.inner.env.max_key_size(),
            _txn: PhantomData,
        })
    }
    pub fn get<'a>(&'a self, db: &Database<'env>, key: &[u8]) -> Result<Option<&'a [u8]>> {
        self.inner.get(db, key)
    }
    pub fn cursor<'a>(&'a self, db: &Database<'env>) -> Result<Cursor<'a>> {
        Cursor::new(&self.inner, db)
    }
}

/// Prepared mapped reader; no Rc or DB-handle lookup in its per-key path.
pub struct ReadDatabase<'txn> {
    raw: *mut ffi::Txn,
    id: u32,
    max_key: usize,
    _txn: PhantomData<&'txn ()>,
}
impl ReadDatabase<'_> {
    pub fn get<'a>(&'a self, key: &[u8]) -> Result<Option<&'a [u8]>> {
        if key.is_empty() || key.len() > self.max_key {
            return Err(Error::InvalidInput("LMDB keys require 1..=max_key bytes"));
        }
        let mut key = val(key);
        let mut data = val(&[]);
        // SAFETY: construction pinned a valid DBI and read transaction for this view's lifetime.
        let code = unsafe { ffi::mdb_get(self.raw, self.id, &mut key, &mut data) };
        if code == ffi::NOTFOUND {
            return Ok(None);
        }
        check(code)?;
        // SAFETY: the read snapshot is immutable and this return borrows the prepared view.
        unsafe { bytes(data) }.map(Some)
    }
}

pub struct WriteTransaction<'env> {
    pub(crate) inner: Transaction<'env>,
}
impl<'env> WriteTransaction<'env> {
    pub fn open_database(
        &mut self,
        name: &str,
        kind: DatabaseKind,
        create: bool,
    ) -> Result<Database<'env>> {
        if name.is_empty() || name.len() > self.inner.env.max_key_size() {
            return Err(Error::InvalidInput(
                "named database name length unsupported by LMDB",
            ));
        }
        let name = CString::new(name)
            .map_err(|_| Error::InvalidInput("LMDB named databases do not accept embedded NUL"))?;
        let mut id = 0;
        let flags = if create { ffi::CREATE } else { 0 }
            | if kind == DatabaseKind::SortedDuplicates {
                ffi::DUPSORT
            } else {
                0
            };
        // SAFETY: writer exclusive methods own a valid txn; name/out pointers are live.
        check(unsafe { ffi::mdb_dbi_open(self.inner.raw, name.as_ptr(), flags, &mut id) })?;
        let mut actual = 0;
        // SAFETY: the DBI was opened in this live transaction.
        check(unsafe { ffi::mdb_dbi_flags(self.inner.raw, id, &mut actual) })?;
        if (actual & ffi::DUPSORT != 0) != (kind == DatabaseKind::SortedDuplicates) {
            return Err(Error::WrongKind);
        }
        let state = Rc::new(DatabaseState {
            id,
            kind,
            status: Cell::new(Status::Pending(self.inner.id)),
        });
        self.inner.opened.push(state.clone());
        Ok(Database {
            env: self.inner.env,
            state,
        })
    }
    pub fn get<'a>(&'a self, db: &Database<'env>, key: &[u8]) -> Result<Option<&'a [u8]>> {
        self.inner.get(db, key)
    }
    pub fn put(&mut self, db: &Database<'env>, key: &[u8], value: &[u8]) -> Result<()> {
        if db.state.kind != DatabaseKind::Ordinary {
            return Err(Error::WrongKind);
        }
        self.put_raw(db, key, value, 0).map(|_| ())
    }
    pub fn insert_duplicate(
        &mut self,
        db: &Database<'env>,
        key: &[u8],
        value: &[u8],
    ) -> Result<bool> {
        if db.state.kind != DatabaseKind::SortedDuplicates {
            return Err(Error::WrongKind);
        }
        if value.is_empty() || value.len() > self.inner.env.max_key_size() {
            return Err(Error::InvalidInput(
                "LMDB duplicate values require 1..=max_key bytes",
            ));
        }
        self.put_raw(db, key, value, ffi::NODUPDATA)
    }
    fn put_raw(
        &mut self,
        db: &Database<'env>,
        key: &[u8],
        value: &[u8],
        flags: u32,
    ) -> Result<bool> {
        let id = self.inner.database(db)?;
        validate_key(self.inner.env, key)?;
        let mut key = val(key);
        let mut value = val(value);
        // SAFETY: mutable writer borrow prevents invalidating any returned borrowed value;
        // LMDB copies these caller-owned bytes and does not mutate them with these flags.
        let code = unsafe { ffi::mdb_put(self.inner.raw, id, &mut key, &mut value, flags) };
        if code == ffi::KEYEXIST && flags == ffi::NODUPDATA {
            return Ok(false);
        }
        check(code)?;
        Ok(true)
    }
    pub fn commit(mut self) -> Result<()> {
        let raw = self.inner.raw;
        self.inner.raw = std::ptr::null_mut();
        // SAFETY: commit consumes/frees the handle, including on an error; Drop cannot abort it again.
        let code = unsafe { ffi::mdb_txn_commit(raw) };
        for state in &self.inner.opened {
            state.status.set(if code == 0 {
                Status::Committed
            } else {
                Status::Aborted
            });
        }
        check(code)
    }
    pub fn abort(self) {}
}

pub(crate) fn val(bytes: &[u8]) -> ffi::Val {
    ffi::Val {
        size: bytes.len(),
        data: bytes.as_ptr().cast_mut().cast(),
    }
}
pub(crate) fn validate_key(env: &Environment, key: &[u8]) -> Result<()> {
    if key.is_empty() || key.len() > env.max_key_size() {
        return Err(Error::InvalidInput("LMDB keys require 1..=max_key bytes"));
    }
    Ok(())
}
pub(crate) unsafe fn bytes<'a>(value: ffi::Val) -> Result<&'a [u8]> {
    if value.size == 0 {
        return Ok(&[]);
    }
    if value.data.is_null() || value.size > isize::MAX as usize {
        return Err(Error::InvalidInput("invalid borrowed LMDB byte range"));
    }
    // SAFETY: caller binds the returned borrow to the valid transaction/cursor.
    Ok(unsafe { std::slice::from_raw_parts(value.data.cast(), value.size) })
}
