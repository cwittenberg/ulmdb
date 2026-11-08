use super::*;
use std::ffi::CString;

struct Fixture {
    env: *mut MDB_env,
    path: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-c-read-cursor-{}-{}",
            std::process::id(),
            ulmdb_storage::new_identity().unwrap()
        ));
        let name = CString::new(path.to_str().unwrap()).unwrap();
        let mut env = ptr::null_mut();
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut env), 0);
            assert_eq!(environment::mdb_env_set_maxdbs(env, 8), 0);
            assert_eq!(
                environment::mdb_env_open(env, name.as_ptr(), 0x200000, 0o600),
                0
            );
        }
        Self { env, path }
    }
    fn begin(&self, flags: u32) -> *mut MDB_txn {
        let mut txn = ptr::null_mut();
        assert_eq!(
            unsafe { transaction::mdb_txn_begin(self.env, ptr::null_mut(), flags, &mut txn) },
            0
        );
        txn
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { environment::mdb_env_close(self.env) };
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
fn val(bytes: &[u8]) -> MDB_val {
    MDB_val {
        mv_size: bytes.len(),
        mv_data: bytes.as_ptr().cast_mut().cast(),
    }
}
fn contents(value: MDB_val) -> Vec<u8> {
    unsafe { bytes(&value).unwrap() }.to_vec()
}
fn open(txn: *mut MDB_txn, name: *const c_char, flags: u32) -> u32 {
    let mut dbi = u32::MAX;
    assert_eq!(
        unsafe { operations::mdb_dbi_open(txn, name, flags, &mut dbi) },
        0
    );
    dbi
}
fn put(txn: *mut MDB_txn, dbi: u32, key: &[u8], value: &[u8]) {
    assert_eq!(
        unsafe { operations::mdb_put(txn, dbi, &mut val(key), &mut val(value), 0) },
        0
    );
}
fn cursor(txn: *mut MDB_txn, dbi: u32) -> *mut MDB_cursor {
    let mut cursor = ptr::null_mut();
    assert_eq!(unsafe { mdb_cursor_open(txn, dbi, &mut cursor) }, 0);
    cursor
}
fn get(cursor: *mut MDB_cursor, op: u32, input: &[u8]) -> Result<(MDB_val, MDB_val), c_int> {
    let mut key = val(input);
    let mut value = val(&[]);
    match unsafe { mdb_cursor_get(cursor, &mut key, &mut value, op) } {
        0 => Ok((key, value)),
        error => Err(error),
    }
}
fn returned_key(cursor: *mut MDB_cursor, op: u32, input: &[u8]) -> Vec<u8> {
    contents(get(cursor, op, input).unwrap().0)
}

#[test]
fn read_continuation_scans_named_pages_seeks_and_keeps_borrowed_values() {
    let fixture = Fixture::new();
    let writer = fixture.begin(0);
    let named = open(writer, c"rows".as_ptr(), CREATE);
    for n in 0..600u64 {
        put(writer, named, &(n * 2).to_be_bytes(), &[n as u8; 64]);
    }
    assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
    let reader = fixture.begin(RDONLY);
    let cursor = cursor(reader, named);
    let borrowed = get(cursor, 0, &[]).unwrap().1;
    for n in 1..600u64 {
        let (key, value) = get(cursor, 8, &[]).unwrap();
        assert_eq!(contents(key), (n * 2).to_be_bytes());
        assert_eq!(contents(value), [n as u8; 64]);
        assert!(unsafe { &*cursor }.continuation.is_some());
    }
    assert_eq!(get(cursor, 8, &[]).err(), Some(NOTFOUND));
    assert_eq!(returned_key(cursor, 12, &[]), 1196u64.to_be_bytes());
    assert_eq!(contents(borrowed), [0; 64]);
    let search = 200u64.to_be_bytes();
    let (key, _) = get(cursor, 15, &search).unwrap();
    assert_eq!(key.mv_data, search.as_ptr().cast_mut().cast());
    assert_eq!(returned_key(cursor, 8, &[]), 202u64.to_be_bytes());
    assert_eq!(returned_key(cursor, 12, &[]), 200u64.to_be_bytes());
    assert_eq!(get(cursor, 15, &201u64.to_be_bytes()).err(), Some(NOTFOUND));
    assert_eq!(returned_key(cursor, 8, &[]), 202u64.to_be_bytes());
    assert_eq!(
        returned_key(cursor, 17, &201u64.to_be_bytes()),
        202u64.to_be_bytes()
    );
    assert_eq!(returned_key(cursor, 4, &[]), 202u64.to_be_bytes());
    assert_eq!(returned_key(cursor, 6, &[]), 1198u64.to_be_bytes());
    for n in (0..599u64).rev() {
        assert_eq!(returned_key(cursor, 12, &[]), (n * 2).to_be_bytes());
    }
    assert_eq!(get(cursor, 12, &[]).err(), Some(NOTFOUND));
    unsafe {
        mdb_cursor_close(cursor);
        transaction::mdb_txn_abort(reader);
    }
}

#[test]
fn read_continuation_merges_main_catalog_and_handles_direction_changes() {
    let fixture = Fixture::new();
    let writer = fixture.begin(0);
    let main = open(writer, ptr::null(), 0);
    for name in [c"b", c"d", c"q"] {
        open(writer, name.as_ptr(), CREATE);
    }
    for key in [b"a", b"c", b"e", b"z"] {
        put(writer, main, key, key);
    }
    assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
    let reader = fixture.begin(RDONLY);
    let cursor = cursor(reader, main);
    assert_eq!(returned_key(cursor, 0, &[]), b"a");
    assert_eq!(returned_key(cursor, 8, &[]), b"b");
    assert_eq!(returned_key(cursor, 8, &[]), b"c");
    assert_eq!(returned_key(cursor, 12, &[]), b"b");
    assert_eq!(returned_key(cursor, 12, &[]), b"a");
    for key in [b"b", b"c", b"d", b"e", b"q", b"z"] {
        assert_eq!(returned_key(cursor, 8, &[]), key);
    }
    assert_eq!(get(cursor, 8, &[]).err(), Some(NOTFOUND));
    assert_eq!(returned_key(cursor, 12, &[]), b"q");
    assert_eq!(returned_key(cursor, 17, b"bb"), b"c");
    let input = [b'b'];
    let (key, value) = get(cursor, 15, &input).unwrap();
    assert_eq!(key.mv_data, input.as_ptr().cast_mut().cast());
    assert_eq!(value.mv_size, 8 + 5 * std::mem::size_of::<usize>());
    assert_eq!(returned_key(cursor, 4, &[]), b"b");
    assert_eq!(returned_key(cursor, 12, &[]), b"a");
    assert!(unsafe { &*cursor }.continuation.is_some());
    unsafe {
        mdb_cursor_close(cursor);
        transaction::mdb_txn_abort(reader);
    }
}

#[test]
fn read_continuation_reset_renew_rebinds_a_new_snapshot() {
    let fixture = Fixture::new();
    let writer = fixture.begin(0);
    let main = open(writer, ptr::null(), 0);
    for key in [b"a", b"b"] {
        put(writer, main, key, key);
    }
    assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
    let reader = fixture.begin(RDONLY);
    let cursor = cursor(reader, main);
    assert_eq!(returned_key(cursor, 0, &[]), b"a");
    unsafe {
        transaction::mdb_txn_reset(reader);
    }
    assert_eq!(get(cursor, 8, &[]).err(), Some(BAD_TXN));
    let writer = fixture.begin(0);
    put(writer, main, b"c", b"new");
    assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
    assert_eq!(unsafe { transaction::mdb_txn_renew(reader) }, 0);
    assert_eq!(unsafe { mdb_cursor_renew(reader, cursor) }, 0);
    assert!(unsafe { &*cursor }.continuation.is_none());
    assert_eq!(returned_key(cursor, 8, &[]), b"a");
    assert_eq!(returned_key(cursor, 8, &[]), b"b");
    assert_eq!(returned_key(cursor, 8, &[]), b"c");
    unsafe {
        transaction::mdb_txn_abort(reader);
        mdb_cursor_close(cursor);
    }
}

unsafe extern "C" fn descending(a: *const MDB_val, b: *const MDB_val) -> c_int {
    unsafe { bytes(b).unwrap().cmp(bytes(a).unwrap()) as c_int }
}

#[test]
fn read_continuation_observes_comparator_changes_and_closed_dbis() {
    let fixture = Fixture::new();
    let writer = fixture.begin(0);
    let named = open(writer, c"rows".as_ptr(), CREATE);
    for key in [b"a", b"b", b"c"] {
        put(writer, named, key, key);
    }
    let write_cursor = cursor(writer, named);
    assert_eq!(returned_key(write_cursor, 0, &[]), b"a");
    assert!(unsafe { &*write_cursor }.continuation.is_none());
    assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
    let reader = fixture.begin(RDONLY);
    let cursor = cursor(reader, named);
    assert_eq!(returned_key(cursor, 0, &[]), b"a");
    assert_eq!(
        unsafe { operations::mdb_set_compare(reader, named, descending as *mut c_void) },
        0
    );
    assert_eq!(returned_key(cursor, 0, &[]), b"c");
    assert!(unsafe { &*cursor }.continuation.is_none());
    assert_eq!(returned_key(cursor, 8, &[]), b"b");
    assert_eq!(
        unsafe { operations::mdb_set_compare(reader, named, ptr::null_mut()) },
        0
    );
    assert_eq!(returned_key(cursor, 0, &[]), b"a");
    assert!(unsafe { &*cursor }.continuation.is_some());
    unsafe {
        operations::mdb_dbi_close(fixture.env, named);
    }
    assert_eq!(get(cursor, 8, &[]).err(), Some(BAD_DBI));
    unsafe {
        mdb_cursor_close(cursor);
        transaction::mdb_txn_abort(reader);
    }
}

#[test]
fn read_continuation_handles_main_with_catalog_only_and_failed_seek() {
    let fixture = Fixture::new();
    let writer = fixture.begin(0);
    let main = open(writer, ptr::null(), 0);
    open(writer, c"named".as_ptr(), CREATE);
    assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
    let reader = fixture.begin(RDONLY);
    let cursor = cursor(reader, main);
    assert_eq!(returned_key(cursor, 8, &[]), b"named");
    assert_eq!(get(cursor, 8, &[]).err(), Some(NOTFOUND));
    assert_eq!(get(cursor, 15, b"absent").err(), Some(NOTFOUND));
    assert_eq!(returned_key(cursor, 4, &[]), b"named");
    assert_eq!(returned_key(cursor, 6, &[]), b"named");
    assert_eq!(get(cursor, 12, &[]).err(), Some(NOTFOUND));
    unsafe {
        mdb_cursor_close(cursor);
        transaction::mdb_txn_abort(reader);
    }
}

#[test]
fn read_continuation_mixed_main_operations_match_ordered_row_model() {
    let fixture = Fixture::new();
    let writer = fixture.begin(0);
    let main = open(writer, ptr::null(), 0);
    let mut rows = Vec::new();
    for n in (0..200).step_by(2) {
        let key = format!("k{n:03}").into_bytes();
        put(writer, main, &key, b"ordinary");
        rows.push(key);
    }
    for n in [1, 11, 21] {
        let name = CString::new(format!("k{n:03}")).unwrap();
        open(writer, name.as_ptr(), CREATE);
        rows.push(name.as_bytes().to_vec());
    }
    rows.sort();
    assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
    let reader = fixture.begin(RDONLY);
    let cursor = cursor(reader, main);
    let mut position = None::<usize>;
    let mut random = 0x9812abefu64;
    for _ in 0..2000 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        let input = format!("k{:03}", (random >> 8) % 240).into_bytes();
        let op = [0, 6, 8, 12, 15, 16, 17, 4][(random as usize) % 8];
        let expected = match op {
            0 => Some(0),
            6 => rows.len().checked_sub(1),
            8 => match position {
                None => Some(0),
                Some(at) => (at + 1 < rows.len()).then_some(at + 1),
            },
            12 => match position {
                None => rows.len().checked_sub(1),
                Some(at) => at.checked_sub(1),
            },
            15 | 16 => rows.binary_search(&input).ok(),
            17 => {
                let at = rows.partition_point(|row| row < &input);
                (at < rows.len()).then_some(at)
            }
            4 => position,
            _ => unreachable!(),
        };
        match (get(cursor, op, &input), expected) {
            (Ok((key, _)), Some(at)) => {
                assert_eq!(contents(key), rows[at], "operation {op}");
                assert!(unsafe { &*cursor }.continuation.is_some());
                position = Some(at);
            }
            (Err(error), None) => assert_eq!(error, if op == 4 { libc::EINVAL } else { NOTFOUND }),
            (actual, expected) => panic!(
                "operation {op}: success={}, expected={expected:?}",
                actual.is_ok()
            ),
        }
    }
    unsafe {
        mdb_cursor_close(cursor);
        transaction::mdb_txn_abort(reader);
    }
}

#[test]
fn scoped_continuation_keeps_native_candidate_through_consecutive_main_catalog_rows() {
    let fixture = Fixture::new();
    let writer = fixture.begin(0);
    let main = open(writer, ptr::null(), 0);
    for name in [c"b", c"c", c"d", c"e"] {
        open(writer, name.as_ptr(), CREATE);
    }
    put(writer, main, b"a", b"native-a");
    put(writer, main, b"z", b"native-z");
    assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
    let reader = fixture.begin(RDONLY);
    let cursor = cursor(reader, main);
    let native_borrow = get(cursor, 0, &[]).unwrap().1;
    assert_eq!(returned_key(cursor, 8, &[]), b"b");
    let catalog_borrow = get(cursor, 4, &[]).unwrap().1;
    let catalog_copy = contents(catalog_borrow);
    assert_eq!(returned_key(cursor, 8, &[]), b"c");
    assert_eq!(returned_key(cursor, 8, &[]), b"d");
    assert_eq!(returned_key(cursor, 12, &[]), b"c");
    assert_eq!(returned_key(cursor, 8, &[]), b"d");
    assert_eq!(returned_key(cursor, 8, &[]), b"e");
    assert_eq!(returned_key(cursor, 8, &[]), b"z");
    assert_eq!(get(cursor, 8, &[]).err(), Some(NOTFOUND));
    for key in [b"e", b"d", b"c", b"b", b"a"] {
        assert_eq!(returned_key(cursor, 12, &[]), key);
    }
    assert_eq!(get(cursor, 12, &[]).err(), Some(NOTFOUND));
    assert_eq!(returned_key(cursor, 8, &[]), b"b");
    assert_eq!(contents(native_borrow), b"native-a");
    assert_eq!(contents(catalog_borrow), catalog_copy);
    assert!(unsafe { &*cursor }.continuation.is_some());
    unsafe {
        mdb_cursor_close(cursor);
        transaction::mdb_txn_abort(reader);
    }
}
