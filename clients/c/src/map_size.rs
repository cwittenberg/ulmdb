//! Published logical map capacity for independent LMDB adapter environments.
//! The caller holds the compatibility writer lease while publishing a commit.
use super::{FfiResult, code};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

const CONFIG: &str = "LMDB-MAPSIZE";

pub(super) fn read(directory: &Path) -> FfiResult<Option<usize>> {
    let path = directory.join(CONFIG);
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(code(error.into())),
        Ok(metadata) if !metadata.is_file() => return Err(-30796),
        Ok(_) => {}
    }
    let mut bytes = Vec::with_capacity(9);
    File::open(path)
        .map_err(|error| code(error.into()))?
        .take(9)
        .read_to_end(&mut bytes)
        .map_err(|error| code(error.into()))?;
    let bytes: [u8; 8] = bytes.try_into().map_err(|_| -30796)?;
    let size = usize::try_from(u64::from_le_bytes(bytes)).map_err(|_| -30796)?;
    if size == 0 {
        return Err(-30796);
    }
    Ok(Some(size))
}

pub(super) fn publish(directory: &Path, size: usize) -> FfiResult<()> {
    if size == 0 {
        return Err(libc::EINVAL);
    }
    if read(directory)?.is_some_and(|published| published >= size) {
        return Ok(());
    }
    let size = u64::try_from(size).map_err(|_| -30792)?;
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let (temporary, mut file) = loop {
        let serial = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| -30792)?;
        let path = directory.join(format!(".lmdb-mapsize-{}-{serial}.tmp", std::process::id()));
        match ulmdb_storage::access::options_for_entry(&path)
            .map_err(code)?
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => break (path, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(code(error.into())),
        }
    };
    let result = (|| -> std::io::Result<()> {
        file.write_all(&size.to_le_bytes())?;
        ulmdb_platform::sync_file(&file)?;
        ulmdb_platform::rename_replace(&temporary, &directory.join(CONFIG))
            .map_err(ulmdb_platform::rename_original_error)?;
        ulmdb_platform::sync_path(directory)
    })();
    if result.is_err() {
        // Only the private file is ours. If rename succeeded it is absent;
        // never remove the published config after a directory-sync failure.
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|error| code(error.into()))
}

/// Two scalar certificates cover the common selected/previous views. No
/// snapshot, native handle, mapped window or business record is retained.
#[derive(Default)]
pub(super) struct Bounds {
    recent: [Option<Bound>; 2],
}
#[derive(Clone, Copy, Eq, PartialEq)]
struct Key {
    uuid: [u8; 16],
    checkpoint: [u8; 64],
    // Facade reads can project empty MAIN without changing this checkpoint.
    // Its catalog charge must not certify the unprojected native writer base.
    main_present: bool,
}
#[derive(Clone, Copy)]
struct Bound {
    key: Key,
    used: usize,
}
impl Bounds {
    fn get(&self, key: Key) -> Option<usize> {
        self.recent
            .iter()
            .flatten()
            .find(|b| b.key == key)
            .map(|b| b.used)
    }
    fn insert(&mut self, key: Key, used: usize) {
        if self.recent[0].is_some_and(|b| b.key == key) {
            self.recent[0] = Some(Bound { key, used });
        } else {
            self.recent[1] = self.recent[0];
            self.recent[0] = Some(Bound { key, used });
        }
    }
}
fn key(read: &super::ReadTxn) -> FfiResult<Key> {
    let main_present = match read.database_kind(super::MAIN) {
        Ok(_) => true,
        Err(ulmdb::Error::MissingDatabase) => false,
        Err(error) => return Err(code(error)),
    };
    Ok(Key {
        uuid: read.file_manifest().map_err(code)?.uuid,
        checkpoint: read.file_checkpoint().map_err(code)?,
        main_present,
    })
}
/// Measure a new immutable view once. The existing scalar quota walker reads
/// metadata with short scopes; it never materializes value bodies or invokes
/// ordering callbacks. Capacity hints alone do not certify Native-only writes.
pub(super) fn used(shared: &super::Shared, read: &super::ReadTxn) -> FfiResult<Option<usize>> {
    shared.check_process()?;
    let key = key(read)?;
    if let Ok(bounds) = shared.map_bounds.lock()
        && let Some(used) = bounds.get(key)
    {
        return Ok(Some(used));
    }
    let manifest = read.file_manifest().map_err(code)?;
    let used = if manifest.catalog.is_null() && manifest.segments.is_empty() {
        // Proven empty inventory: virtual MAIN has no persisted catalog row
        // or payload. Only the facade's two-page logical reserve is charged.
        8192
    } else {
        match super::incremental::logical_size(read, shared) {
            Ok(used) => used,
            // Keep the existing lazy structural/reverse-map error boundary.
            // This is no size certificate and must never enter the cache.
            Err(-30796) => return Ok(None),
            // Resource/IO/transform failures can occur for valid data. They
            // are not evidence the view fits: fail admission explicitly.
            Err(error) => return Err(error),
        }
    };
    // Metadata decoding can invoke application crypto callbacks. Recheck the
    // creator before touching a mutex copied by a callback's fork.
    shared.check_process()?;
    if let Ok(mut bounds) = shared.map_bounds.lock() {
        bounds.insert(key, used);
    }
    Ok(Some(used))
}
pub(super) fn check(
    shared: &super::Shared,
    read: &super::ReadTxn,
    capacity: usize,
) -> FfiResult<()> {
    check_with_used(shared, read, capacity).map(|_| ())
}
/// Return only the scalar certified for this exact admitted immutable root.
/// A writer may use it as its starting quota while retaining its writer lease;
/// private mutations still adjust/invalidate it and commit reconciles roots.
pub(super) fn check_with_used(
    shared: &super::Shared,
    read: &super::ReadTxn,
    capacity: usize,
) -> FfiResult<Option<usize>> {
    let used = used(shared, read)?;
    if used.is_some_and(|used| used > capacity) {
        Err(-30785) // MDB_MAP_RESIZED; the owner must adopt/grow its local map.
    } else {
        Ok(used)
    }
}
pub(super) fn minimum(shared: &super::Shared, read: &super::ReadTxn) -> FfiResult<Option<usize>> {
    used(shared, read)?
        .map(|used| {
            let page = shared.page_size.get();
            used.checked_add(page - 1)
                .map(|n| n / page * page)
                .ok_or(-30792)
        })
        .transpose()
}
/// Successful C publication already reconciled every winning logical root.
/// Bind that scalar to the actual new immutable checkpoint, best effort only.
pub(super) fn published(shared: &super::Shared, read: &super::ReadTxn, used: usize) {
    if shared.creator != std::process::id() {
        return;
    }
    if let Ok(key) = key(read)
        && let Ok(mut bounds) = shared.map_bounds.lock()
    {
        bounds.insert(key, used);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    struct Fixture {
        env: *mut MDB_env,
        old: *mut MDB_txn,
        reset: *mut MDB_txn,
        path: std::path::PathBuf,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            unsafe {
                if !self.old.is_null() {
                    transaction::mdb_txn_abort(self.old);
                }
                if !self.reset.is_null() {
                    transaction::mdb_txn_abort(self.reset);
                }
                if !self.env.is_null() {
                    environment::mdb_env_close(self.env);
                }
            }
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
    #[test]
    fn native_peer_growth_requires_adoption_without_losing_old_borrows_or_reset_state() {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-map-growth-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        {
            let e = Environment::create(&path).unwrap();
            let mut w = e.begin_write().unwrap();
            w.create_database(MAIN, DatabaseKind::Kv).unwrap();
            w.put(MAIN, b"a", b"A").unwrap();
            w.commit().unwrap();
        }
        std::fs::write(path.join("LMDB-ABI"), b"1.0\n").unwrap();
        let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        let mut f = Fixture {
            env: ptr::null_mut(),
            old: ptr::null_mut(),
            reset: ptr::null_mut(),
            path,
        };
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut f.env), 0);
            assert_eq!(environment::mdb_env_set_mapsize(f.env, 1 << 20), 0);
            assert_eq!(
                environment::mdb_env_open(f.env, name.as_ptr(), 0x200000, 0o600),
                0
            );
            assert_eq!(
                transaction::mdb_txn_begin(f.env, ptr::null_mut(), RDONLY, &mut f.old),
                0
            );
            let mut k = MDB_val {
                mv_size: 1,
                mv_data: b"a".as_ptr().cast_mut().cast(),
            };
            let mut v = MDB_val {
                mv_size: 0,
                mv_data: ptr::null_mut(),
            };
            assert_eq!(operations::mdb_get(f.old, 1, &mut k, &mut v), 0);
            let held = std::slice::from_raw_parts(v.mv_data.cast::<u8>(), v.mv_size);
            assert_eq!(
                transaction::mdb_txn_begin(f.env, ptr::null_mut(), RDONLY, &mut f.reset),
                0
            );
            transaction::mdb_txn_reset(f.reset);
            let published_hint = read(&f.path).unwrap();
            {
                // Native-only writers do not update the C capacity hint.
                let e = Environment::open(&f.path).unwrap();
                let mut w = e.begin_write().unwrap();
                w.put(MAIN, b"b", &vec![b'B'; 2 << 20]).unwrap();
                w.commit().unwrap();
            }
            assert_eq!(published_hint, read(&f.path).unwrap());
            assert_eq!(held, b"A");
            let shared = (*f.env).shared.as_ref().unwrap();
            let readers = shared.readers.load(Ordering::Acquire);
            let mut failed = ptr::null_mut();
            assert_eq!(
                transaction::mdb_txn_begin(f.env, ptr::null_mut(), RDONLY, &mut failed),
                -30785
            );
            assert!(failed.is_null());
            assert_eq!(transaction::mdb_txn_renew(f.reset), -30785);
            assert!((*f.reset).state.is_none());
            assert_eq!(shared.readers.load(Ordering::Acquire), readers);
            assert_eq!(*shared.lifecycle.lock().unwrap(), 0);
            transaction::mdb_txn_abort(f.old);
            f.old = ptr::null_mut();
            assert_eq!(environment::mdb_env_set_mapsize(f.env, 0), 0);
            assert!((*f.env).mapsize > 1 << 20);
            // A nonzero shrink request also clamps to the committed minimum.
            assert_eq!(environment::mdb_env_set_mapsize(f.env, 1), 0);
            assert!((*f.env).mapsize > 1 << 20);
            assert_eq!(transaction::mdb_txn_renew(f.reset), 0);
            k.mv_data = b"b".as_ptr().cast_mut().cast();
            assert_eq!(operations::mdb_get(f.reset, 1, &mut k, &mut v), 0);
            assert_eq!(v.mv_size, 2 << 20);
            assert!(
                std::slice::from_raw_parts(v.mv_data.cast::<u8>(), v.mv_size)
                    .iter()
                    .all(|b| *b == b'B')
            );
            assert!(
                shared
                    .map_bounds
                    .lock()
                    .unwrap()
                    .recent
                    .iter()
                    .flatten()
                    .count()
                    <= 2
            );
        }
    }
}
