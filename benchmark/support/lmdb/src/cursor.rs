use crate::{
    Database, DatabaseKind, Error, Result, check, ffi,
    transaction::{Transaction, bytes, val},
};
use std::marker::PhantomData;

/// Read-only Iterator adapter: each item borrows the cursor/transaction lifetime.
pub struct Cursor<'txn> {
    raw: *mut ffi::Cursor,
    max_key: usize,
    duplicates: bool,
    _txn: PhantomData<&'txn ()>,
}
impl<'txn> Cursor<'txn> {
    pub(crate) fn new<'env>(txn: &'txn Transaction<'env>, db: &Database<'env>) -> Result<Self> {
        let id = txn.database(db)?;
        let mut raw = std::ptr::null_mut();
        // SAFETY: cursor retains the transaction borrow and is closed before its transaction.
        check(unsafe { ffi::mdb_cursor_open(txn.raw, id, &mut raw) })?;
        if raw.is_null() {
            return Err(Error::InvalidInput("null LMDB cursor"));
        }
        Ok(Self {
            raw,
            max_key: txn.env.max_key_size(),
            duplicates: db.kind() == DatabaseKind::SortedDuplicates,
            _txn: PhantomData,
        })
    }
    pub fn first(&mut self) -> Result<Option<(&[u8], &[u8])>> {
        self.read(ffi::FIRST, &[], &[])
    }
    pub fn last(&mut self) -> Result<Option<(&[u8], &[u8])>> {
        self.read(ffi::LAST, &[], &[])
    }
    pub fn next_pair(&mut self) -> Result<Option<(&[u8], &[u8])>> {
        self.read(ffi::NEXT, &[], &[])
    }
    pub fn previous_pair(&mut self) -> Result<Option<(&[u8], &[u8])>> {
        self.read(ffi::PREV, &[], &[])
    }
    pub fn next_duplicate(&mut self) -> Result<Option<(&[u8], &[u8])>> {
        if !self.duplicates {
            return Err(Error::WrongKind);
        }
        self.read(ffi::NEXT_DUP, &[], &[])
    }
    pub fn next_key(&mut self) -> Result<Option<(&[u8], &[u8])>> {
        self.read(ffi::NEXT_NODUP, &[], &[])
    }
    pub fn lower_bound(&mut self, key: &[u8]) -> Result<Option<(&[u8], &[u8])>> {
        if key.is_empty() || key.len() > self.max_key {
            return Err(Error::InvalidInput("LMDB cursor seek key size unsupported"));
        }
        if !self.position(ffi::SET_RANGE, key, &[])? {
            return Ok(None);
        }
        self.read(ffi::GET_CURRENT, &[], &[])
    }
    pub fn seek_pair(&mut self, key: &[u8], value: &[u8]) -> Result<Option<(&[u8], &[u8])>> {
        if !self.duplicates {
            return Err(Error::WrongKind);
        }
        if key.is_empty()
            || key.len() > self.max_key
            || value.is_empty()
            || value.len() > self.max_key
        {
            return Err(Error::InvalidInput("LMDB duplicate seek size unsupported"));
        }
        if !self.position(ffi::GET_BOTH, key, value)? {
            return Ok(None);
        }
        self.read(ffi::GET_CURRENT, &[], &[])
    }
    fn position(&mut self, op: i32, key: &[u8], value: &[u8]) -> Result<bool> {
        let mut key = val(key);
        let mut value = val(value);
        // SAFETY: seek inputs live for this call; returned input aliases never escape.
        let code = unsafe { ffi::mdb_cursor_get(self.raw, &mut key, &mut value, op) };
        if code == ffi::NOTFOUND {
            Ok(false)
        } else {
            check(code)?;
            Ok(true)
        }
    }
    fn read(&mut self, op: i32, key: &[u8], value: &[u8]) -> Result<Option<(&[u8], &[u8])>> {
        let mut key = val(key);
        let mut value = val(value);
        // SAFETY: read-only cursor is live, and the pinned API only changes these Val structs.
        let code = unsafe { ffi::mdb_cursor_get(self.raw, &mut key, &mut value, op) };
        if code == ffi::NOTFOUND {
            return Ok(None);
        }
        check(code)?;
        // SAFETY: mapped output bytes live through the borrowed cursor's read transaction.
        Ok(Some((unsafe { bytes(key) }?, unsafe { bytes(value) }?)))
    }
}
impl Drop for Cursor<'_> {
    fn drop(&mut self) {
        unsafe { ffi::mdb_cursor_close(self.raw) };
    }
}
