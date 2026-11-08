//! LMDB's public main-cursor catalog is synthesized from native database names.
//! Descriptors stay opaque to applications; native storage remains unchanged.
use super::*;

pub(super) struct CatalogRow {
    pub(super) pair: Pair,
    handle: DatabaseHandle,
}
pub(super) struct CatalogIndex {
    pub(super) rows: Vec<CatalogRow>,
}
impl MDB_txn {
    pub(super) fn catalog_index(&self) -> FfiResult<Arc<CatalogIndex>> {
        if !self.readonly {
            return Err(BAD_TXN);
        }
        if let Some(index) = self.catalog_index.lock().map_err(|_| BAD_TXN)?.clone() {
            return Ok(index);
        }
        // No mutex is held while consulting Native storage or metadata. Byte
        // sorting is intrinsic, so installing a comparator cannot stale it.
        let mut rows = self.catalog_rows()?;
        rows.sort_unstable_by(|a, b| a.pair.0.cmp(&b.pair.0));
        let index = Arc::new(CatalogIndex { rows });
        let mut stored = self.catalog_index.lock().map_err(|_| BAD_TXN)?;
        Ok(stored.get_or_insert(index).clone())
    }
    pub(super) fn database_names(&self) -> FfiResult<Vec<Vec<u8>>> {
        match self.state.as_ref() {
            Some(State::Read(r)) => r.database_names(),
            Some(State::PrivateRead(r)) => r.database_names(),
            Some(State::Write(w)) => w.database_names(),
            Some(State::Child(w)) => w.database_names(),
            Some(State::Prepared(_)) | None => return Err(BAD_TXN),
        }
        .map_err(code)
    }
    pub(super) fn main_alias(&self) -> FfiResult<AliasHandle> {
        self.shared
            .handles
            .lock()
            .map_err(|_| BAD_TXN)?
            .get(1)
            .and_then(Option::as_ref)
            .cloned()
            .ok_or(BAD_DBI)
    }
    pub(super) fn native_main_seek(
        &self,
        bound: Option<(&[u8], Option<&[u8]>)>,
        inclusive: bool,
    ) -> FfiResult<Option<(&[u8], &[u8])>> {
        let after = bound.and_then(|(key, value)| value.map(|value| (key, value)));
        let _ = inclusive; // internal callers resume exact stored pairs, never range-seek.
        match self.state.as_ref() {
            Some(State::Read(r)) => r.structural_resume_pair(MAIN, after),
            Some(State::PrivateRead(r)) => r.structural_resume_pair(MAIN, after),
            Some(State::Write(w)) => w.structural_resume_pair(MAIN, after),
            Some(State::Child(w)) => w.structural_resume_pair(MAIN, after),
            _ => return Err(BAD_TXN),
        }
        .or_else(|error| {
            if matches!(error, Error::MissingDatabase) {
                Ok(None)
            } else {
                Err(error)
            }
        })
        .map_err(code)
    }
    pub(super) fn visible_main_empty(&self) -> FfiResult<bool> {
        if self.native_main_seek(None, false)?.is_some() {
            return Ok(false);
        }
        for name in self.database_names()? {
            let actual = self.handle_by_name(&name)?;
            if self.catalog_hidden(NativeId::of(&actual))? {
                continue;
            }
            if self.external_database_name(&name)?.is_some() {
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub(super) fn visible_named(&self, requested: &[u8]) -> FfiResult<alias::VisibleName> {
        // Validate any existing reverse mapping before considering a public
        // capability's known input name. Missing maps never authorize retarget.
        let physical = self.resolve_database_name(requested)?;
        let mut key = requested.to_vec();
        if self.shared.catalog_terminator {
            key.push(0);
        }
        let main = self.main_alias()?;
        if !self.custom_key(&main) && self.db_flags(&main)? & (DUPSORT | 8) == 0 {
            // Default/reverse comparison has exact byte equality. Authenticate
            // the ordinary row first, then resolve a genuine catalog handle;
            // this role needs neither a user-row scan nor row materialization.
            if self.raw_get(MAIN, &key)?.is_some() {
                return Ok(alias::VisibleName::Ordinary);
            }
            return match operations::current_handle(self, &physical) {
                Ok(actual) if self.catalog_hidden(NativeId::of(&actual))? => {
                    Ok(alias::VisibleName::Absent)
                }
                Ok(actual) => Ok(alias::VisibleName::Named(actual)),
                Err(Error::MissingDatabase) => Ok(alias::VisibleName::Absent),
                Err(error) => Err(code(error)),
            };
        }
        let mut previous = None;
        loop {
            let Some((stored, value)) = self.native_main_seek(previous, false)? else {
                break;
            };
            if self.key_order(&main, stored, &key).is_eq() {
                return Ok(alias::VisibleName::Ordinary);
            }
            previous = Some((stored, Some(value)));
        }
        for name in self.database_names()? {
            let actual = self.handle_by_name(&name)?;
            if self.catalog_hidden(NativeId::of(&actual))? {
                continue;
            }
            let Some(mut external) = self.external_database_name(&name)? else {
                continue;
            };
            if self.shared.catalog_terminator {
                external.push(0);
            }
            if self.key_order(&main, &external, &key).is_eq() {
                return Ok(alias::VisibleName::Named(actual));
            }
        }
        Ok(alias::VisibleName::Absent)
    }
    pub(super) fn catalog_handle_for_key(&self, key: &[u8]) -> FfiResult<Option<DatabaseHandle>> {
        let main = self.main_alias()?;
        if self.readonly && !self.custom_key(&main) && self.db_flags(&main)? & 8 == 0 {
            let index = self.catalog_index()?;
            return Ok(index
                .rows
                .binary_search_by(|row| row.pair.0.as_slice().cmp(key))
                .ok()
                .map(|at| index.rows[at].handle.clone()));
        }
        match self.catalog_handle_for_key_unmasked(key)? {
            Some(actual) if self.catalog_hidden(NativeId::of(&actual))? => Ok(None),
            result => Ok(result),
        }
    }
    pub(super) fn catalog_handle_for_key_unmasked(
        &self,
        key: &[u8],
    ) -> FfiResult<Option<DatabaseHandle>> {
        let main = self.main_alias()?;
        for name in self.database_names()? {
            let Some(mut external) = self.external_database_name(&name)? else {
                continue;
            };
            if self.shared.catalog_terminator {
                external.push(0);
            }
            if self.key_order(&main, &external, key).is_eq() {
                return self.handle_by_name(&name).map(Some);
            }
        }
        Ok(None)
    }
    pub(super) fn missing_named_code(&self, alias: &AliasHandle) -> FfiResult<c_int> {
        if alias.is_main() {
            return Ok(NOTFOUND);
        }
        if self
            .shared
            .rollback_invalid
            .lock()
            .map_err(|_| BAD_TXN)?
            .contains(&alias.native_id())
        {
            return Ok(BAD_DBI);
        }
        Ok(self
            .shared
            .profile
            .missing_stale(self.visible_main_empty()?))
    }
    fn catalog_rows(&self) -> FfiResult<Vec<CatalogRow>> {
        let mut rows = Vec::new();
        for name in self.database_names()? {
            let Some(external) = self.external_database_name(&name)? else {
                continue;
            };
            let h = self.handle_by_name(&name)?;
            if self.catalog_hidden(NativeId::of(&h))? {
                continue;
            }
            let mut key = external;
            if self.shared.catalog_terminator {
                key.push(0);
            }
            // MDB_db is private to LMDB; expose its ABI-sized opaque descriptor
            // with the persisted public flags. Physical page addresses are not
            // meaningful in the native multi-file store.
            let mut descriptor = vec![0u8; 8 + 5 * std::mem::size_of::<usize>()];
            descriptor[4..6]
                .copy_from_slice(&(self.native_settings(&h)?.flags as u16).to_ne_bytes());
            let last = descriptor.len() - std::mem::size_of::<usize>();
            descriptor[last..].copy_from_slice(&usize::MAX.to_ne_bytes());
            rows.push(CatalogRow {
                pair: (key, descriptor),
                handle: h,
            });
        }
        Ok(rows)
    }
    pub(super) fn catalog_pairs(&self) -> FfiResult<Vec<Pair>> {
        if self.readonly {
            return Ok(self
                .catalog_index()?
                .rows
                .iter()
                .map(|row| row.pair.clone())
                .collect());
        }
        Ok(self
            .catalog_rows()?
            .into_iter()
            .map(|row| row.pair)
            .collect())
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_cursor_is_db(cursor: *mut MDB_cursor) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| -> FfiResult<bool> {
        let c = unsafe { get_ref(cursor)? };
        let t = unsafe { get_ref(c.txn)? };
        t.live()?;
        if t.peek_handle(c.dbi)?.name() != MAIN {
            return Ok(false);
        }
        let Some((key, _)) = &c.position else {
            return Ok(false);
        };
        Ok(t.catalog_handle_for_key(key)?.is_some())
    }));
    i32::from(matches!(result, Ok(Ok(true))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::CString, path::PathBuf};
    struct Fixture {
        env: *mut MDB_env,
        path: PathBuf,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            unsafe { environment::mdb_env_close(self.env) };
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
    fn begin(env: *mut MDB_env, flags: u32) -> *mut MDB_txn {
        let mut txn = ptr::null_mut();
        assert_eq!(
            unsafe { transaction::mdb_txn_begin(env, ptr::null_mut(), flags, &mut txn) },
            0
        );
        txn
    }
    fn create(txn: *mut MDB_txn, name: &str) {
        let name = CString::new(name).unwrap();
        let mut dbi = 0;
        assert_eq!(
            unsafe { operations::mdb_dbi_open(txn, name.as_ptr(), CREATE, &mut dbi) },
            0
        );
    }
    #[test]
    fn catalog_index_keeps_snapshot_and_reset_renew_captures_new_catalog() {
        let path = std::env::temp_dir().join(format!(
            "c-catalog-index-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        let mut env = ptr::null_mut();
        assert_eq!(unsafe { environment::mdb_env_create(&mut env) }, 0);
        let fixture = Fixture { env, path };
        assert_eq!(unsafe { environment::mdb_env_set_maxdbs(env, 16) }, 0);
        assert_eq!(
            unsafe { environment::mdb_env_open(env, name.as_ptr(), 0x200000, 0o600) },
            0
        );
        let writer = begin(env, 0);
        create(writer, "alpha");
        assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
        let reader = begin(env, RDONLY);
        let old_index = unsafe { &*reader }.catalog_index().unwrap();
        assert_eq!(old_index.rows.len(), 1);
        assert_eq!(
            old_index.rows[0]
                .pair
                .0
                .strip_suffix(&[0])
                .unwrap_or(&old_index.rows[0].pair.0),
            b"alpha"
        );
        let weak = Arc::downgrade(&old_index);
        drop(old_index);
        let mut cursor = ptr::null_mut();
        assert_eq!(
            unsafe { cursor::mdb_cursor_open(reader, 1, &mut cursor) },
            0
        );
        let mut key = MDB_val {
            mv_size: 0,
            mv_data: ptr::null_mut(),
        };
        let mut value = key;
        assert_eq!(
            unsafe { cursor::mdb_cursor_get(cursor, &mut key, &mut value, 0) },
            0
        );
        let first = key;
        for _ in 0..128 {
            assert_eq!(
                unsafe { cursor::mdb_cursor_get(cursor, &mut key, &mut value, 0) },
                0
            );
        }
        assert!(
            unsafe { &*reader }.retired_rows.is_empty(),
            "read-only catalog walks retain one metadata index"
        );
        assert_eq!(
            unsafe { operations::mdb_set_compare(reader, 1, ptr::null_mut()) },
            0
        );
        // Callback registration changes the query strategy but must not free
        // prior read outputs. Weak proves ownership without masking a release.
        assert!(weak.strong_count() > 0);
        let first_bytes =
            unsafe { std::slice::from_raw_parts(first.mv_data.cast::<u8>(), first.mv_size) };
        assert!(first_bytes.starts_with(b"alpha"));
        unsafe { cursor::mdb_cursor_close(cursor) };
        let old_index = weak.upgrade().unwrap();
        let writer = begin(env, 0);
        // Writers consult current transactional metadata rather than the read
        // snapshot's index, including before they have committed a new name.
        assert_eq!(unsafe { &*writer }.catalog_pairs().unwrap().len(), 1);
        create(writer, "bravo");
        assert_eq!(unsafe { &*writer }.catalog_pairs().unwrap().len(), 2);
        assert!(unsafe { &*writer }.catalog_index.lock().unwrap().is_none());
        assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
        let still_old = unsafe { &*reader }.catalog_index().unwrap();
        assert!(Arc::ptr_eq(&old_index, &still_old));
        assert_eq!(still_old.rows.len(), 1);
        unsafe { transaction::mdb_txn_reset(reader) };
        assert!(unsafe { &*reader }.catalog_index.lock().unwrap().is_none());
        assert_eq!(unsafe { transaction::mdb_txn_renew(reader) }, 0);
        let renewed = unsafe { &*reader }.catalog_index().unwrap();
        assert!(!Arc::ptr_eq(&old_index, &renewed));
        assert_eq!(renewed.rows.len(), 2);
        assert_eq!(
            renewed.rows[1]
                .pair
                .0
                .strip_suffix(&[0])
                .unwrap_or(&renewed.rows[1].pair.0),
            b"bravo"
        );
        unsafe { transaction::mdb_txn_abort(reader) };
        drop(fixture);
    }
}
