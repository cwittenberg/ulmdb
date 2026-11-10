use super::*;
use crate::{environment, operations, transaction};
use std::{
    ffi::CString,
    sync::atomic::{AtomicU64, Ordering},
};

static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    env: *mut MDB_env,
    path: std::path::PathBuf,
}
impl Fixture {
    fn new(flags: u32) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-sql-hash-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        let mut env = ptr::null_mut();
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut env), 0);
            assert_eq!(environment::mdb_env_set_maxdbs(env, 512), 0);
            assert_eq!(environment::mdb_env_set_mapsize(env, 1 << 30), 0);
            assert_eq!(
                environment::mdb_env_open(env, name.as_ptr(), flags, 0o600),
                0
            );
            let t = begin(env, false);
            for (name, byte) in [("a", b'A'), ("b", b'B')] {
                let dbi = db(t, name, CREATE);
                for id in 0..4 {
                    let key = encode(id);
                    let data = if id == 3 && name == "a" {
                        Vec::new()
                    } else {
                        vec![byte; 64]
                    };
                    assert_eq!(
                        operations::mdb_put(t, dbi, &mut val(&key), &mut val(&data), 0),
                        0
                    );
                }
            }
            let letters = db(t, "letters", CREATE);
            assert_eq!(
                operations::mdb_put(t, letters, &mut val(b"alpha"), &mut val(b"letter"), 0),
                0
            );
            assert_eq!(transaction::mdb_txn_commit(t), 0);
        }
        Self { env, path }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { environment::mdb_env_close(self.env) };
        std::fs::remove_dir_all(&self.path).unwrap();
    }
}
fn encode(id: i64) -> [u8; 8] {
    ((id as u64) ^ 0x8000_0000_0000_0000).to_be_bytes()
}
fn val(bytes: &[u8]) -> MDB_val {
    MDB_val {
        mv_size: bytes.len(),
        mv_data: bytes.as_ptr().cast_mut().cast(),
    }
}
unsafe fn begin(env: *mut MDB_env, readonly: bool) -> *mut MDB_txn {
    let mut t = ptr::null_mut();
    assert_eq!(
        unsafe {
            transaction::mdb_txn_begin(
                env,
                ptr::null_mut(),
                if readonly { RDONLY } else { 0 },
                &mut t,
            )
        },
        0
    );
    t
}
unsafe fn db(t: *mut MDB_txn, name: &str, flags: u32) -> u32 {
    let name = CString::new(name).unwrap();
    let mut dbi = 0;
    assert_eq!(
        unsafe { operations::mdb_dbi_open(t, name.as_ptr(), flags, &mut dbi) },
        0
    );
    dbi
}
unsafe fn get(t: *mut MDB_txn, dbi: u32, key: &[u8]) -> Result<Vec<u8>, i32> {
    let mut value = val(&[]);
    let rc = unsafe { operations::mdb_get(t, dbi, &mut val(key), &mut value) };
    if rc != 0 {
        Err(rc)
    } else {
        Ok(unsafe { bytes(&value) }.unwrap().to_vec())
    }
}
unsafe fn stats(t: *mut MDB_txn) -> SqlHashStats {
    let mut out = SqlHashStats::default();
    assert_eq!(unsafe { ulmdb_sql_hash_stats(t, &mut out) }, 0);
    out
}
unsafe fn limits(t: *mut MDB_txn, edit: impl FnOnce(&mut HashLimits)) {
    let Some(State::Read(read)) = unsafe { &*t }.state.as_ref() else {
        panic!()
    };
    read.cell
        .with_dependent(|_, dependent| edit(&mut dependent.0.borrow_mut().limits));
}
unsafe extern "C" fn folded(a: *const MDB_val, b: *const MDB_val) -> c_int {
    let a = unsafe { bytes(a) }
        .unwrap()
        .iter()
        .map(u8::to_ascii_lowercase)
        .collect::<Vec<_>>();
    let b = unsafe { bytes(b) }
        .unwrap()
        .iter()
        .map(u8::to_ascii_lowercase)
        .collect::<Vec<_>>();
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

#[test]
fn sql_hash_ordinary_get_aliasing_misses_empty_values_and_lazy_databases() {
    let f = Fixture::new(0);
    unsafe {
        let t = begin(f.env, true);
        let a = db(t, "a", 0);
        let b = db(t, "b", 0);
        assert_eq!(stats(t).preparations, 0);
        let mut empty_key = val(&[]);
        let mut sentinel = MDB_val {
            mv_size: 123,
            mv_data: 123usize as *mut c_void,
        };
        assert_eq!(
            operations::mdb_get(t, a, &mut empty_key, &mut sentinel),
            BAD_VALSIZE
        );
        assert_eq!((sentinel.mv_size, sentinel.mv_data as usize), (123, 123));
        assert_eq!(stats(t).preparations, 0);
        let key = encode(0);
        let mut alias = val(&key);
        assert_eq!(operations::mdb_get(t, a, &mut alias, &mut alias), 0);
        assert_eq!(bytes(&alias).unwrap(), [b'A'; 64]);
        assert_eq!(
            (
                stats(t).preparations,
                stats(t).prepared_databases,
                stats(t).entries
            ),
            (1, 1, 4)
        );
        let missing = encode(99);
        let mut alias = val(&missing);
        let before = (alias.mv_size, alias.mv_data);
        assert_eq!(operations::mdb_get(t, a, &mut alias, &mut alias), NOTFOUND);
        assert_eq!((alias.mv_size, alias.mv_data), before);
        assert!(get(t, a, &encode(3)).unwrap().is_empty());
        assert_eq!(get(t, b, &encode(1)).unwrap(), [b'B'; 64]);
        assert_eq!(
            (
                stats(t).preparations,
                stats(t).prepared_databases,
                stats(t).entries
            ),
            (2, 2, 8)
        );
        assert_eq!(
            operations::mdb_get(t, a, &mut empty_key, &mut sentinel),
            BAD_VALSIZE
        );
        assert_eq!(stats(t).preparations, 2);
        transaction::mdb_txn_abort(t);
    }
}

#[test]
fn sql_hash_typed_batch_and_ordinary_get_share_one_exact_index() {
    let f = Fixture::new(0);
    unsafe {
        let t = begin(f.env, true);
        let a = db(t, "a", 0);
        let ids = [-1, 0, 1, 1, 3, 99];
        let mut out = [super::super::SqlHit {
            id: 0,
            value: val(&[]),
        }; 8];
        let (mut consumed, mut found) = (99, 99);
        assert_eq!(
            super::super::ulmdb_sql_get_batch(
                t,
                a,
                ids.as_ptr(),
                ids.len(),
                out.as_mut_ptr(),
                out.len(),
                &mut consumed,
                &mut found
            ),
            0
        );
        assert_eq!((consumed, found), (6, 4));
        assert_eq!(
            out[..found].iter().map(|hit| hit.id).collect::<Vec<_>>(),
            [0, 1, 1, 3]
        );
        for hit in &out[..3] {
            assert_eq!(bytes(&hit.value).unwrap(), [b'A'; 64]);
        }
        assert_eq!(out[3].value.mv_size, 0);
        assert_eq!(get(t, a, &encode(2)).unwrap(), [b'A'; 64]);
        assert_eq!((stats(t).preparations, stats(t).prepared_databases), (1, 1));
        transaction::mdb_txn_abort(t);
    }
}

#[test]
fn sql_hash_budgets_are_aggregate_and_refusals_keep_native_values() {
    let f = Fixture::new(0);
    unsafe {
        for dimension in 0..3 {
            let t = begin(f.env, true);
            let a = db(t, "a", 0);
            let b = db(t, "b", 0);
            limits(t, |limit| match dimension {
                0 => limit.max_entries = 6,
                1 => limit.max_index_bytes = ulmdb::lookup::hash_index_bytes(4).unwrap() + 8,
                _ => limit.max_total_value_bytes = 256,
            });
            assert_eq!(get(t, a, &encode(0)).unwrap(), [b'A'; 64]);
            for _ in 0..3 {
                assert_eq!(get(t, b, &encode(0)).unwrap(), [b'B'; 64]);
            }
            let s = stats(t);
            assert_eq!(
                (
                    s.preparations,
                    s.prepared_databases,
                    s.refused_databases,
                    s.entries
                ),
                (2, 1, 1, 4)
            );
            transaction::mdb_txn_abort(t);
        }
        let t = begin(f.env, true);
        let a = db(t, "a", 0);
        limits(t, |limit| limit.max_value_bytes = 32);
        for _ in 0..3 {
            assert_eq!(get(t, a, &encode(0)).unwrap(), [b'A'; 64]);
        }
        assert_eq!(
            (
                stats(t).preparations,
                stats(t).prepared_databases,
                stats(t).refused_databases
            ),
            (1, 0, 1)
        );
        transaction::mdb_txn_abort(t);
    }
}

#[test]
fn sql_hash_current_callbacks_override_an_existing_index() {
    let f = Fixture::new(0);
    unsafe {
        let t = begin(f.env, true);
        let letters = db(t, "letters", 0);
        assert_eq!(get(t, letters, b"alpha").unwrap(), b"letter");
        assert_eq!(
            operations::mdb_set_compare(t, letters, folded as *const () as *mut c_void),
            0
        );
        assert_eq!(get(t, letters, b"ALPHA").unwrap(), b"letter");
        assert_eq!(stats(t).preparations, 1);
        assert_eq!(operations::mdb_set_compare(t, letters, ptr::null_mut()), 0);
        assert_eq!(get(t, letters, b"ALPHA"), Err(NOTFOUND));
        assert_eq!(get(t, letters, b"alpha").unwrap(), b"letter");
        assert_eq!(stats(t).preparations, 1);
        transaction::mdb_txn_abort(t);
    }
}

#[test]
fn sql_hash_reset_renew_releases_old_state_and_captures_new_names_and_head() {
    let f = Fixture::new(0);
    unsafe {
        let t = begin(f.env, true);
        let a = db(t, "a", 0);
        assert_eq!(get(t, a, &encode(0)).unwrap(), [b'A'; 64]);
        let mut cursor0 = ptr::null_mut();
        assert_eq!(crate::cursor::mdb_cursor_open(t, a, &mut cursor0), 0);
        transaction::mdb_txn_reset(t);
        assert!((*t).state.is_none());
        assert!((*t).reader_lease.is_none());
        let mut output = SqlHashStats::default();
        assert_eq!(ulmdb_sql_hash_stats(t, &mut output), BAD_TXN);
        let w = begin(f.env, false);
        let wa = db(w, "a", 0);
        assert_eq!(
            operations::mdb_put(w, wa, &mut val(&encode(0)), &mut val(b"new-head"), 0),
            0
        );
        let c = db(w, "new-database", CREATE);
        assert_eq!(
            operations::mdb_put(w, c, &mut val(b"new"), &mut val(b"catalog"), 0),
            0
        );
        assert_eq!(transaction::mdb_txn_commit(w), 0);
        assert_eq!(transaction::mdb_txn_renew(t), 0);
        assert_eq!(stats(t).preparations, 0);
        let a = db(t, "a", 0);
        let c = db(t, "new-database", 0);
        assert_eq!(get(t, a, &encode(0)).unwrap(), b"new-head");
        assert_eq!(get(t, c, b"new").unwrap(), b"catalog");
        assert_eq!(stats(t).prepared_databases, 2);
        assert_eq!(crate::cursor::mdb_cursor_renew(t, cursor0), 0);
        crate::cursor::mdb_cursor_close(cursor0);
        transaction::mdb_txn_abort(t);
    }
}

#[test]
fn sql_hash_thread_fork_guards_and_notls_sequential_handoff() {
    for flags in [0, 0x200000] {
        let f = Fixture::new(flags);
        unsafe {
            let t = begin(f.env, true);
            let a = db(t, "a", 0);
            assert_eq!(get(t, a, &encode(0)).unwrap(), [b'A'; 64]);
            let raw = t as usize;
            let result = std::thread::spawn(move || get(raw as *mut MDB_txn, a, &encode(0)))
                .join()
                .unwrap();
            if flags == 0 {
                assert_eq!(result, Err(BAD_TXN));
            } else {
                assert_eq!(result.unwrap(), [b'A'; 64]);
            }
            assert_eq!(stats(t).preparations, 1);
            for raw_fork in [false, true] {
                let child = if raw_fork {
                    libc::syscall(libc::SYS_fork) as libc::pid_t
                } else {
                    libc::fork()
                };
                assert!(child >= 0);
                if child == 0 {
                    libc::_exit(if get(t, a, &encode(0)) == Err(BAD_TXN) {
                        0
                    } else {
                        1
                    });
                }
                let mut status = 0;
                assert_eq!(libc::waitpid(child, &mut status, 0), child);
                assert_eq!(status, 0);
            }
            assert_eq!(get(t, a, &encode(0)).unwrap(), [b'A'; 64]);
            transaction::mdb_txn_abort(t);
        }
    }
}

#[test]
fn sql_hash_inventory_limit_disables_only_indexing() {
    let f = Fixture::new(0);
    unsafe {
        let w = begin(f.env, false);
        for index in 0..MAX_DATABASES {
            db(w, &format!("extra-{index}"), CREATE);
        }
        assert_eq!(transaction::mdb_txn_commit(w), 0);
        let t = begin(f.env, true);
        let a = db(t, "a", 0);
        assert_eq!(get(t, a, &encode(0)).unwrap(), [b'A'; 64]);
        assert_eq!((stats(t).preparations, stats(t).prepared_databases), (0, 0));
        transaction::mdb_txn_abort(t);
    }
}

#[test]
fn sql_hash_cached_native_identity_and_closed_public_dbi_are_not_retargeted() {
    let f = Fixture::new(0);
    unsafe {
        let t = begin(f.env, true);
        let a = db(t, "a", 0);
        assert_eq!(get(t, a, &encode(0)).unwrap(), [b'A'; 64]);
        let admitted = (*t).handle(a).unwrap().actual().clone();
        let other_path = f.path.join("other-owner");
        let other = Environment::create(&other_path).unwrap();
        let mut writer = other.begin_write().unwrap();
        writer
            .create_database(admitted.name(), DatabaseKind::Kv)
            .unwrap();
        writer
            .put(admitted.name(), &encode(0), b"wrong-owner")
            .unwrap();
        writer.commit().unwrap();
        let other_read = other.begin_read();
        let foreign = other_read.open_database(admitted.name()).unwrap();
        assert_ne!(foreign, admitted);
        let Some(State::Read(read)) = (*t).state.as_ref() else {
            panic!()
        };
        match read.get_after_admission(&foreign, &encode(0)) {
            Err(error) => assert_eq!(error, code(Error::StaleHandle)),
            Ok(_) => panic!("foreign native owner was admitted"),
        }
        // A later generation of the same native name also cannot retarget an
        // index that remains attached to its original immutable read snapshot.
        let pinned = SqlReadTxn::try_new(other.begin_read()).unwrap();
        let old = pinned.open_database(admitted.name()).unwrap();
        assert!(
            pinned
                .get_after_admission(&old, &encode(0))
                .unwrap()
                .is_some()
        );
        let mut writer = other.begin_write().unwrap();
        writer.drop_database(admitted.name()).unwrap();
        writer
            .create_database(admitted.name(), DatabaseKind::Kv)
            .unwrap();
        writer
            .put(admitted.name(), &encode(0), b"new-generation")
            .unwrap();
        writer.commit().unwrap();
        let fresh_read = other.begin_read();
        let fresh = fresh_read.open_database(admitted.name()).unwrap();
        assert_ne!(old.id(), fresh.id());
        match pinned.get_after_admission(&fresh, &encode(0)) {
            Err(error) => assert_eq!(error, code(Error::StaleHandle)),
            Ok(_) => panic!("recreated native name retargeted an old index"),
        }
        assert_eq!(get(t, a, &encode(0)).unwrap(), [b'A'; 64]);
        assert_eq!(stats(t).preparations, 1);
        operations::mdb_dbi_close(f.env, a);
        assert!(get(t, a, &encode(0)).is_err());
        let reopened = db(t, "a", 0);
        assert_eq!(get(t, reopened, &encode(0)).unwrap(), [b'A'; 64]);
        assert_eq!(stats(t).preparations, 1);
        transaction::mdb_txn_abort(t);
        drop(other_read);
        drop(fresh_read);
        drop(pinned);
        drop(other);
    }
}
