//! A genuine fresh0 is allowed only for the proven empty C facade projection.
use super::*;
use crate::compat_features::{DB_FLAGS, META, Settings};
use sha2::{Digest, Sha256};

const NAME_PREFIX: &[u8] = b"\0lmdb-name:";
fn mapped_name(physical: &[u8], external: &[u8], name_limit: usize, modern: bool) -> FfiResult<()> {
    if (external.len() <= 255 && !(external.is_empty() && modern))
        || external.len() > name_limit
        || external.contains(&0)
    {
        return Err(-30796);
    }
    let mut expected = NAME_PREFIX.to_vec();
    expected.extend_from_slice(&Sha256::digest(external));
    if physical != expected {
        return Err(-30796);
    }
    Ok(())
}
pub(super) fn logical_empty(read: &ReadTxn, name_limit: usize, modern: bool) -> FfiResult<bool> {
    let names = read.database_names().map_err(code)?;
    let mut empty = true;
    let mut main_id = None;
    let mut metadata = false;
    for name in names {
        let actual = read.open_database(&name).map_err(code)?;
        if name == MAIN {
            main_id = Some(alias::NativeId::of(&actual).bytes());
            match actual.kind() {
                DatabaseKind::Kv => {
                    if read
                        .structural_resume_pair(MAIN, None)
                        .map_err(code)?
                        .is_some()
                    {
                        empty = false;
                    }
                }
                // Public MAIN DUPSORT is a genuine nondefault flag state,
                // including when its duplicate tree contains no records.
                DatabaseKind::Duplicates { .. } => empty = false,
                DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => return Err(-30796),
            }
        } else if name == META {
            if actual.kind() != DatabaseKind::Kv {
                return Err(-30796);
            }
            metadata = true;
        } else if name.starts_with(NAME_PREFIX) {
            let mut key = b"name:".to_vec();
            key.extend_from_slice(&name);
            let external = match read.get(META, &key) {
                Ok(Some(external)) => external,
                Ok(None) | Err(Error::MissingDatabase) => return Err(-30796),
                Err(error) => return Err(code(error)),
            };
            mapped_name(&name, external, name_limit, modern)?;
            empty = false;
        } else if name.first().is_some_and(|b| *b != 0) {
            // A named KV/dups/IP index is content even when its user tree is empty.
            empty = false;
        } else {
            return Err(-30796); // Opaque reserved data must never be discarded.
        }
    }
    if metadata {
        let mut bound = None;
        while let Some((key, value)) = read
            .client_seek_pair_borrowed(META, bound, false, false)
            .map_err(code)?
        {
            if key.len() == 8 {
                let settings = Settings::decode(value)?;
                if settings.flags & !DB_FLAGS != 0 || value[12..] != [0; 4] {
                    return Err(-30796);
                }
                if main_id.as_ref().is_some_and(|id| id.as_slice() == key)
                    && (settings.flags != 0 || settings.key_width != 0 || settings.dup_width != 0)
                {
                    empty = false;
                }
            } else if let Some(physical) = key.strip_prefix(b"name:") {
                mapped_name(physical, value, name_limit, modern)?;
            } else {
                return Err(-30796);
            }
            bound = Some((key, None));
        }
    }
    Ok(empty)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_rejects_missing_or_mismatched_live_long_name_metadata() {
        for missing in [true, false] {
            let root = std::env::temp_dir().join(format!(
                "ulmdb-copy-empty-corrupt-{}",
                ulmdb_storage::new_identity().unwrap()
            ));
            fs::create_dir(&root).unwrap();
            let source = root.join("source");
            let target = root.join("copy");
            let path = std::ffi::CString::new(source.to_str().unwrap()).unwrap();
            let destination = std::ffi::CString::new(target.to_str().unwrap()).unwrap();
            let external = vec![b'n'; 300];
            let name = std::ffi::CString::new(external.clone()).unwrap();
            let mut env = ptr::null_mut();
            let mut txn = ptr::null_mut();
            let mut dbi = 0;
            unsafe {
                assert_eq!(environment::mdb_env_create(&mut env), 0);
                assert_eq!(environment::mdb_env_set_maxdbs(env, 8), 0);
                assert_eq!(environment::mdb_env_open(env, path.as_ptr(), 0, 0o600), 0);
                assert_eq!(
                    transaction::mdb_txn_begin(env, ptr::null_mut(), 0, &mut txn),
                    0
                );
                assert_eq!(
                    operations::mdb_dbi_open(txn, name.as_ptr(), CREATE, &mut dbi),
                    0
                );
                assert_eq!(transaction::mdb_txn_commit(txn), 0);
                environment::mdb_env_close(env);
            }
            // Publish damage through a genuine native writer after every C
            // handle/reader ended; never mutate files behind immutable mappings.
            let native = Environment::open(&source).unwrap();
            let mut writer = native.begin_write().unwrap();
            if missing {
                writer.drop_database(META).unwrap();
            } else {
                let mut physical = NAME_PREFIX.to_vec();
                physical.extend_from_slice(&Sha256::digest(&external));
                let mut key = b"name:".to_vec();
                key.extend_from_slice(&physical);
                writer.put(META, &key, &vec![b'x'; 300]).unwrap();
            }
            writer.commit().unwrap();
            drop(native);
            let before = fs::read(source.join("CURRENT")).unwrap();
            let result = unsafe {
                assert_eq!(environment::mdb_env_create(&mut env), 0);
                assert_eq!(environment::mdb_env_set_maxdbs(env, 8), 0);
                assert_eq!(environment::mdb_env_open(env, path.as_ptr(), 0, 0o600), 0);
                let result = mdb_env_copy2(env, destination.as_ptr(), 1);
                environment::mdb_env_close(env);
                result
            };
            assert_eq!(result, -30796, "missing={missing}");
            assert!(!target.exists());
            assert_eq!(fs::read(source.join("CURRENT")).unwrap(), before);
            fs::remove_dir_all(root).unwrap();
        }
    }
}
