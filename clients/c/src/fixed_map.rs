//! Explicit fixed placement. Creation reserves a genuine persisted window;
//! ordinary callers/copy audits do not silently opt into address placement.
use super::*;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(super) fn open(
    directory: &std::path::Path,
    env: &MDB_env,
    flags: u32,
    modern: bool,
) -> FfiResult<Option<Arc<ulmdb::FixedMap>>> {
    if flags & 1 == 0 {
        return Ok(None);
    }
    let transform = env.crypto.context(env.mapsize)?;
    if transform.as_ref().is_some_and(|c| c.encrypted()) {
        return Err(libc::EINVAL);
    }
    let fixed = if directory.join("FIXEDMAP").exists() {
        ulmdb::FixedMap::open_namespace(directory).map_err(|error| match error {
            Error::Busy if modern => -30769,
            other => code(other),
        })?
    } else {
        if flags & RDONLY != 0 {
            return Err(libc::EINVAL);
        }
        // Bind/check the true empty selected realm while genuine writer or
        // explicit caller-managed publication authority protects creation.
        let owner = if flags & 0x400000 != 0 {
            Environment::open_externally_synchronized_with_transform_options(
                directory,
                ulmdb::ResidencyPolicy::Demand,
                read_ahead(flags),
                transform.clone(),
            )
        } else {
            Environment::open_with_transform_options(
                directory,
                ulmdb::ResidencyPolicy::Demand,
                read_ahead(flags),
                transform.clone(),
            )
        }
        .map_err(code)?;
        let read = owner.begin_read();
        let m = read.file_manifest().map_err(code)?;
        if m.sequence != 0 || !m.catalog.is_null() || !m.segments.is_empty() {
            return Err(libc::EINVAL);
        }
        // New coordinated namespaces negotiate the actual reusable protocol.
        // Caller-managed NOLOCK retains v1; existing layouts above never upgrade.
        let create = if flags & 0x400000 != 0 {
            ulmdb::FixedMap::create
        } else {
            ulmdb::FixedMap::create_reusable
        };
        create(directory, m.uuid, m.epoch, env.mapsize, None).map_err(code)?
    };
    if flags & 0x400000 != 0 && fixed.supports_reuse().map_err(code)? {
        return Err(libc::EINVAL);
    }
    Ok(Some(fixed))
}
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(super) fn open(
    _directory: &std::path::Path,
    _env: &MDB_env,
    flags: u32,
    _modern: bool,
) -> FfiResult<()> {
    if flags & 1 != 0 {
        Err(libc::ENOTSUP)
    } else {
        Ok(())
    }
}
pub(super) fn read(shared: &Shared, read: ReadTxn) -> FfiResult<ReadTxn> {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    if let Some(fixed) = &shared.fixed_map {
        return read
            .with_fixed_map(&shared.directory, fixed.clone())
            .map_err(code);
    }
    Ok(read)
}
pub(super) fn owner(shared: &Shared, owner: &Environment) -> FfiResult<()> {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    if let Some(fixed) = &shared.fixed_map {
        owner.bind_fixed_map_layout(fixed.clone()).map_err(code)?;
    } else if let Some(declaration) = &shared.fixed_declaration {
        owner
            .track_fixed_map_declaration(declaration.clone())
            .map_err(code)?;
    }
    Ok(())
}
pub(super) fn previous_owner(shared: &Shared, owner: &ulmdb::PreviousSession) -> FfiResult<()> {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    if let Some(fixed) = &shared.fixed_map {
        owner.bind_fixed_map_layout(fixed.clone()).map_err(code)?;
    } else if let Some(declaration) = &shared.fixed_declaration {
        owner
            .track_fixed_map_declaration(declaration.clone())
            .map_err(code)?;
    }
    Ok(())
}
pub(super) fn writer(shared: &Shared, writer: &mut WriteTxn<'_>) -> FfiResult<()> {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    if let Some(fixed) = &shared.fixed_map {
        writer.bind_fixed_map(fixed.clone()).map_err(code)?;
    }
    Ok(())
}
pub(super) fn address(shared: &Shared) -> FfiResult<*mut c_void> {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    if let Some(fixed) = &shared.fixed_map {
        return Ok(fixed.info().map_err(code)?.address as *mut c_void);
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    if shared.directory.join("FIXEDMAP").exists() {
        let read = previous::metadata_snapshot(shared, shared.previous.active(), false)?;
        let m = read.file_manifest().map_err(code)?;
        return Ok(
            ulmdb::FixedMap::declared_info(&shared.directory, m.uuid, m.epoch)
                .map_err(code)?
                .address as *mut c_void,
        );
    }
    Ok(ptr::null_mut())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::{
        ffi::CString,
        fs,
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
    struct Env(*mut MDB_env);
    impl Drop for Env {
        fn drop(&mut self) {
            unsafe { environment::mdb_env_close(self.0) }
        }
    }
    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn open(path: &Path, flags: u32) -> Env {
        let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        let mut e = ptr::null_mut();
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut e), 0);
            assert_eq!(environment::mdb_env_open(e, name.as_ptr(), flags, 0o600), 0);
        }
        Env(e)
    }
    fn put(e: &Env, byte: u8) {
        put_before_commit_flags(e, byte, None)
    }
    fn put_before_commit_flags(e: &Env, byte: u8, flags: Option<u32>) {
        let mut w = ptr::null_mut();
        assert_eq!(
            unsafe { transaction::mdb_txn_begin(e.0, ptr::null_mut(), 0, &mut w) },
            0
        );
        let mut db = 0;
        assert_eq!(
            unsafe { operations::mdb_dbi_open(w, ptr::null(), 0, &mut db) },
            0
        );
        let mut k = MDB_val {
            mv_size: 1,
            mv_data: b"k".as_ptr().cast_mut().cast(),
        };
        let bytes = vec![byte; 12_000];
        let mut v = MDB_val {
            mv_size: bytes.len(),
            mv_data: bytes.as_ptr().cast_mut().cast(),
        };
        assert_eq!(unsafe { operations::mdb_put(w, db, &mut k, &mut v, 0) }, 0);
        if let Some(flags) = flags {
            assert_eq!(unsafe { environment::mdb_env_set_flags(e.0, flags, 1) }, 0);
        }
        assert_eq!(unsafe { transaction::mdb_txn_commit(w) }, 0);
    }
    fn get(e: &Env, expected: u8) -> usize {
        let mut r = ptr::null_mut();
        assert_eq!(
            unsafe { transaction::mdb_txn_begin(e.0, ptr::null_mut(), RDONLY, &mut r) },
            0
        );
        let mut k = MDB_val {
            mv_size: 1,
            mv_data: b"k".as_ptr().cast_mut().cast(),
        };
        let mut v = MDB_val {
            mv_size: 0,
            mv_data: ptr::null_mut(),
        };
        assert_eq!(unsafe { operations::mdb_get(r, 1, &mut k, &mut v) }, 0);
        assert_eq!(
            unsafe { std::slice::from_raw_parts(v.mv_data.cast::<u8>(), v.mv_size) },
            vec![expected; 12_000]
        );
        let address = v.mv_data as usize;
        unsafe { transaction::mdb_txn_abort(r) };
        address
    }
    #[test]
    fn public_fixedmap_flag_reports_actual_window_and_value_pointer_survives_close_reopen() {
        if isolated(
            "fixed_map::tests::public_fixedmap_flag_reports_actual_window_and_value_pointer_survives_close_reopen",
        ) {
            return;
        }
        for mode in [
            ulmdb::PublicationMode::Legacy,
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            let d = Dir(std::env::temp_dir().join(format!(
                "c-fixed-{}",
                ulmdb_storage::new_identity().unwrap()
            )));
            drop(Environment::create_with_publication(&d.0, mode).unwrap());
            let e = open(&d.0, 1);
            put(&e, 7);
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
            let address = get(&e, 7);
            assert!((info.address..info.address + info.capacity).contains(&address));
            let mut out = std::mem::MaybeUninit::<MDB_envinfo>::uninit();
            assert_eq!(
                unsafe { environment::mdb_env_info(e.0, out.as_mut_ptr()) },
                0
            );
            assert_eq!(
                unsafe { out.assume_init() }.me_mapaddr as usize,
                info.address
            );
            drop(e);
            let e = open(&d.0, 1 | RDONLY);
            assert_eq!(get(&e, 7), address);
            drop(e);
            let e = open(&d.0, 1);
            put(&e, 9);
            let changed = get(&e, 9);
            assert_ne!(changed, address);
            drop(e);
            let e = open(&d.0, 1 | RDONLY);
            assert_eq!(get(&e, 9), changed);
        }
    }
    #[test]
    fn ordinary_reopen_can_write_while_stored_base_is_occupied_then_fixed_reopen_uses_new_slot() {
        if isolated(
            "fixed_map::tests::ordinary_reopen_can_write_while_stored_base_is_occupied_then_fixed_reopen_uses_new_slot",
        ) {
            return;
        }
        for mode in [
            ulmdb::PublicationMode::Legacy,
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            let d = Dir(std::env::temp_dir().join(format!(
                "c-fixed-normal-{}",
                ulmdb_storage::new_identity().unwrap()
            )));
            drop(Environment::create_with_publication(&d.0, mode).unwrap());
            let e = open(&d.0, 1);
            put(&e, 7);
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
            drop(e);
            let guard = ulmdb_storage::fixed_address::Reservation::reserve(
                Some(info.address),
                info.capacity,
            )
            .unwrap();
            let e = open(&d.0, 0);
            let normal = get(&e, 7);
            assert!(!(info.address..info.address + info.capacity).contains(&normal));
            let mut out = std::mem::MaybeUninit::<MDB_envinfo>::uninit();
            assert_eq!(
                unsafe { environment::mdb_env_info(e.0, out.as_mut_ptr()) },
                0
            );
            assert_eq!(
                unsafe { out.assume_init() }.me_mapaddr as usize,
                info.address
            );
            put(&e, 9);
            let changed = get(&e, 9);
            assert!(!(info.address..info.address + info.capacity).contains(&changed));
            drop(e);
            drop(guard);
            let e = open(&d.0, 1 | RDONLY);
            let fixed = get(&e, 9);
            assert!((info.address..info.address + info.capacity).contains(&fixed));
        }
    }
    #[test]
    fn raw_compact_and_fd_copies_preserve_checked_fixed_layout_without_mapping_live_target() {
        if isolated(
            "fixed_map::tests::raw_compact_and_fd_copies_preserve_checked_fixed_layout_without_mapping_live_target",
        ) {
            return;
        }
        use std::os::fd::AsRawFd;
        for mode in [
            ulmdb::PublicationMode::Legacy,
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            let d = Dir(std::env::temp_dir().join(format!(
                "c-fixed-copy-{}",
                ulmdb_storage::new_identity().unwrap()
            )));
            fs::create_dir(&d.0).unwrap();
            let source = d.0.join("source");
            drop(Environment::create_with_publication(&source, mode).unwrap());
            let e = open(&source, 1);
            put(&e, 7);
            put(&e, 9);
            let base = unsafe {
                (*e.0)
                    .shared
                    .as_ref()
                    .unwrap()
                    .fixed_map
                    .as_ref()
                    .unwrap()
                    .info()
                    .unwrap()
                    .address
            };
            let raw = d.0.join("raw");
            let compact = d.0.join("compact");
            for (path, flags) in [(&raw, 0), (&compact, 1)] {
                let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
                assert_eq!(unsafe { copy::mdb_env_copy2(e.0, name.as_ptr(), flags) }, 0);
                assert!(path.join("FIXEDMAP").is_file());
            }
            let archive = d.0.join("archive");
            let output = fs::File::create(&archive).unwrap();
            assert_eq!(
                unsafe { copy::mdb_env_copyfd2(e.0, output.as_raw_fd(), 0) },
                0
            );
            drop(output);
            let restored = d.0.join("restored");
            copy::restore_archive_with_transform(&archive, &restored, None).unwrap();
            // Copies cannot borrow or clobber the live source's reserved range.
            let _ = get(&e, 9);
            drop(e);
            for path in [&raw, &compact, &restored] {
                let e = open(path, 1 | RDONLY);
                let address = get(&e, 9);
                let mut info = std::mem::MaybeUninit::<MDB_envinfo>::uninit();
                assert_eq!(
                    unsafe { environment::mdb_env_info(e.0, info.as_mut_ptr()) },
                    0
                );
                let info = unsafe { info.assume_init() };
                assert_eq!(info.me_mapaddr as usize, base);
                assert!((base..base + info.me_mapsize).contains(&address));
            }
        }
    }
    #[test]
    fn fixed_writemap_reserve_is_actual_shared_writable_window_then_sealed_at_same_offset() {
        if isolated(
            "fixed_map::tests::fixed_writemap_reserve_is_actual_shared_writable_window_then_sealed_at_same_offset",
        ) {
            return;
        }
        for mode in [
            ulmdb::PublicationMode::Legacy,
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            let d = Dir(std::env::temp_dir().join(format!(
                "c-fixed-writer-{}",
                ulmdb_storage::new_identity().unwrap()
            )));
            drop(Environment::create_with_publication(&d.0, mode).unwrap());
            let e = open(&d.0, 1 | 0x80000 | 0x200000);
            let mut old = ptr::null_mut();
            let mut old_value = MDB_val {
                mv_size: 0,
                mv_data: ptr::null_mut(),
            };
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
            for (byte, commit) in [(3u8, false), (7, true), (9, true)] {
                let mut w = ptr::null_mut();
                assert_eq!(
                    unsafe { transaction::mdb_txn_begin(e.0, ptr::null_mut(), 0, &mut w) },
                    0
                );
                let map = match unsafe { (*w).state.as_ref().unwrap() } {
                    State::Write(w) => w.mapped_allocation().unwrap().unwrap(),
                    _ => panic!("writer"),
                };
                let mut k = MDB_val {
                    mv_size: 1,
                    mv_data: b"k".as_ptr().cast_mut().cast(),
                };
                let mut v = MDB_val {
                    mv_size: 12_000,
                    mv_data: ptr::null_mut(),
                };
                assert_eq!(
                    unsafe { operations::mdb_put(w, 1, &mut k, &mut v, 0x10000) },
                    0
                );
                let address = v.mv_data as usize;
                assert!((info.address..info.address + info.capacity).contains(&address));
                assert!((map.address..map.address + map.capacity).contains(&address));
                let maps = fs::read_to_string("/proc/self/maps").unwrap();
                let row = maps
                    .lines()
                    .find(|line| {
                        let range = line.split_whitespace().next().unwrap();
                        let (a, b) = range.split_once('-').unwrap();
                        let a = usize::from_str_radix(a, 16).unwrap();
                        let b = usize::from_str_radix(b, 16).unwrap();
                        (a..b).contains(&address)
                    })
                    .unwrap();
                assert!(row.contains("rw-s") && row.contains("writemap"), "{row}");
                unsafe {
                    std::slice::from_raw_parts_mut(v.mv_data.cast::<u8>(), v.mv_size).fill(byte)
                };
                if !commit {
                    unsafe { transaction::mdb_txn_abort(w) };
                    continue;
                }
                if byte == 9 {
                    assert_eq!(
                        unsafe { environment::mdb_env_set_flags(e.0, 0x100000, 1) },
                        0
                    );
                }
                assert_eq!(unsafe { transaction::mdb_txn_commit(w) }, 0);
                assert_eq!(get(&e, byte), address);
                if byte == 7 {
                    assert_eq!(
                        unsafe {
                            transaction::mdb_txn_begin(e.0, ptr::null_mut(), RDONLY, &mut old)
                        },
                        0
                    );
                    assert_eq!(
                        unsafe { operations::mdb_get(old, 1, &mut k, &mut old_value) },
                        0
                    );
                } else {
                    assert_eq!(
                        unsafe {
                            std::slice::from_raw_parts(
                                old_value.mv_data.cast::<u8>(),
                                old_value.mv_size,
                            )
                        },
                        vec![7; 12_000]
                    );
                }
            }
            unsafe { transaction::mdb_txn_abort(old) };
            let final_address = get(&e, 9);
            drop(e);
            let reopened = open(&d.0, 1 | RDONLY);
            assert_eq!(get(&reopened, 9), final_address);
        }
    }
    unsafe extern "C" fn checksum(
        source: *const MDB_val,
        destination: *mut MDB_val,
        _: *const MDB_val,
    ) {
        let source = unsafe { &*source };
        let destination = unsafe { &mut *destination };
        let bytes = if source.mv_size == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(source.mv_data.cast::<u8>(), source.mv_size) }
        };
        let mut h = 14695981039346656037u64;
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        assert_eq!(destination.mv_size, 8);
        unsafe {
            std::ptr::copy_nonoverlapping(
                h.to_le_bytes().as_ptr(),
                destination.mv_data.cast::<u8>(),
                8,
            )
        };
    }
    fn checksum_open(path: &Path, flags: u32, create: Option<ulmdb::PublicationMode>) -> Env {
        let mut e = ptr::null_mut();
        assert_eq!(unsafe { environment::mdb_env_create(&mut e) }, 0);
        assert_eq!(
            unsafe { crypto::mdb_env_set_checksum(e, Some(checksum), 8) },
            0
        );
        if let Some(mode) = create {
            let ctx = unsafe { (*e).crypto.context((*e).mapsize).unwrap().unwrap() };
            drop(Environment::create_with_transform(path, mode, ctx).unwrap());
        }
        let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(
            unsafe { environment::mdb_env_open(e, name.as_ptr(), flags, 0o600) },
            0
        );
        Env(e)
    }
    #[test]
    fn public_checksum_only_fixedmap_uses_sealed_decoded_addresses_and_reopens() {
        if isolated(
            "fixed_map::tests::public_checksum_only_fixedmap_uses_sealed_decoded_addresses_and_reopens",
        ) {
            return;
        }
        for mode in [
            ulmdb::PublicationMode::Legacy,
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            for writer_flag in [0, 0x80000, 0x180000] {
                let d = Dir(std::env::temp_dir().join(format!(
                    "c-fixed-checksum-{}",
                    ulmdb_storage::new_identity().unwrap()
                )));
                let e = checksum_open(&d.0, 1 | writer_flag, Some(mode));
                put(&e, 7);
                let first = get(&e, 7);
                if writer_flag == 0x80000 {
                    put_before_commit_flags(&e, 9, Some(0x100000));
                } else {
                    put(&e, 9);
                }
                let second = get(&e, 9);
                assert_ne!(first, second);
                let mut info = std::mem::MaybeUninit::<MDB_envinfo>::uninit();
                assert_eq!(
                    unsafe { environment::mdb_env_info(e.0, info.as_mut_ptr()) },
                    0
                );
                let info = unsafe { info.assume_init() };
                assert!(
                    (info.me_mapaddr as usize..info.me_mapaddr as usize + info.me_mapsize)
                        .contains(&second)
                );
                let mut held = ptr::null_mut();
                assert_eq!(
                    unsafe { transaction::mdb_txn_begin(e.0, ptr::null_mut(), RDONLY, &mut held) },
                    0
                );
                let mut key = MDB_val {
                    mv_size: 1,
                    mv_data: b"k".as_ptr().cast_mut().cast(),
                };
                let mut value = MDB_val {
                    mv_size: 0,
                    mv_data: ptr::null_mut(),
                };
                assert_eq!(
                    unsafe { operations::mdb_get(held, 1, &mut key, &mut value) },
                    0
                );
                assert_eq!(value.mv_data as usize, second);
                let maps = fs::read_to_string("/proc/self/maps").unwrap();
                assert!(maps.contains("ulmdb-fixed-decoded"));
                unsafe { transaction::mdb_txn_abort(held) };
                drop(e);
                let reopened = checksum_open(&d.0, 1 | RDONLY, None);
                assert_eq!(get(&reopened, 9), second);
            }
        }
    }
}
