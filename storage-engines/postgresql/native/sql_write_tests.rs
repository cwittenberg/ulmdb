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
        let path = std::env::temp_dir().join(format!(
            "ulmdb-sql-write-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        drop(Environment::create(&path).unwrap());
        std::fs::write(path.join("LMDB-ABI"), b"1.0\n").unwrap();
        let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        let mut env = ptr::null_mut();
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut env), 0);
            assert_eq!(environment::mdb_env_set_maxdbs(env, 32), 0);
            assert_eq!(environment::mdb_env_set_mapsize(env, 1 << 20), 0);
            assert_eq!(
                environment::mdb_env_open(env, name.as_ptr(), 0x200000, 0o600),
                0
            );
            let w = begin(env, ptr::null_mut(), false);
            let kv = db(w, c"kv".as_ptr(), CREATE);
            assert_eq!(put(w, kv, b"a", b"old", 0), 0);
            assert_eq!(transaction::mdb_txn_commit(w), 0);
        }
        Self { env, path }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe {
            environment::mdb_env_close(self.env);
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
unsafe fn begin(env: *mut MDB_env, parent: *mut MDB_txn, read: bool) -> *mut MDB_txn {
    let mut out = ptr::null_mut();
    assert_eq!(
        unsafe {
            transaction::mdb_txn_begin(env, parent, if read { 0x20000 } else { 0 }, &mut out)
        },
        0
    );
    out
}
fn val(value: &[u8]) -> MDB_val {
    MDB_val {
        mv_size: value.len(),
        mv_data: value.as_ptr().cast_mut().cast(),
    }
}
unsafe fn db(txn: *mut MDB_txn, name: *const c_char, flags: u32) -> u32 {
    let mut dbi = 0;
    assert_eq!(
        unsafe { operations::mdb_dbi_open(txn, name, flags, &mut dbi) },
        0
    );
    dbi
}
unsafe fn put(txn: *mut MDB_txn, dbi: u32, key: &[u8], value: &[u8], flags: u32) -> c_int {
    unsafe { operations::mdb_put(txn, dbi, &mut val(key), &mut val(value), flags) }
}
unsafe fn get(txn: *mut MDB_txn, dbi: u32, key: &[u8]) -> FfiResult<Vec<u8>> {
    let mut out = val(&[]);
    let rc = unsafe { operations::mdb_get(txn, dbi, &mut val(key), &mut out) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(unsafe { bytes(&out)? }.to_vec())
}
fn lookups() -> usize {
    LOOKUPS.with(std::cell::Cell::get)
}

#[test]
fn sql_nooverwrite_writable_view_one_lookup_and_retained_duplicate_output() {
    let f = Fixture::new();
    unsafe {
        let w = begin(f.env, ptr::null_mut(), false);
        let kv = db(w, c"kv".as_ptr(), 0);
        assert!(matches!(
            (*w).handle(kv).unwrap().binding.view,
            DataView::Writable(_)
        ));
        let mut cursor = ptr::null_mut();
        assert_eq!(cursor::mdb_cursor_open(w, kv, &mut cursor), 0);
        let mut key = val(&[]);
        let mut data = val(&[]);
        assert_eq!(cursor::mdb_cursor_get(cursor, &mut key, &mut data, 0), 0);
        let before = lookups();
        assert_eq!(put(w, kv, b"b", b"two", 0x10), 0);
        assert_eq!(lookups(), before + 1);
        assert_eq!(cursor::mdb_cursor_get(cursor, &mut key, &mut data, 8), 0);
        assert_eq!(bytes(&key).unwrap(), b"b");
        let revision = (*w).revision;
        let quota = (*w).quota_used;
        let mut old = val(b"replacement");
        assert_eq!(
            operations::mdb_put(w, kv, &mut val(b"a"), &mut old, 0x10),
            KEYEXIST
        );
        assert_eq!(lookups(), before + 2);
        assert_eq!((*w).revision, revision);
        assert_eq!((*w).quota_used, quota);
        assert_eq!(bytes(&old).unwrap(), b"old");
        assert_eq!(put(w, kv, b"a", b"updated", 0), 0);
        assert_eq!(put(w, kv, b"large", &vec![7; 100_000], 0x10), 0);
        assert_eq!(
            bytes(&old).unwrap(),
            b"old",
            "duplicate output borrowed a replaced arena value"
        );
        assert_eq!(get(w, kv, b"a"), Ok(b"updated".to_vec()));
        cursor::mdb_cursor_close(cursor);
        assert_eq!(transaction::mdb_txn_commit(w), 0);
        let r = begin(f.env, ptr::null_mut(), true);
        let kv = db(r, c"kv".as_ptr(), 0);
        assert_eq!(get(r, kv, b"b"), Ok(b"two".to_vec()));
        transaction::mdb_txn_abort(r);
    }
}

#[test]
fn sql_nooverwrite_keeps_input_validation_reserve_aliasing_and_capacity_failures() {
    let f = Fixture::new();
    unsafe {
        let w = begin(f.env, ptr::null_mut(), false);
        let kv = db(w, c"kv".as_ptr(), 0);
        let before = lookups();
        let mut invalid = MDB_val {
            mv_size: usize::MAX,
            mv_data: ptr::null_mut(),
        };
        assert_eq!(
            operations::mdb_put(w, kv, &mut val(b"a"), &mut invalid, 0x10),
            BAD_VALSIZE
        );
        invalid.mv_size = 1;
        assert_eq!(
            operations::mdb_put(w, kv, &mut val(b"a"), &mut invalid, 0x10),
            libc::EINVAL
        );
        assert_eq!(
            operations::mdb_put(w, kv, &mut val(&[]), &mut val(b"x"), 0x10),
            BAD_VALSIZE
        );
        assert_eq!(lookups(), before);
        let mut reserved = MDB_val {
            mv_size: 8,
            mv_data: ptr::null_mut(),
        };
        assert_eq!(
            operations::mdb_put(w, kv, &mut val(b"source"), &mut reserved, 0x10000),
            0
        );
        ptr::copy_nonoverlapping(b"copy-key".as_ptr(), reserved.mv_data.cast(), 8);
        let mut aliased_key = reserved;
        let mut aliased_value = reserved;
        assert_eq!(
            operations::mdb_put(w, kv, &mut aliased_key, &mut aliased_value, 0x10),
            0
        );
        assert_eq!(lookups(), before + 1);
        assert_eq!(get(w, kv, b"copy-key"), Ok(b"copy-key".to_vec()));
        assert_eq!(get(w, kv, b"source"), Ok(b"copy-key".to_vec()));
        let large = vec![8; 2 << 20];
        let mut duplicate = val(&large);
        assert_eq!(
            operations::mdb_put(w, kv, &mut val(b"a"), &mut duplicate, 0x10),
            KEYEXIST
        );
        assert_eq!(bytes(&duplicate).unwrap(), b"old");
        assert!(!(*w).capacity_failed);
        assert_eq!(put(w, kv, b"full", &large, 0x10), -30792);
        assert!((*w).capacity_failed);
        assert_eq!(get(w, kv, b"full"), Err(BAD_TXN));
        transaction::mdb_txn_abort(w);
        let w = begin(f.env, ptr::null_mut(), false);
        let kv = db(w, c"kv".as_ptr(), 0);
        assert_eq!(get(w, kv, b"copy-key"), Err(NOTFOUND));
        assert_eq!(get(w, kv, b"full"), Err(NOTFOUND));
        assert_eq!(put(w, kv, b"after-abort", b"works", 0x10), 0);
        assert_eq!(transaction::mdb_txn_commit(w), 0);
    }
}

unsafe extern "C" fn folded(a: *const MDB_val, b: *const MDB_val) -> c_int {
    unsafe { bytes(a).unwrap() }
        .iter()
        .map(u8::to_ascii_lowercase)
        .cmp(
            unsafe { bytes(b).unwrap() }
                .iter()
                .map(u8::to_ascii_lowercase),
        ) as c_int
}

#[test]
fn sql_nooverwrite_custom_reverse_duplicates_and_main_use_original_paths() {
    let f = Fixture::new();
    unsafe {
        let w = begin(f.env, ptr::null_mut(), false);
        let main = db(w, ptr::null(), 0);
        let reverse = db(w, c"reverse".as_ptr(), CREATE | 2);
        let duplicates = db(w, c"duplicates".as_ptr(), CREATE | DUPSORT);
        let custom = db(w, c"folded".as_ptr(), CREATE);
        assert_eq!(
            operations::mdb_set_compare(w, custom, folded as *const () as *mut c_void),
            0
        );
        for dbi in [main, reverse, duplicates, custom] {
            assert_eq!(put(w, dbi, b"a", b"first", 0), 0);
        }
        let before = lookups();
        for dbi in [main, reverse, duplicates] {
            let mut old = val(b"new");
            assert_eq!(
                operations::mdb_put(w, dbi, &mut val(b"a"), &mut old, 0x10),
                KEYEXIST
            );
            assert_eq!(bytes(&old).unwrap(), b"first");
        }
        let mut old = val(b"new");
        assert_eq!(
            operations::mdb_put(w, custom, &mut val(b"A"), &mut old, 0x10),
            KEYEXIST
        );
        assert_eq!(bytes(&old).unwrap(), b"first");
        assert_eq!(lookups(), before);
        transaction::mdb_txn_abort(w);
    }
}

#[test]
fn sql_nooverwrite_child_rollback_wrong_thread_readonly_and_slot_reuse() {
    let f = Fixture::new();
    unsafe {
        let w = begin(f.env, ptr::null_mut(), false);
        let kv = db(w, c"kv".as_ptr(), 0);
        let child = begin(f.env, w, false);
        let child_kv = db(child, c"kv".as_ptr(), 0);
        let before = lookups();
        assert_eq!(put(child, child_kv, b"child", b"discard", 0x10), 0);
        assert_eq!(lookups(), before);
        transaction::mdb_txn_abort(child);
        assert_eq!(get(w, kv, b"child"), Err(NOTFOUND));
        let raw = w as usize;
        assert_eq!(
            std::thread::spawn(move || put(raw as *mut MDB_txn, kv, b"thread", b"bad", 0x10))
                .join()
                .unwrap(),
            BAD_TXN
        );
        operations::mdb_dbi_close(f.env, kv);
        assert_eq!(put(w, kv, b"closed", b"bad", 0x10), BAD_DBI);
        let replacement = db(w, c"replacement".as_ptr(), CREATE);
        assert_eq!(replacement, kv);
        assert_eq!(put(w, replacement, b"a", b"replacement", 0x10), 0);
        assert_eq!(transaction::mdb_txn_commit(w), 0);
        let r = begin(f.env, ptr::null_mut(), true);
        let old = db(r, c"kv".as_ptr(), 0);
        let replacement = db(r, c"replacement".as_ptr(), 0);
        assert_eq!(get(r, old, b"a"), Ok(b"old".to_vec()));
        assert_eq!(get(r, replacement, b"a"), Ok(b"replacement".to_vec()));
        let before = lookups();
        let expected = (*r).shared.readonly_error();
        assert_eq!(put(r, replacement, b"readonly", b"bad", 0x10), expected);
        assert_eq!(lookups(), before);
        transaction::mdb_txn_abort(r);
    }
}
