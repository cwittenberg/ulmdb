//! Immutable pre-open bounded remapping configuration. The source graph uses
//! authenticated chunks; normal mappings remain the default implementation.
use super::*;
pub(super) type Options = Option<ulmdb::chunked::CacheOptions>;
pub(super) fn options(flags: u32, mapsize: usize) -> FfiResult<Options> {
    if flags & 0x4000000 == 0 {
        return Ok(None);
    }
    if !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
        return Err(libc::ENOTSUP);
    }
    // Explicit/published jumbo capacity authorizes larger bounded windows.
    let jumbo = mapsize > ulmdb::ValueLimit::EXTENDED.segment_bytes();
    let maximum = ulmdb::ValueLimit::MAXIMUM.is_supported()
        && mapsize > ulmdb::ValueLimit::LARGE.segment_bytes();
    // The native payload profile permits512MiB; a contiguous window includes
    // its16-byte payload header plus chunk alignment. Full caches never evict
    // a live borrowed pointer: they return the ordinary resource-limit error.
    let cache = ulmdb_storage::chunk_cache::ChunkCache::new(ulmdb_storage::chunk_cache::Limits {
        mapped_bytes: if maximum {
            (mapsize as u64).clamp(2 << 30, 16 << 30)
        } else if jumbo {
            2 << 30
        } else {
            512 << 20
        },
        windows: 8192,
        chunk_bytes: 64 << 10,
        max_window_bytes: if maximum {
            ulmdb::ValueLimit::MAXIMUM.segment_bytes()
        } else if jumbo {
            ulmdb::ValueLimit::LARGE.segment_bytes()
        } else {
            256 << 20
        },
    })
    .map_err(code)?;
    let mut options = ulmdb::chunked::CacheOptions::new(cache);
    options.pins = ulmdb_storage::chunk_table::PinLimits {
        sources: 65536,
        pins: 8192,
    };
    Ok(Some(options))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::{
        ffi::CString,
        fs,
        os::fd::AsRawFd,
        path::{Path, PathBuf},
    };
    fn isolated(name: &str) -> bool {
        const KEY: &str = "ULMDB_C_FIXEDMAP_TEST";
        if std::env::var(KEY).as_deref() == Ok(name) {
            return false;
        }
        // After a fixed owner closes, another test thread may legitimately
        // occupy its declared address. Preserve every collision assertion in a
        // fresh VMA namespace rather than retrying or accepting a busy reopen.
        struct Owned(std::process::Child);
        impl Drop for Owned {
            fn drop(&mut self) {
                if self.0.try_wait().ok().flatten().is_none() {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
        }
        let mut child = Owned(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", name, "--nocapture", "--test-threads=1"])
                .env(KEY, name)
                .spawn()
                .unwrap(),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success(), "fixed-map worker {name}: {status}");
                return true;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "fixed-map worker deadline: {name}"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    struct Env(*mut MDB_env);
    impl Drop for Env {
        fn drop(&mut self) {
            unsafe { environment::mdb_env_close(self.0) }
        }
    }
    struct Txn(*mut MDB_txn);
    impl Drop for Txn {
        fn drop(&mut self) {
            unsafe { transaction::mdb_txn_abort(self.0) }
        }
    }
    fn open(path: &Path, flags: u32) -> Env {
        let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        let mut e = ptr::null_mut();
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut e), 0);
            assert_eq!(environment::mdb_env_set_maxdbs(e, 16), 0);
            assert_eq!(environment::mdb_env_set_mapsize(e, 64 << 20), 0);
            assert_eq!(
                environment::mdb_env_open(e, name.as_ptr(), flags, 0o600),
                0,
                "open {}",
                path.display()
            );
        }
        Env(e)
    }
    fn begin(e: &Env, flags: u32) -> Txn {
        let mut t = ptr::null_mut();
        assert_eq!(
            unsafe { transaction::mdb_txn_begin(e.0, ptr::null_mut(), flags, &mut t) },
            0
        );
        Txn(t)
    }
    fn commit(mut t: Txn) {
        let raw = t.0;
        t.0 = ptr::null_mut();
        assert_eq!(unsafe { transaction::mdb_txn_commit(raw) }, 0);
    }
    fn val(bytes: &[u8]) -> MDB_val {
        MDB_val {
            mv_size: bytes.len(),
            mv_data: bytes.as_ptr().cast_mut().cast(),
        }
    }
    fn put(t: &Txn, db: u32, key: &[u8], bytes: &[u8]) {
        assert_eq!(
            unsafe { operations::mdb_put(t.0, db, &mut val(key), &mut val(bytes), 0) },
            0
        );
    }
    fn get(t: &Txn, db: u32, key: &[u8]) -> MDB_val {
        let mut out = val(&[]);
        assert_eq!(
            unsafe { operations::mdb_get(t.0, db, &mut val(key), &mut out) },
            0
        );
        out
    }
    fn bytes(v: &MDB_val) -> &[u8] {
        unsafe { std::slice::from_raw_parts(v.mv_data.cast(), v.mv_size) }
    }
    fn assert_cached(t: &Txn) {
        unsafe {
            assert!(
                matches!((*t.0).state.as_ref(),Some(State::Read(read)) if read.uses_chunk_cache())
            );
        }
    }
    #[test]
    fn remap_public_transactions_borrow_write_reopen_and_copy_all_file_modes() {
        for mode in [
            ulmdb::PublicationMode::Legacy,
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            for extra in [0, 0x80000, 0x400000] {
                let d = Dir(std::env::temp_dir().join(format!(
                    "c-remap-{}",
                    ulmdb_storage::new_identity().unwrap()
                )));
                fs::create_dir(&d.0).unwrap();
                let source = d.0.join("source");
                drop(Environment::create_with_publication(&source, mode).unwrap());
                let flags = 0x4000000 | 0x200000 | extra;
                let e = open(&source, flags);
                if extra == 0x80000 {
                    assert_eq!(unsafe { (*e.0).flags() } & 0x80000, 0);
                }
                let w = begin(&e, 0);
                let name = CString::new("named").unwrap();
                let mut db = 0;
                assert_eq!(
                    unsafe { operations::mdb_dbi_open(w.0, name.as_ptr(), CREATE, &mut db) },
                    0
                );
                for k in 0..1500u64 {
                    put(&w, db, &k.to_be_bytes(), &[41; 128]);
                }
                put(&w, db, b"large", &vec![7; 200_000]);
                commit(w);
                let old = begin(&e, RDONLY);
                assert_cached(&old);
                let held = get(&old, db, b"large");
                assert_eq!(bytes(&held), vec![7; 200_000]);
                let w = begin(&e, 0);
                put(&w, db, b"large", b"new");
                commit(w);
                let next = begin(&e, RDONLY);
                assert_cached(&next);
                assert_eq!(bytes(&get(&next, db, b"large")), b"new");
                drop(next);
                assert_eq!(bytes(&held), vec![7; 200_000]);
                let cache = unsafe {
                    (*e.0)
                        .shared
                        .as_ref()
                        .unwrap()
                        .cache
                        .as_ref()
                        .unwrap()
                        .cache
                        .clone()
                };
                let stats = cache.stats().unwrap();
                assert!(stats.misses > 0 && stats.mapped_bytes > 0 && stats.pinned_windows > 0);
                drop(old);
                for compact in [false, true] {
                    let target = d.0.join(if compact { "compact" } else { "raw" });
                    let name = CString::new(target.as_os_str().as_encoded_bytes()).unwrap();
                    assert_eq!(
                        unsafe { copy::mdb_env_copy2(e.0, name.as_ptr(), u32::from(compact)) },
                        0,
                        "copy mode={mode:?} extra={extra}"
                    );
                    let target = open(&target, 0x4000000 | 0x200000 | RDONLY | extra & 0x400000);
                    let r = begin(&target, RDONLY);
                    assert_cached(&r);
                    let mut actual = 0;
                    assert_eq!(
                        unsafe {
                            operations::mdb_dbi_open(
                                r.0,
                                CString::new("named").unwrap().as_ptr(),
                                0,
                                &mut actual,
                            )
                        },
                        0
                    );
                    assert_eq!(bytes(&get(&r, actual, b"large")), b"new");
                    drop(r);
                    drop(target);
                }
                let archive = d.0.join("archive");
                let out = fs::File::create(&archive).unwrap();
                assert_eq!(unsafe { copy::mdb_env_copyfd2(e.0, out.as_raw_fd(), 0) }, 0);
                drop(out);
                drop(e);
                let restored = open(&archive, flags | 0x4000);
                let r = begin(&restored, RDONLY);
                assert_cached(&r);
                let mut actual = 0;
                assert_eq!(
                    unsafe {
                        operations::mdb_dbi_open(
                            r.0,
                            CString::new("named").unwrap().as_ptr(),
                            0,
                            &mut actual,
                        )
                    },
                    0
                );
                assert_eq!(bytes(&get(&r, actual, b"large")), b"new");
                drop(r);
                drop(restored);
                let e = open(&source, flags);
                let r = begin(&e, RDONLY);
                assert_cached(&r);
                let mut db = 0;
                assert_eq!(
                    unsafe {
                        operations::mdb_dbi_open(
                            r.0,
                            CString::new("named").unwrap().as_ptr(),
                            0,
                            &mut db,
                        )
                    },
                    0
                );
                assert_eq!(bytes(&get(&r, db, b"large")), b"new");
                drop(r);
            }
        }
    }
    #[test]
    fn remap_previous_branch_and_read_reset_renew_keep_cached_sources() {
        for mode in [
            ulmdb::PublicationMode::Legacy,
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            for extra in [0, 0x400000] {
                let d = Dir(std::env::temp_dir().join(format!(
                    "c-remap-prev-{}",
                    ulmdb_storage::new_identity().unwrap()
                )));
                drop(Environment::create_with_publication(&d.0, mode).unwrap());
                fs::write(d.0.join("LMDB-ABI"), b"1.0\n").unwrap();
                let flags = 0x4000000 | 0x200000 | extra;
                let e = open(&d.0, flags);
                for value in [b"old".as_slice(), b"new".as_slice()] {
                    let w = begin(&e, 0);
                    put(&w, 1, b"k", value);
                    commit(w);
                }
                drop(e);
                let e = open(&d.0, flags | 0x2000000);
                let r = begin(&e, RDONLY);
                assert_cached(&r);
                assert_eq!(bytes(&get(&r, 1, b"k")), b"old");
                unsafe { transaction::mdb_txn_reset(r.0) };
                assert_eq!(unsafe { transaction::mdb_txn_renew(r.0) }, 0);
                assert_cached(&r);
                assert_eq!(bytes(&get(&r, 1, b"k")), b"old");
                drop(r);
                let w = begin(&e, 0);
                put(&w, 1, b"k", b"branch");
                assert_eq!(unsafe { transaction::mdb_txn_prepare(w.0) }, 0);
                commit(w);
                let r = begin(&e, RDONLY);
                assert_cached(&r);
                // Locked facade retains its actual A offset until the second
                // publication; caller-managed NOLOCK has no shared offset record.
                assert_eq!(
                    bytes(&get(&r, 1, b"k")),
                    if extra == 0 {
                        b"old".as_slice()
                    } else {
                        b"branch".as_slice()
                    }
                );
                drop(r);
                let w = begin(&e, 0);
                put(&w, 1, b"k", b"second");
                commit(w);
                let r = begin(&e, RDONLY);
                assert_cached(&r);
                assert_eq!(bytes(&get(&r, 1, b"k")), b"second");
                drop(r);
                drop(e);
                let e = open(&d.0, flags);
                let r = begin(&e, RDONLY);
                assert_cached(&r);
                assert_eq!(bytes(&get(&r, 1, b"k")), b"second");
            }
        }
    }
    #[test]
    fn cached_ordinary_c_writer_preserves_existing_fixed_declaration() {
        const TEST: &str =
            "chunks::tests::cached_ordinary_c_writer_preserves_existing_fixed_declaration";
        const WORKER: &str = "ULMDB_C_CACHED_FIXED_CHILD";
        if std::env::var_os(WORKER).as_deref() != Some(std::ffi::OsStr::new(TEST)) {
            // The declared window is deliberately released during the ordinary
            // cached write. Unrelated parallel tests may correctly occupy it.
            // Exec gives this address-sensitive journey its own VMA namespace;
            // retain every address/collision assertion and never choose a retry.
            struct Owned(std::process::Child);
            impl Drop for Owned {
                fn drop(&mut self) {
                    if self.0.try_wait().ok().flatten().is_none() {
                        let _ = self.0.kill();
                        let _ = self.0.wait();
                    }
                }
            }
            let mut child = Owned(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
                    .env(WORKER, TEST)
                    // Inherit the outer stage process group and output handles: its
                    // cancellation also owns the worker, with no unread pipe wait.
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit())
                    .spawn()
                    .unwrap(),
            );
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            loop {
                if let Some(status) = child.0.try_wait().unwrap() {
                    assert!(status.success(), "fixed-cache exec worker failed: {status}");
                    return;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "fixed-cache exec worker exceeded15s deadline"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        for mode in [
            ulmdb::PublicationMode::Legacy,
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            let d = Dir(std::env::temp_dir().join(format!(
                "c-remap-declared-{}",
                ulmdb_storage::new_identity().unwrap()
            )));
            drop(Environment::create_with_publication(&d.0, mode).unwrap());
            let e = open(&d.0, 1 | 0x200000);
            let w = begin(&e, 0);
            put(&w, 1, b"k", b"old");
            commit(w);
            drop(e);
            let e = open(&d.0, 0x4000000 | 0x200000);
            let w = begin(&e, 0);
            put(&w, 1, b"k", b"new");
            commit(w);
            let r = begin(&e, RDONLY);
            assert_cached(&r);
            assert_eq!(bytes(&get(&r, 1, b"k")), b"new");
            drop(r);
            drop(e);
            let e = open(&d.0, 1 | 0x200000 | RDONLY);
            let r = begin(&e, RDONLY);
            let value = get(&r, 1, b"k");
            assert_eq!(bytes(&value), b"new");
            let info = unsafe {
                (*e.0)
                    .shared
                    .as_ref()
                    .unwrap()
                    .fixed_map
                    .as_ref()
                    .unwrap()
                    .info()
                    .unwrap()
            };
            assert!(
                (info.address..info.address + info.capacity).contains(&(value.mv_data as usize))
            );
        }
    }
    /// Caller-managed publication must not acquire native LOCK or History even
    /// when a nonfixed cached writer extends a persisted numeric fixed layout.
    #[test]
    fn cached_nolock_weak_writer_preserves_fixed_layout_under_held_kernel_gates() {
        if isolated("chunks::tests::cached_nolock_weak_writer_preserves_fixed_layout_under_held_kernel_gates") {
            return;
        }
        use std::io::Write;
        for mode in [
            ulmdb::PublicationMode::Legacy,
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            for policy in [0, 0x40000, 0x10000] {
                let d = Dir(std::env::temp_dir().join(format!(
                    "c-remap-weak-fixed-{}",
                    ulmdb_storage::new_identity().unwrap()
                )));
                drop(Environment::create_with_publication(&d.0, mode).unwrap());
                fs::write(d.0.join("LMDB-ABI"), b"1.0\n").unwrap();
                let e = open(&d.0, 1 | 0x200000 | 0x400000);
                let w = begin(&e, 0);
                put(&w, 1, b"k", &[11; 20000]);
                commit(w);
                drop(e);
                let native_lock = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(d.0.join("LOCK"))
                    .unwrap();
                native_lock.lock().unwrap();
                let history = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(d.0.join(".ulmdb-coordination/history"))
                    .unwrap();
                history.lock().unwrap();
                let flags = 0x4000000 | 0x200000 | 0x400000 | policy;
                let e = open(&d.0, flags);
                let old = begin(&e, RDONLY);
                assert_cached(&old);
                let held = get(&old, 1, b"k");
                println!(
                    "CACHE_LAYOUT_BEGIN mode={mode:?} policy={policy} path={}",
                    d.0.display()
                );
                std::io::stdout().flush().unwrap();
                let w = begin(&e, 0);
                put(&w, 1, b"k", &[23; 30000]);
                assert_eq!(unsafe { transaction::mdb_txn_prepare(w.0) }, 0);
                commit(w);
                // Second owner/write exercises metadata retained by the first
                // published cached snapshot, rather than only the initial base.
                let w = begin(&e, 0);
                put(&w, 1, b"second", &[37; 9000]);
                commit(w);
                println!("CACHE_LAYOUT_END mode={mode:?} policy={policy}");
                std::io::stdout().flush().unwrap();
                assert_eq!(bytes(&held), [11; 20000]);
                let now = begin(&e, RDONLY);
                assert_cached(&now);
                assert_eq!(bytes(&get(&now, 1, b"k")), [23; 30000]);
                assert_eq!(bytes(&get(&now, 1, b"second")), [37; 9000]);
                drop(now);
                drop(old);
                println!("CACHE_LAYOUT_SYNC_BEGIN mode={mode:?} policy={policy}");
                std::io::stdout().flush().unwrap();
                assert_eq!(unsafe { environment::mdb_env_sync(e.0, 1) }, 0);
                println!("CACHE_LAYOUT_SYNC_END mode={mode:?} policy={policy}");
                std::io::stdout().flush().unwrap();
                drop(e);
                drop(history);
                drop(native_lock);
                let e = open(&d.0, 1 | 0x200000 | RDONLY);
                let r = begin(&e, RDONLY);
                let got = get(&r, 1, b"k");
                assert_eq!(bytes(&got), [23; 30000]);
                assert_eq!(bytes(&get(&r, 1, b"second")), [37; 9000]);
                let info = unsafe {
                    (*e.0)
                        .shared
                        .as_ref()
                        .unwrap()
                        .fixed_map
                        .as_ref()
                        .unwrap()
                        .info()
                        .unwrap()
                };
                assert!(
                    (info.address..info.address + info.capacity).contains(&(got.mv_data as usize))
                );
                drop(r);
                drop(e);
                println!("CACHE_LAYOUT_CASE_PASS mode={mode:?} policy={policy}");
            }
        }
    }
}
