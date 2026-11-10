use super::*;
use crate::{cursor, environment, operations, transaction};
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
    fn new() -> Self {
        Self::with_flags(0)
    }
    fn with_flags(flags: u32) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-sql-batch-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        let mut env = ptr::null_mut();
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut env), 0);
            assert_eq!(environment::mdb_env_set_maxdbs(env, 16), 0);
            assert_eq!(environment::mdb_env_set_mapsize(env, 256 << 20), 0);
            assert_eq!(
                environment::mdb_env_open(env, name.as_ptr(), flags, 0o600),
                0
            );
            let txn = begin(env, false);
            let dbi = db(txn, "batch", CREATE);
            for id in 0..600u64 {
                assert_eq!(
                    operations::mdb_put(
                        txn,
                        dbi,
                        &mut val(&id.to_be_bytes()),
                        &mut val(&[id as u8; 64]),
                        0
                    ),
                    0
                );
            }
            for (name, flags) in [("duplicates", DUPSORT), ("reverse", 2), ("custom", 0)] {
                let dbi = db(txn, name, CREATE | flags);
                if name == "custom" {
                    assert_eq!(
                        operations::mdb_set_compare(txn, dbi, compare as *const () as *mut c_void),
                        0
                    );
                }
                assert_eq!(
                    operations::mdb_put(txn, dbi, &mut val(b"key"), &mut val(b"value"), 0),
                    0
                );
            }
            let typed = db(txn, "typed", CREATE);
            for (id, key) in [
                (i64::MIN, [0; 8]),
                (-7, [0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xf9]),
                (0, [0x80, 0, 0, 0, 0, 0, 0, 0]),
                (1, [0x80, 0, 0, 0, 0, 0, 0, 1]),
                (i64::MAX, [0xff; 8]),
            ] {
                let value = id.to_le_bytes();
                assert_eq!(
                    operations::mdb_put(
                        txn,
                        typed,
                        &mut val(&key),
                        &mut val(if id == 1 { &[] } else { &value }),
                        0
                    ),
                    0
                );
            }
            db(txn, "empty", CREATE);
            assert_eq!(transaction::mdb_txn_commit(txn), 0);
        }
        Self { env, path }
    }
    fn read(&self, name: Option<&str>) -> Reader {
        unsafe {
            let txn = begin(self.env, true);
            let mut dbi = 0;
            if let Some(name) = name {
                dbi = db(txn, name, 0);
            } else {
                assert_eq!(operations::mdb_dbi_open(txn, ptr::null(), 0, &mut dbi), 0);
            }
            let mut cursor = ptr::null_mut();
            assert_eq!(cursor::mdb_cursor_open(txn, dbi, &mut cursor), 0);
            Reader { txn, cursor, dbi }
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { environment::mdb_env_close(self.env) };
        std::fs::remove_dir_all(&self.path).unwrap();
    }
}
struct Reader {
    txn: *mut MDB_txn,
    cursor: *mut MDB_cursor,
    dbi: u32,
}
impl Drop for Reader {
    fn drop(&mut self) {
        unsafe {
            cursor::mdb_cursor_close(self.cursor);
            if !self.txn.is_null() {
                transaction::mdb_txn_abort(self.txn);
            }
        }
    }
}
fn val(b: &[u8]) -> MDB_val {
    MDB_val {
        mv_size: b.len(),
        mv_data: b.as_ptr().cast_mut().cast(),
    }
}
fn row() -> SqlRow {
    SqlRow {
        key: val(&[]),
        value: val(&[]),
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
unsafe fn db(txn: *mut MDB_txn, name: &str, flags: u32) -> u32 {
    let name = CString::new(name).unwrap();
    let mut dbi = 0;
    assert_eq!(
        unsafe { operations::mdb_dbi_open(txn, name.as_ptr(), flags, &mut dbi) },
        0
    );
    dbi
}
unsafe fn ordinary(cursor: *mut MDB_cursor, op: u32) -> (Vec<u8>, Vec<u8>) {
    let mut k = val(&[]);
    let mut v = val(&[]);
    assert_eq!(
        unsafe { cursor::mdb_cursor_get(cursor, &mut k, &mut v, op) },
        0
    );
    (unsafe { bytes(&k).unwrap().to_vec() }, unsafe {
        bytes(&v).unwrap().to_vec()
    })
}
fn id(r: &SqlRow) -> u64 {
    u64::from_be_bytes(unsafe { bytes(&r.key).unwrap().try_into().unwrap() })
}
unsafe extern "C" fn compare(a: *const MDB_val, b: *const MDB_val) -> c_int {
    match unsafe { bytes(a).unwrap().cmp(bytes(b).unwrap()) } {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}
#[test]
fn sql_batch_cardinality_mixed_cursor_and_borrowed_rows() {
    let f = Fixture::new();
    let r = f.read(Some("batch"));
    unsafe {
        let mut rows = [row(); 1024];
        let mut n = 0;
        assert_eq!(
            ulmdb_sql_cursor_batch(r.cursor, rows.as_mut_ptr(), 256, &mut n, 0),
            0
        );
        assert_eq!(n, 256);
        assert_eq!(
            rows[..n].iter().map(id).collect::<Vec<_>>(),
            (0..256).collect::<Vec<_>>()
        );
        let first = rows[0].value;
        assert_eq!(ordinary(r.cursor, 4).0, 255u64.to_be_bytes());
        assert_eq!(ordinary(r.cursor, 8).0, 256u64.to_be_bytes());
        assert_eq!(
            ulmdb_sql_cursor_batch(r.cursor, rows.as_mut_ptr(), 256, &mut n, 8),
            0
        );
        assert_eq!(
            rows[..n].iter().map(id).collect::<Vec<_>>(),
            (257..513).collect::<Vec<_>>()
        );
        assert_eq!(
            ulmdb_sql_cursor_batch(r.cursor, rows.as_mut_ptr(), 256, &mut n, 8),
            0
        );
        assert_eq!(
            rows[..n].iter().map(id).collect::<Vec<_>>(),
            (513..600).collect::<Vec<_>>()
        );
        assert_eq!(
            ulmdb_sql_cursor_batch(r.cursor, rows.as_mut_ptr(), 256, &mut n, 8),
            NOTFOUND
        );
        assert_eq!(n, 0);
        assert_eq!(bytes(&first).unwrap(), [0; 64]);
        assert_eq!(
            ulmdb_sql_cursor_batch(r.cursor, rows.as_mut_ptr(), 1024, &mut n, 0),
            0
        );
        assert_eq!(
            rows[..n].iter().map(id).collect::<Vec<_>>(),
            (0..600).collect::<Vec<_>>()
        );
    }
}
#[test]
fn sql_batch_output_bounds_and_ahead_candidate_preserve_every_row() {
    let f = Fixture::new();
    let r = f.read(Some("batch"));
    unsafe {
        let mut rows = [row(); 1025];
        let mut n = 99;
        rows[3].key.mv_size = 123456;
        assert_eq!(
            ulmdb_sql_cursor_batch(r.cursor, rows.as_mut_ptr(), 3, &mut n, 0),
            0
        );
        assert_eq!(n, 3);
        assert_eq!(rows[3].key.mv_size, 123456);
        for cap in [0, 1025] {
            assert_eq!(
                ulmdb_sql_cursor_batch(r.cursor, rows.as_mut_ptr(), cap, &mut n, 8),
                libc::EINVAL
            );
            assert_eq!(n, 0);
            assert_eq!(ordinary(r.cursor, 4).0, 2u64.to_be_bytes());
        }
        assert_eq!(
            ulmdb_sql_cursor_batch(r.cursor, rows.as_mut_ptr(), 1, &mut n, 6),
            libc::EINVAL
        );
        assert_eq!(
            ulmdb_sql_cursor_batch(r.cursor, ptr::null_mut(), 1, &mut n, 0),
            libc::EINVAL
        );
        // The same production byte-limit branch is exercised on real native
        // rows with a small limit, avoiding a 64 MiB correctness fixture.
        let mut seen = Vec::new();
        let mut op = 0;
        loop {
            let rc = batch(r.cursor, rows.as_mut_ptr(), 256, &mut n, op, 200);
            if rc == NOTFOUND {
                break;
            }
            assert_eq!(rc, 0);
            assert_eq!(n, 2);
            seen.extend(rows[..n].iter().map(id));
            op = 8;
        }
        assert_eq!(seen, (0..600).collect::<Vec<_>>());
        assert_eq!(
            batch(r.cursor, rows.as_mut_ptr(), 256, &mut n, 0, 71),
            BAD_VALSIZE
        );
        assert_eq!(n, 0);
        assert_eq!(ordinary(r.cursor, 4).0, 599u64.to_be_bytes());
        assert_eq!(ordinary(r.cursor, 0).0, 0u64.to_be_bytes());
        assert_eq!(ordinary(r.cursor, 8).0, 1u64.to_be_bytes());
        assert_eq!(
            batch(r.cursor, rows.as_mut_ptr(), 256, &mut n, 8, 71),
            BAD_VALSIZE
        );
        assert_eq!(n, 0);
        assert_eq!(ordinary(r.cursor, 4).0, 1u64.to_be_bytes());
        assert_eq!(ordinary(r.cursor, 8).0, 2u64.to_be_bytes());
        assert_eq!(
            ulmdb_sql_cursor_batch(r.cursor, rows.as_mut_ptr(), 1, &mut n, 0),
            0
        );
        assert_eq!(id(&rows[0]), 0);
    }
}

#[test]
fn sql_batch_local_cursor_preserves_generic_continuation_refusals_and_eof() {
    let f = Fixture::new();
    let r = f.read(Some("batch"));
    unsafe {
        let mut output = [row(); 256];
        let mut count = 99;
        assert_eq!(ordinary(r.cursor, 0).0, 0u64.to_be_bytes());
        assert_eq!(ordinary(r.cursor, 8).0, 1u64.to_be_bytes());
        assert!((*r.cursor).continuation.is_some());
        assert_eq!(
            batch(r.cursor, output.as_mut_ptr(), 256, &mut count, 8, 71),
            BAD_VALSIZE
        );
        assert_eq!(count, 0);
        assert!((*r.cursor).continuation.is_some());
        assert_eq!(ordinary(r.cursor, 4).0, 1u64.to_be_bytes());
        assert_eq!(ordinary(r.cursor, 8).0, 2u64.to_be_bytes());
        assert_eq!(
            batch(r.cursor, output.as_mut_ptr(), 256, &mut count, 8, 144),
            0
        );
        assert_eq!(count, 2);
        assert_eq!(output[..count].iter().map(id).collect::<Vec<_>>(), [3, 4]);
        assert!((*r.cursor).continuation.is_none());
        assert_eq!(ordinary(r.cursor, 4).0, 4u64.to_be_bytes());
        assert_eq!(ordinary(r.cursor, 12).0, 3u64.to_be_bytes());
        assert_eq!(
            ulmdb_sql_cursor_batch(r.cursor, output.as_mut_ptr(), 2, &mut count, 8),
            0
        );
        assert_eq!(output[..count].iter().map(id).collect::<Vec<_>>(), [4, 5]);
        assert!((*r.cursor).continuation.is_none());
        let key_bytes = 597u64.to_be_bytes();
        let mut key = val(&key_bytes);
        let mut value = val(&[]);
        assert_eq!(
            cursor::mdb_cursor_get(r.cursor, &mut key, &mut value, 17),
            0
        );
        assert_eq!(
            ulmdb_sql_cursor_batch(r.cursor, output.as_mut_ptr(), 256, &mut count, 8),
            0
        );
        assert_eq!(
            output[..count].iter().map(id).collect::<Vec<_>>(),
            [598, 599]
        );
        assert_eq!(ordinary(r.cursor, 4).0, 599u64.to_be_bytes());
        assert!((*r.cursor).continuation.is_some());
        for _ in 0..2 {
            assert_eq!(
                ulmdb_sql_cursor_batch(r.cursor, output.as_mut_ptr(), 256, &mut count, 8),
                NOTFOUND
            );
            assert_eq!(count, 0);
            assert!((*r.cursor).continuation.is_some());
            assert_eq!(ordinary(r.cursor, 4).0, 599u64.to_be_bytes());
        }
    }
}
#[test]
fn sql_batch_reset_abort_thread_and_fork_guards() {
    let f = Fixture::new();
    let mut r = f.read(Some("batch"));
    unsafe {
        let raw = r.cursor as usize;
        assert_eq!(
            std::thread::spawn(move || {
                let mut output = row();
                let mut n = 99;
                let rc = ulmdb_sql_cursor_batch(raw as *mut MDB_cursor, &mut output, 1, &mut n, 0);
                (rc, n)
            })
            .join()
            .unwrap(),
            (BAD_TXN, 0)
        );
        let mut output = row();
        let mut n = 99;
        assert_eq!(
            ulmdb_sql_cursor_batch(r.cursor, &mut output, 1, &mut n, 0),
            0
        );
        for raw_fork in [false, true] {
            let child = if raw_fork {
                libc::syscall(libc::SYS_fork) as libc::pid_t
            } else {
                libc::fork()
            };
            assert!(child >= 0);
            if child == 0 {
                let rc = ulmdb_sql_cursor_batch(r.cursor, &mut output, 1, &mut n, 8);
                libc::_exit(if rc == BAD_TXN && n == 0 { 0 } else { 1 });
            }
            let mut status = 0;
            assert_eq!(libc::waitpid(child, &mut status, 0), child);
            assert_eq!(status, 0);
        }
        transaction::mdb_txn_reset(r.txn);
        assert_eq!(
            ulmdb_sql_cursor_batch(r.cursor, &mut output, 1, &mut n, 0),
            BAD_TXN
        );
        assert_eq!(transaction::mdb_txn_renew(r.txn), 0);
        assert_eq!(cursor::mdb_cursor_renew(r.txn, r.cursor), 0);
        assert_eq!(
            ulmdb_sql_cursor_batch(r.cursor, &mut output, 1, &mut n, 0),
            0
        );
        assert_eq!(id(&output), 0);
        transaction::mdb_txn_abort(r.txn);
        r.txn = ptr::null_mut();
        assert_eq!(
            ulmdb_sql_cursor_batch(r.cursor, &mut output, 1, &mut n, 8),
            libc::EINVAL
        );
        assert_eq!(n, 0);
    }
}
#[test]
fn sql_batch_unsupported_modes_preserve_generic_bookmark() {
    let f = Fixture::new();
    unsafe {
        for name in [None, Some("duplicates"), Some("reverse"), Some("custom")] {
            let r = f.read(name);
            let before = ordinary(r.cursor, 0);
            let mut output = row();
            let mut n = 99;
            assert_eq!(
                ulmdb_sql_cursor_batch(r.cursor, &mut output, 1, &mut n, 8),
                INCOMPATIBLE,
                "{name:?}"
            );
            assert_eq!(n, 0);
            assert_eq!(ordinary(r.cursor, 4), before);
        }
        let t = begin(f.env, false);
        let dbi = db(t, "batch", 0);
        let mut cursor = ptr::null_mut();
        assert_eq!(cursor::mdb_cursor_open(t, dbi, &mut cursor), 0);
        let before = ordinary(cursor, 0);
        let mut output = row();
        let mut n = 99;
        assert_eq!(
            ulmdb_sql_cursor_batch(cursor, &mut output, 1, &mut n, 8),
            INCOMPATIBLE
        );
        assert_eq!(n, 0);
        assert_eq!(ordinary(cursor, 4), before);
        cursor::mdb_cursor_close(cursor);
        transaction::mdb_txn_abort(t);
    }
}

#[test]
fn sql_batch_notls_allows_sequential_thread_handoff() {
    let fixture = Fixture::with_flags(0x200000);
    let reader = fixture.read(Some("batch"));
    let raw = reader.cursor as usize;
    assert_eq!(
        std::thread::spawn(move || unsafe {
            let mut output = row();
            let mut count = 0;
            assert_eq!(
                ulmdb_sql_cursor_batch(raw as *mut MDB_cursor, &mut output, 1, &mut count, 0),
                0
            );
            (count, id(&output))
        })
        .join()
        .unwrap(),
        (1, 0)
    );
    unsafe {
        assert_eq!(ordinary(reader.cursor, 8).0, 1u64.to_be_bytes());
    }
}

#[test]
fn sql_bytewise_format_checks_native_read_write_and_reopened_custom() {
    let mut f = Fixture::new();
    unsafe {
        // Drop all process-local comparator callbacks while preserving the
        // native catalog's Custom ordering and the ordinary flags0 metadata.
        let old = f.env;
        f.env = ptr::null_mut();
        environment::mdb_env_close(old);
        let name = CString::new(f.path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(environment::mdb_env_create(&mut f.env), 0);
        assert_eq!(environment::mdb_env_set_maxdbs(f.env, 16), 0);
        assert_eq!(environment::mdb_env_set_mapsize(f.env, 256 << 20), 0);
        assert_eq!(environment::mdb_env_open(f.env, name.as_ptr(), 0, 0o600), 0);
        for readonly in [true, false] {
            let t = begin(f.env, readonly);
            let ordinary = db(t, "batch", 0);
            assert_eq!(ulmdb_sql_bytewise_format(t, ordinary), 0);
            for name in ["custom", "duplicates", "reverse"] {
                let dbi = db(t, name, 0);
                if name == "custom" {
                    let mut flags = 99;
                    assert_eq!(operations::mdb_dbi_flags(t, dbi, &mut flags), 0);
                    assert_eq!(flags, 0);
                    let txn = &mut *t;
                    let handle = txn.handle(dbi).unwrap();
                    assert_eq!(
                        txn.actual_native_order(&handle).unwrap().keys,
                        ulmdb::ordering::OrderingSpec::Custom
                    );
                }
                assert_eq!(ulmdb_sql_bytewise_format(t, dbi), INCOMPATIBLE, "{name}");
            }
            let mut main = 0;
            assert_eq!(operations::mdb_dbi_open(t, ptr::null(), 0, &mut main), 0);
            assert_eq!(ulmdb_sql_bytewise_format(t, main), INCOMPATIBLE);
            transaction::mdb_txn_abort(t);
        }
        // Even a bytewise physical tree cannot authorize process-local custom
        // callbacks. Format admission must not configure or mutate the tree.
        let t = begin(f.env, true);
        let dbi = db(t, "batch", 0);
        assert_eq!(
            operations::mdb_set_compare(t, dbi, compare as *const () as *mut c_void),
            0
        );
        assert_eq!(ulmdb_sql_bytewise_format(t, dbi), INCOMPATIBLE);
        assert_eq!(operations::mdb_set_compare(t, dbi, ptr::null_mut()), 0);
        assert_eq!(ulmdb_sql_bytewise_format(t, dbi), 0);
        let raw = t as usize;
        assert_eq!(
            std::thread::spawn(move || ulmdb_sql_bytewise_format(raw as *mut MDB_txn, dbi))
                .join()
                .unwrap(),
            BAD_TXN
        );
        let child = libc::fork();
        assert!(child >= 0);
        if child == 0 {
            libc::_exit(if ulmdb_sql_bytewise_format(t, dbi) == BAD_TXN {
                0
            } else {
                1
            });
        }
        let mut status = 0;
        assert_eq!(libc::waitpid(child, &mut status, 0), child);
        assert_eq!(status, 0);
        transaction::mdb_txn_reset(t);
        assert_eq!(ulmdb_sql_bytewise_format(t, dbi), BAD_TXN);
        assert_eq!(transaction::mdb_txn_renew(t), 0);
        assert_eq!(ulmdb_sql_bytewise_format(t, dbi), 0);
        transaction::mdb_txn_abort(t);
        assert_eq!(
            ulmdb_sql_bytewise_format(ptr::null_mut(), dbi),
            libc::EINVAL
        );
    }
}

fn hit() -> SqlHit {
    SqlHit {
        id: 0,
        value: val(&[]),
    }
}
fn typed_value(id: i64) -> Vec<u8> {
    if id == 1 {
        Vec::new()
    } else {
        id.to_le_bytes().to_vec()
    }
}
unsafe fn lookup(reader: &Reader, ids: &[i64], out: &mut [SqlHit]) -> (i32, usize, usize) {
    let mut consumed = 99;
    let mut found = 99;
    let rc = unsafe {
        ulmdb_sql_get_batch(
            reader.txn,
            reader.dbi,
            ids.as_ptr(),
            ids.len(),
            out.as_mut_ptr(),
            out.len(),
            &mut consumed,
            &mut found,
        )
    };
    (rc, consumed, found)
}
#[test]
fn sql_get_batch_signed_misses_duplicates_partial_and_borrowed_values() {
    let f = Fixture::new();
    let r = f.read(Some("typed"));
    unsafe {
        let mut out = [hit(); 8];
        let ids = [i64::MIN, -7, 42, 0, -7, 1, i64::MAX];
        assert_eq!(lookup(&r, &ids, &mut out), (0, 7, 6));
        let expected = [i64::MIN, -7, 0, -7, 1, i64::MAX];
        for (row, id) in out[..6].iter().zip(expected) {
            assert_eq!(row.id, id);
            assert_eq!(bytes(&row.value).unwrap(), typed_value(id));
        }
        let borrowed = out[0].value;
        let ids = [42, -7, 43, 0, i64::MAX];
        out[2].id = 123456;
        assert_eq!(lookup(&r, &ids, &mut out[..2]), (0, 4, 2));
        assert_eq!([out[0].id, out[1].id], [-7, 0]);
        assert_eq!(out[2].id, 123456);
        assert_eq!(lookup(&r, &ids[4..], &mut out), (0, 1, 1));
        assert_eq!(out[0].id, i64::MAX);
        assert_eq!(bytes(&borrowed).unwrap(), i64::MIN.to_le_bytes());
        assert_eq!(lookup(&r, &[2, 3, 42], &mut out), (0, 3, 0));
        drop(r);
        let empty = f.read(Some("empty"));
        assert_eq!(lookup(&empty, &[0, 1], &mut out), (0, 2, 0));
    }
}
#[test]
fn sql_get_batch_budget_limits_and_empty_inputs_make_exact_progress() {
    let f = Fixture::new();
    let r = f.read(Some("typed"));
    unsafe {
        let ids = [42, -7, 43, 0, 1];
        let mut out = [hit(); 8];
        let mut consumed = 99;
        let mut found = 99;
        assert_eq!(
            get_batch(
                r.txn,
                r.dbi,
                ids.as_ptr(),
                ids.len(),
                out.as_mut_ptr(),
                8,
                &mut consumed,
                &mut found,
                20
            ),
            0
        );
        assert_eq!((consumed, found, out[0].id), (3, 1, -7));
        assert_eq!(
            get_batch(
                r.txn,
                r.dbi,
                ids[3..].as_ptr(),
                2,
                out.as_mut_ptr(),
                8,
                &mut consumed,
                &mut found,
                20
            ),
            0
        );
        assert_eq!((consumed, found, out[0].id), (1, 1, 0));
        assert_eq!(
            get_batch(
                r.txn,
                r.dbi,
                ids.as_ptr(),
                ids.len(),
                out.as_mut_ptr(),
                8,
                &mut consumed,
                &mut found,
                15
            ),
            BAD_VALSIZE
        );
        assert_eq!((consumed, found), (0, 0));
        assert_eq!(lookup(&r, &[-7], &mut out), (0, 1, 1));
        assert_eq!(
            ulmdb_sql_get_batch(
                r.txn,
                r.dbi,
                ptr::null(),
                0,
                ptr::null_mut(),
                0,
                &mut consumed,
                &mut found
            ),
            0
        );
        assert_eq!((consumed, found), (0, 0));
        for (count, capacity) in [(1025, 8), (1, 1025), (1, 0)] {
            assert_eq!(
                ulmdb_sql_get_batch(
                    r.txn,
                    r.dbi,
                    ids.as_ptr(),
                    count,
                    out.as_mut_ptr(),
                    capacity,
                    &mut consumed,
                    &mut found
                ),
                libc::EINVAL
            );
            assert_eq!((consumed, found), (0, 0));
        }
        assert_eq!(
            ulmdb_sql_get_batch(
                r.txn,
                r.dbi,
                ptr::null(),
                1,
                out.as_mut_ptr(),
                8,
                &mut consumed,
                &mut found
            ),
            libc::EINVAL
        );
        assert_eq!(
            ulmdb_sql_get_batch(
                r.txn,
                r.dbi,
                ids.as_ptr(),
                1,
                ptr::null_mut(),
                8,
                &mut consumed,
                &mut found
            ),
            libc::EINVAL
        );
    }
}
#[test]
fn sql_get_batch_unsupported_sources_retain_cursor_and_writer_state() {
    let f = Fixture::new();
    unsafe {
        for name in [None, Some("duplicates"), Some("reverse"), Some("custom")] {
            let r = f.read(name);
            let original = ordinary(r.cursor, 0);
            let mut out = [hit(); 2];
            assert_eq!(lookup(&r, &[0], &mut out), (INCOMPATIBLE, 0, 0));
            assert_eq!(ordinary(r.cursor, 4), original);
        }
        let txn = begin(f.env, false);
        let dbi = db(txn, "typed", 0);
        let mut out = [hit(); 1];
        let mut consumed = 99;
        let mut found = 99;
        assert_eq!(
            ulmdb_sql_get_batch(
                txn,
                dbi,
                [0].as_ptr(),
                1,
                out.as_mut_ptr(),
                1,
                &mut consumed,
                &mut found
            ),
            INCOMPATIBLE
        );
        assert_eq!((consumed, found), (0, 0));
        transaction::mdb_txn_abort(txn);
    }
}
#[test]
fn sql_get_batch_snapshot_renew_thread_and_process_lifetimes() {
    let f = Fixture::new();
    let mut r = f.read(Some("typed"));
    unsafe {
        let mut out = [hit(); 2];
        assert_eq!(lookup(&r, &[0], &mut out), (0, 1, 1));
        let txn = r.txn as usize;
        let dbi = r.dbi;
        assert_eq!(
            std::thread::spawn(move || {
                let mut out = [hit(); 1];
                let mut consumed = 99;
                let mut found = 99;
                let rc = ulmdb_sql_get_batch(
                    txn as *mut MDB_txn,
                    dbi,
                    [0].as_ptr(),
                    1,
                    out.as_mut_ptr(),
                    1,
                    &mut consumed,
                    &mut found,
                );
                (rc, consumed, found)
            })
            .join()
            .unwrap(),
            (BAD_TXN, 0, 0)
        );
        for raw_fork in [false, true] {
            let child = if raw_fork {
                libc::syscall(libc::SYS_fork) as libc::pid_t
            } else {
                libc::fork()
            };
            assert!(child >= 0);
            if child == 0 {
                let (rc, consumed, found) = lookup(&r, &[0], &mut out);
                libc::_exit(if (rc, consumed, found) == (BAD_TXN, 0, 0) {
                    0
                } else {
                    1
                });
            }
            let mut status = 0;
            assert_eq!(libc::waitpid(child, &mut status, 0), child);
            assert_eq!(status, 0);
        }
        transaction::mdb_txn_reset(r.txn);
        assert_eq!(lookup(&r, &[0], &mut out), (BAD_TXN, 0, 0));
        let writer = begin(f.env, false);
        let dbi = db(writer, "typed", 0);
        assert_eq!(
            operations::mdb_put(
                writer,
                dbi,
                &mut val(&[0x80, 0, 0, 0, 0, 0, 0, 0]),
                &mut val(b"changed"),
                0
            ),
            0
        );
        assert_eq!(transaction::mdb_txn_commit(writer), 0);
        assert_eq!(transaction::mdb_txn_renew(r.txn), 0);
        r.dbi = db(r.txn, "typed", 0);
        assert_eq!(cursor::mdb_cursor_renew(r.txn, r.cursor), 0);
        assert_eq!(lookup(&r, &[0], &mut out), (0, 1, 1));
        assert_eq!(bytes(&out[0].value).unwrap(), b"changed");
        transaction::mdb_txn_abort(r.txn);
        r.txn = ptr::null_mut();
        let mut row = row();
        let mut count = 99;
        assert_eq!(
            ulmdb_sql_cursor_batch(r.cursor, &mut row, 1, &mut count, 0),
            libc::EINVAL
        );
    }
    drop(r);
    let f = Fixture::with_flags(0x200000);
    let r = f.read(Some("typed"));
    let txn = r.txn as usize;
    let dbi = r.dbi;
    assert_eq!(
        std::thread::spawn(move || unsafe {
            let mut out = [hit(); 1];
            let mut consumed = 99;
            let mut found = 99;
            let rc = ulmdb_sql_get_batch(
                txn as *mut MDB_txn,
                dbi,
                [0].as_ptr(),
                1,
                out.as_mut_ptr(),
                1,
                &mut consumed,
                &mut found,
            );
            (rc, consumed, found)
        })
        .join()
        .unwrap(),
        (0, 1, 1)
    );
}
