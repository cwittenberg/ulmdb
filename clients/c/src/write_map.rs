//! WRITEMAP uses the Native allocator's actual shared writable file mapping.
//! Mapping configuration is sampled before mutation; MAPASYNC/NOSYNC data flush
//! is sampled again at prepare so runtime flag changes do not become sticky.
use super::*;
use std::sync::atomic::Ordering;
fn flush(flags: u32) -> ulmdb::MappedFlush {
    if flags & durability::NOSYNC != 0 {
        ulmdb::MappedFlush::None
    } else if flags & 0x100000 != 0 {
        ulmdb::MappedFlush::Asynchronous
    } else {
        ulmdb::MappedFlush::Synchronous
    }
}
pub(super) fn begin(writer: &mut WriteTxn<'_>, env: &MDB_env) -> FfiResult<()> {
    if env.flags() & 0x80000 == 0 {
        return Ok(());
    }
    let capacity = env
        .mapsize
        .min(ulmdb::ValueLimit::platform_maximum().segment_bytes())
        / 4096
        * 4096;
    writer
        .enable_write_map(capacity, flush(env.flags()))
        .map_err(code)?;
    sample_initialization(writer, env.flags())
}
// Sample on each mutable Native access so toggling the public runtime flag
// during a writer/savepoint takes effect before its next allocation.
pub(super) fn sample_initialization(writer: &mut WriteTxn<'_>, flags: u32) -> FfiResult<()> {
    if flags & 0x80000 == 0 {
        return Ok(());
    }
    writer
        .set_mapped_no_mem_init(flags & 0x1000000 != 0)
        .map_err(code)
}
pub(super) fn sample<'a>(mut writer: WriteTxn<'a>, shared: &Shared) -> ulmdb::Result<WriteTxn<'a>> {
    if writer.mapped_allocation()?.is_some() {
        writer.set_mapped_flush(flush(shared.flags.load(Ordering::Acquire)))?;
    }
    Ok(writer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::CString, fs, path::PathBuf};
    struct Fixture {
        env: *mut MDB_env,
        path: PathBuf,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            unsafe { environment::mdb_env_close(self.env) };
            let _ = fs::remove_dir_all(&self.path);
        }
    }
    fn fixture(mode: ulmdb::PublicationMode) -> Fixture {
        fixture_with_flags(mode, 0)
    }
    fn fixture_with_flags(mode: ulmdb::PublicationMode, extra: u32) -> Fixture {
        let path = std::env::temp_dir().join(format!(
            "c-real-writemap-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        drop(Environment::create_with_publication(&path, mode).unwrap());
        let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        let mut env = ptr::null_mut();
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut env), 0);
            assert_eq!(environment::mdb_env_set_mapsize(env, 2 << 20), 0);
            assert_eq!(
                environment::mdb_env_open(env, name.as_ptr(), 0x80000 | extra, 0o600),
                0
            );
        }
        Fixture { env, path }
    }
    fn reserve(f: &Fixture, byte: u8, async_flag: bool) -> ((u64, u128), u64) {
        let mut txn = ptr::null_mut();
        assert_eq!(
            unsafe { transaction::mdb_txn_begin(f.env, ptr::null_mut(), 0, &mut txn) },
            0
        );
        let info = match unsafe { (*txn).state.as_ref().unwrap() } {
            State::Write(writer) => writer.mapped_allocation().unwrap().unwrap(),
            _ => panic!("not writer"),
        };
        let mut key = MDB_val {
            mv_size: 1,
            mv_data: b"k".as_ptr().cast_mut().cast(),
        };
        let mut value = MDB_val {
            mv_size: 48_000,
            mv_data: ptr::null_mut(),
        };
        assert_eq!(
            unsafe { operations::mdb_put(txn, 1, &mut key, &mut value, 0x10000) },
            0
        );
        let addr = value.mv_data as usize;
        assert!((info.address..info.address + info.capacity).contains(&addr));
        unsafe {
            std::slice::from_raw_parts_mut(value.mv_data.cast::<u8>(), value.mv_size).fill(byte)
        };
        if async_flag {
            assert_eq!(
                unsafe { environment::mdb_env_set_flags(f.env, 0x100000, 1) },
                0
            );
        }
        assert_eq!(unsafe { transaction::mdb_txn_commit(txn) }, 0);
        let read = ReadTxn::open_snapshot(&f.path).unwrap();
        assert_eq!(
            read.get(MAIN, b"k").unwrap(),
            Some(vec![byte; 48_000].as_slice())
        );
        (
            (info.file_device, u128::from(info.file_inode)),
            read.stats().sequence,
        )
    }
    #[test]
    fn public_writemap_reserve_really_borrows_shared_file_bytes_and_reopens_all_file_modes() {
        for mode in [
            ulmdb::PublicationMode::Legacy,
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            let f = fixture(mode);
            let (identity, seq) = reserve(&f, 7, false);
            assert_eq!(seq, 1);
            let old = ReadTxn::open_snapshot(&f.path).unwrap();
            let old_bytes = old.get(MAIN, b"k").unwrap().unwrap();
            let (_, seq) = reserve(&f, 9, false);
            assert_eq!(seq, 2);
            assert_eq!(old_bytes, vec![7; 48_000]);
            if mode == ulmdb::PublicationMode::Legacy {
                assert!(fs::read_dir(f.path.join("segments")).unwrap().any(|e| {
                    let file = fs::File::open(e.unwrap().path()).unwrap();
                    let actual = ulmdb_platform::file_identity(&file).unwrap();
                    (actual.volume, actual.file_id) == identity
                }));
            }
            assert!(
                fs::read_dir(f.path.join("transactions"))
                    .unwrap()
                    .next()
                    .is_none()
            );
        }
    }
    #[test]
    fn mapasync_changed_during_writer_is_sampled_at_data_prepare_and_force_sync_works() {
        for mode in [
            ulmdb::PublicationMode::Legacy,
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            let f = fixture(mode);
            reserve(&f, 7, true);
            let (_, capture, pin, state) =
                ulmdb_storage::coordination::Coordination::capture_directory(
                    &f.path,
                    Default::default(),
                )
                .unwrap()
                .capture_current_with_durability()
                .unwrap();
            assert!(state.has_data_debt());
            drop(pin);
            drop(capture);
            assert_eq!(unsafe { environment::mdb_env_sync(f.env, 1) }, 0);
            assert_eq!(
                unsafe { environment::mdb_env_set_flags(f.env, 0x100000, 0) },
                0
            );
            reserve(&f, 9, false);
            let (_, capture, pin, state) =
                ulmdb_storage::coordination::Coordination::capture_directory(
                    &f.path,
                    Default::default(),
                )
                .unwrap()
                .capture_current_with_durability()
                .unwrap();
            assert!(!state.has_data_debt());
            drop(pin);
            drop(capture);
        }
    }
    #[test]
    fn public_nonforced_async_sync_worker() {
        let Ok(mode) = std::env::var("ULMDB_ASYNC_SYNC_FORMAT") else {
            return;
        };
        let mode = match mode.as_str() {
            "2" => ulmdb::PublicationMode::Legacy,
            "3" => ulmdb::PublicationMode::Cohort,
            "4" => ulmdb::PublicationMode::Indexed,
            _ => panic!("format"),
        };
        let external = std::env::var("ULMDB_ASYNC_SYNC_NOLOCK").unwrap() == "1";
        let no_sync = std::env::var("ULMDB_ASYNC_SYNC_SKIP").unwrap() == "1";
        let f = fixture_with_flags(mode, if external { 0x400000 } else { 0 });
        reserve(&f, 7, false);
        let old = ReadTxn::open_snapshot(&f.path).unwrap();
        let old_bytes = old.get(MAIN, b"k").unwrap().unwrap();
        reserve(&f, 9, true);
        if no_sync {
            assert_eq!(
                unsafe { environment::mdb_env_set_flags(f.env, durability::NOSYNC, 1) },
                0
            );
        }
        let before = fs::read(f.path.join("CURRENT")).unwrap();
        // Retain a real dirty mapped writer. The selected flush must not acquire
        // its native/common writer lease or flush its unpublished mmap range.
        let mut held = ptr::null_mut();
        assert_eq!(
            unsafe { transaction::mdb_txn_begin(f.env, ptr::null_mut(), 0, &mut held) },
            0
        );
        let info = match unsafe { (*held).state.as_ref().unwrap() } {
            State::Write(w) => w.mapped_allocation().unwrap().unwrap(),
            _ => unreachable!(),
        };
        let mut key = MDB_val {
            mv_size: 1,
            mv_data: b"k".as_ptr().cast_mut().cast(),
        };
        let mut value = MDB_val {
            mv_size: 4,
            mv_data: b"held".as_ptr().cast_mut().cast(),
        };
        assert_eq!(
            unsafe { operations::mdb_put(held, 1, &mut key, &mut value, 0) },
            0
        );
        eprintln!("MAPSYNC_PRIVATE={:x}:{}", info.address, info.capacity);
        eprintln!("MAPSYNC_ASYNC_BEGIN");
        assert_eq!(unsafe { environment::mdb_env_sync(f.env, 0) }, 0);
        eprintln!("MAPSYNC_ASYNC_END");
        assert_eq!(before, fs::read(f.path.join("CURRENT")).unwrap());
        assert_eq!(old_bytes, vec![7; 48_000]);
        let selected = ReadTxn::open_snapshot(&f.path).unwrap();
        assert_eq!(
            selected.get(MAIN, b"k").unwrap(),
            Some(vec![9; 48_000].as_slice())
        );
        // Async does not rewrite weak metadata/debt into a forged Full state.
        assert!(before.starts_with(b"ULMDBWK1"));
        eprintln!("MAPSYNC_FORCE_BEGIN");
        assert_eq!(unsafe { environment::mdb_env_sync(f.env, 1) }, 0);
        eprintln!("MAPSYNC_FORCE_END");
        assert_eq!(before, fs::read(f.path.join("CURRENT")).unwrap());
        assert_eq!(
            match unsafe { (*held).state.as_ref().unwrap() } {
                State::Write(w) => w.get(MAIN, b"k").unwrap(),
                _ => unreachable!(),
            },
            Some(b"held".as_slice())
        );
        unsafe { transaction::mdb_txn_abort(held) };
    }
    #[test]
    #[cfg(target_os = "linux")]
    fn nomeminit_runtime_toggles_sparse_stores_and_preserves_parent_after_rejected_child() {
        for mode in [
            ulmdb::PublicationMode::Legacy,
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            let f = fixture(mode);
            let mut txn = ptr::null_mut();
            assert_eq!(
                unsafe { transaction::mdb_txn_begin(f.env, ptr::null_mut(), 0, &mut txn) },
                0
            );
            let info = match unsafe { (*txn).state.as_ref().unwrap() } {
                State::Write(w) => w.mapped_allocation().unwrap().unwrap(),
                _ => unreachable!(),
            };
            let allocated = || {
                fs::read_dir(f.path.join("transactions"))
                    .unwrap()
                    .map(|e| {
                        ulmdb_platform::file_identity(&fs::File::open(e.unwrap().path()).unwrap())
                            .unwrap()
                    })
                    .find(|m| {
                        m.volume == info.file_device && m.file_id == u128::from(info.file_inode)
                    })
                    .unwrap()
                    .allocated_bytes
            };
            let put = |txn, name: &[u8], n| {
                let mut key = MDB_val {
                    mv_size: name.len(),
                    mv_data: name.as_ptr().cast_mut().cast(),
                };
                let mut value = MDB_val {
                    mv_size: n,
                    mv_data: ptr::null_mut(),
                };
                assert_eq!(
                    unsafe { operations::mdb_put(txn, 1, &mut key, &mut value, 0x10000) },
                    0
                );
                assert!(
                    unsafe { std::slice::from_raw_parts(value.mv_data.cast::<u8>(), n) }
                        .iter()
                        .all(|b| *b == 0)
                );
                value
            };
            let before = allocated();
            let value = put(txn, b"default", 512 << 10);
            assert!(allocated() - before >= (512 << 10) - 4096);
            unsafe { std::slice::from_raw_parts_mut(value.mv_data.cast::<u8>(), value.mv_size) }
                .fill(0x11);
            assert_eq!(
                unsafe { environment::mdb_env_set_flags(f.env, 0x1000000, 1) },
                0
            );
            let before = allocated();
            let value = put(txn, b"sparse", 512 << 10);
            assert!(
                allocated() - before < 128 << 10,
                "runtime enable must affect the existing writer"
            );
            (unsafe {
                std::slice::from_raw_parts_mut(value.mv_data.cast::<u8>(), value.mv_size)
            })[0] = 0x22;
            assert_eq!(
                unsafe { environment::mdb_env_set_flags(f.env, 0x1000000, 0) },
                0
            );
            let before = allocated();
            let _ = put(txn, b"initialized", 256 << 10);
            assert!(
                allocated() - before >= (256 << 10) - 4096,
                "runtime disable must restore stores"
            );
            assert_eq!(
                unsafe { environment::mdb_env_set_flags(f.env, 0x1000000, 1) },
                0
            );
            let mut child = ptr::null_mut();
            assert_eq!(
                unsafe { transaction::mdb_txn_begin(f.env, txn, 0, &mut child) },
                if unsafe { (&*txn).shared.catalog_terminator } {
                    libc::EINVAL
                } else {
                    BAD_TXN
                }
            );
            assert!(
                child.is_null(),
                "C WRITEMAP retains its no-write-child contract"
            );
            let value = put(txn, b"reuse", 65536);
            unsafe { std::slice::from_raw_parts_mut(value.mv_data.cast::<u8>(), value.mv_size) }
                .fill(0x33);
            assert_eq!(unsafe { transaction::mdb_txn_commit(txn) }, 0);
            let read = ReadTxn::open_snapshot(&f.path).unwrap();
            assert!(
                read.get(MAIN, b"reuse")
                    .unwrap()
                    .unwrap()
                    .iter()
                    .all(|b| *b == 0x33)
            );
            assert_eq!(read.get(MAIN, b"sparse").unwrap().unwrap()[0], 0x22);
        }
    }
}
