//! External LMDB names use native names directly up to the native bound. Longer
//! names use collision-checked physical aliases; the reverse map is transactional.
use super::*;
use sha2::{Digest, Sha256};
const ALIAS_PREFIX: &[u8] = b"\0lmdb-name:";
pub(super) fn is_alias(name: &[u8]) -> bool {
    name.starts_with(ALIAS_PREFIX)
}
fn mapping_key(physical: &[u8]) -> Vec<u8> {
    let mut key = b"name:".to_vec();
    key.extend_from_slice(physical);
    key
}
/// The modern catalog stores the terminating NUL in its MAIN key. Legacy
/// catalogs exclude it. The external spelling must fit that selected key.
pub(super) fn external_name_limit(shared: &Shared) -> usize {
    shared
        .max_key_size()
        .saturating_sub(usize::from(shared.catalog_terminator))
}
/// Delta admission uses the same checked public spelling as live C quotas.
pub(super) fn external_name_bytes(
    read: &ReadTxn,
    physical: &[u8],
    shared: &Shared,
) -> FfiResult<usize> {
    if !is_alias(physical) {
        return Ok(physical.len());
    }
    let external = match read.get(compat_features::META, &mapping_key(physical)) {
        Ok(Some(external)) => external,
        Ok(None) | Err(Error::MissingDatabase) => return Err(-30796),
        Err(error) => return Err(code(error)),
    };
    if (external.len() <= 255 && !(external.is_empty() && shared.catalog_terminator))
        || external.len() > external_name_limit(shared)
        || external.contains(&0)
    {
        return Err(-30796);
    }
    let mut expected = ALIAS_PREFIX.to_vec();
    expected.extend_from_slice(&Sha256::digest(external));
    if expected != physical {
        return Err(-30796);
    }
    Ok(external.len())
}
impl MDB_txn {
    /// LMDB supplies a NUL-terminated readable string. Stop at the selected
    /// bound before borrowing/copying it; no unbounded strlen or allocation.
    pub(super) unsafe fn checked_database_name<'a>(&self, p: *const c_char) -> FfiResult<&'a [u8]> {
        if p.is_null() {
            return Ok(MAIN);
        }
        let limit = external_name_limit(&self.shared);
        for len in 0..=limit {
            // SAFETY: caller supplies a C string; scan at most limit+1 bytes.
            if unsafe { *p.add(len) } == 0 {
                return Ok(unsafe { std::slice::from_raw_parts(p.cast::<u8>(), len) });
            }
        }
        Err(BAD_VALSIZE)
    }
    pub(super) fn resolve_database_name(&self, external: &[u8]) -> FfiResult<Vec<u8>> {
        if external == MAIN {
            return Ok(MAIN.to_vec());
        }
        let limit = external_name_limit(&self.shared);
        if (external.is_empty() && !self.shared.catalog_terminator)
            || external.len() > limit
            || external.contains(&0)
        {
            return Err(BAD_VALSIZE);
        }
        if !external.is_empty() && external.len() <= 255 {
            return Ok(external.to_vec());
        }
        let mut physical = ALIAS_PREFIX.to_vec();
        physical.extend_from_slice(&Sha256::digest(external));
        match self.raw_get(compat_features::META, &mapping_key(&physical))? {
            Some(stored) if stored != external => Err(-30796),
            Some(_) => Ok(physical),
            None => match self.handle_by_name(&physical) {
                Ok(_) => Err(-30796),
                Err(NOTFOUND) => Ok(physical),
                Err(e) => Err(e),
            },
        }
    }
    pub(super) fn record_database_name(
        &mut self,
        external: &[u8],
        physical: &[u8],
    ) -> FfiResult<()> {
        if !is_alias(physical) {
            return Ok(());
        }
        if let Some(stored) = self.raw_get(compat_features::META, &mapping_key(physical))? {
            return if stored == external {
                Ok(())
            } else {
                Err(-30796)
            };
        }
        // The database itself must exist before its public name is installed.
        self.handle_by_name(physical)?;
        let writer = self.writer()?;
        if writer.open_database(compat_features::META).is_err() {
            writer
                .create_database(compat_features::META, DatabaseKind::Kv)
                .map_err(code)?;
        }
        writer
            .put(compat_features::META, &mapping_key(physical), external)
            .map_err(code)
    }
    pub(super) fn external_database_name(&self, physical: &[u8]) -> FfiResult<Option<Vec<u8>>> {
        if is_alias(physical) {
            let external = self
                .raw_get(compat_features::META, &mapping_key(physical))?
                .ok_or(-30796)?;
            if (external.len() <= 255 && !(external.is_empty() && self.shared.catalog_terminator))
                || external.len() > external_name_limit(&self.shared)
                || external.contains(&0)
            {
                return Err(-30796);
            }
            if self.resolve_database_name(external)? != physical {
                return Err(-30796);
            }
            Ok(Some(external.to_vec()))
        } else if physical.first().is_some_and(|b| *b != 0) {
            Ok(Some(physical.to_vec()))
        } else {
            Ok(None)
        }
    }
    pub(super) fn remove_database_name(&mut self, physical: &[u8]) -> FfiResult<()> {
        if !is_alias(physical) {
            return Ok(());
        }
        self.writer()?
            .delete(compat_features::META, &mapping_key(physical))
            .map_err(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_name_boundaries_preserve_modern_empty_and_legacy_limits() {
        for (profile, pages, expected, empty_allowed) in [
            ("0.9.31\n", 4096, 511usize, false),
            ("1.0\n", 8192, 4025usize, true),
        ] {
            let root = std::env::temp_dir().join(format!(
                "ulmdb-selected-names-{}",
                ulmdb_storage::new_identity().unwrap()
            ));
            drop(
                Environment::create_with_page_size(
                    &root,
                    ulmdb::PublicationMode::Legacy,
                    ulmdb::PageSize::new(pages as usize).unwrap(),
                )
                .unwrap(),
            );
            std::fs::write(root.join("LMDB-ABI"), profile).unwrap();
            let path = std::ffi::CString::new(root.as_os_str().as_encoded_bytes()).unwrap();
            let accepted = std::ffi::CString::new(vec![b'n'; expected]).unwrap();
            let rejected = std::ffi::CString::new(vec![b'x'; expected + 1]).unwrap();
            let empty = c"";
            let mut env = ptr::null_mut();
            unsafe {
                assert_eq!(environment::mdb_env_create(&mut env), 0);
                assert_eq!(environment::mdb_env_set_maxdbs(env, 8), 0);
                assert_eq!(environment::mdb_env_set_pagesize(env, pages), 0);
                assert_eq!(environment::mdb_env_open(env, path.as_ptr(), 0, 0o600), 0);
                assert_eq!(
                    environment::mdb_env_get_maxkeysize(env) as usize,
                    expected + usize::from(empty_allowed)
                );
                let mut txn = ptr::null_mut();
                let mut dbi = 0;
                assert_eq!(
                    transaction::mdb_txn_begin(env, ptr::null_mut(), 0, &mut txn),
                    0
                );
                assert_eq!(
                    operations::mdb_dbi_open(txn, accepted.as_ptr(), CREATE, &mut dbi),
                    0
                );
                let mut key = MDB_val {
                    mv_size: 1,
                    mv_data: b"k".as_ptr().cast_mut().cast(),
                };
                let mut value = MDB_val {
                    mv_size: 1,
                    mv_data: b"V".as_ptr().cast_mut().cast(),
                };
                assert_eq!(operations::mdb_put(txn, dbi, &mut key, &mut value, 0), 0);
                assert_eq!(transaction::mdb_txn_commit(txn), 0);
                for (name, expected_rc) in [
                    (rejected.as_c_str(), BAD_VALSIZE),
                    (empty, if empty_allowed { 0 } else { BAD_VALSIZE }),
                ] {
                    assert_eq!(
                        transaction::mdb_txn_begin(env, ptr::null_mut(), 0, &mut txn),
                        0
                    );
                    let rc = operations::mdb_dbi_open(txn, name.as_ptr(), CREATE, &mut dbi);
                    assert_eq!(rc, expected_rc);
                    if rc == 0 {
                        assert_ne!(dbi, 1, "empty spelling is never MAIN");
                        assert_eq!(operations::mdb_put(txn, dbi, &mut key, &mut value, 0), 0);
                        assert_eq!(transaction::mdb_txn_commit(txn), 0);
                    } else {
                        transaction::mdb_txn_abort(txn);
                    }
                }
                environment::mdb_env_close(env);
                assert_eq!(environment::mdb_env_create(&mut env), 0);
                assert_eq!(environment::mdb_env_set_maxdbs(env, 8), 0);
                assert_eq!(environment::mdb_env_open(env, path.as_ptr(), 0, 0o600), 0);
                assert_eq!(
                    transaction::mdb_txn_begin(env, ptr::null_mut(), RDONLY, &mut txn),
                    0
                );
                assert_eq!(
                    operations::mdb_dbi_open(txn, accepted.as_ptr(), 0, &mut dbi),
                    0
                );
                assert_eq!(operations::mdb_get(txn, dbi, &mut key, &mut value), 0);
                assert_eq!(
                    std::slice::from_raw_parts(value.mv_data.cast::<u8>(), value.mv_size),
                    b"V"
                );
                if empty_allowed {
                    assert_eq!(
                        operations::mdb_dbi_open(txn, empty.as_ptr(), 0, &mut dbi),
                        0
                    );
                    assert_ne!(dbi, 1);
                    assert_eq!(operations::mdb_get(txn, dbi, &mut key, &mut value), 0);
                }
                transaction::mdb_txn_abort(txn);
                environment::mdb_env_close(env);
            }
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn corrupted_reverse_mapping_never_opens_another_database() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "ulmdb-name-collision-{}-{nonce}",
            std::process::id()
        ));
        let path = std::ffi::CString::new(directory.to_str().unwrap()).unwrap();
        let external = vec![b'n'; 300];
        let requested = std::ffi::CString::new(external.clone()).unwrap();
        let mut env = ptr::null_mut();
        let mut txn = ptr::null_mut();
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut env), 0);
            assert_eq!(environment::mdb_env_set_maxdbs(env, 8), 0);
            assert_eq!(environment::mdb_env_open(env, path.as_ptr(), 0, 0o600), 0);
            assert_eq!(
                transaction::mdb_txn_begin(env, ptr::null_mut(), 0, &mut txn),
                0
            );
            let physical = {
                let transaction = &mut *txn;
                let physical = transaction.resolve_database_name(&external).unwrap();
                let writer = transaction.writer().unwrap();
                if writer.open_database(compat_features::META).is_err() {
                    writer
                        .create_database(compat_features::META, DatabaseKind::Kv)
                        .unwrap();
                }
                writer.create_database(&physical, DatabaseKind::Kv).unwrap();
                writer
                    .put(
                        compat_features::META,
                        &mapping_key(&physical),
                        &vec![b'x'; 300],
                    )
                    .unwrap();
                physical
            };
            let mut dbi = 0;
            assert_eq!(
                operations::mdb_dbi_open(txn, requested.as_ptr(), CREATE, &mut dbi),
                -30796
            );
            assert_eq!(
                (&*txn)
                    .raw_get(compat_features::META, &mapping_key(&physical))
                    .unwrap(),
                Some(vec![b'x'; 300].as_slice())
            );
            transaction::mdb_txn_abort(txn);
            environment::mdb_env_close(env);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
