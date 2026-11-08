use super::*;
use std::{ffi::CString, path::PathBuf};

const MAIN_OVERHEAD: usize = MAIN.len() + 48;
const MAP_FULL: c_int = -30792;

struct Fixture {
    env: *mut MDB_env,
    path: PathBuf,
}
impl Fixture {
    fn new(mapsize: usize, seed: impl FnOnce(&Environment)) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-c-quota-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        {
            let native = Environment::create(&path).unwrap();
            seed(&native);
        }
        // Select a fixture-local ABI without changing process environment.
        std::fs::write(path.join("LMDB-ABI"), b"1.0\n").unwrap();
        let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        let mut env = ptr::null_mut();
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut env), 0);
            assert_eq!(environment::mdb_env_set_maxdbs(env, 8), 0);
            assert_eq!(environment::mdb_env_set_mapsize(env, mapsize), 0);
            assert_eq!(
                environment::mdb_env_open(env, name.as_ptr(), 0x200000, 0o600),
                0
            );
        }
        Self { env, path }
    }
    fn begin(&self, parent: Option<&Txn>, flags: u32) -> Txn {
        let mut raw = ptr::null_mut();
        assert_eq!(
            unsafe {
                transaction::mdb_txn_begin(
                    self.env,
                    parent.map_or(ptr::null_mut(), |txn| txn.raw),
                    flags,
                    &mut raw,
                )
            },
            0
        );
        Txn { raw }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { environment::mdb_env_close(self.env) };
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
struct Txn {
    raw: *mut MDB_txn,
}
impl Txn {
    fn main(&self) {
        let mut dbi = u32::MAX;
        assert_eq!(
            unsafe { operations::mdb_dbi_open(self.raw, ptr::null(), 0, &mut dbi) },
            0
        );
        assert_eq!(dbi, 1);
    }
    fn quota(&self) -> Option<usize> {
        unsafe { (*self.raw).quota_used }
    }
    fn measured(&self) -> usize {
        unsafe { (*self.raw).logical_size().unwrap() }
    }
    fn put(&self, key: &[u8], value: &[u8]) -> c_int {
        unsafe { operations::mdb_put(self.raw, 1, &mut val(key), &mut val(value), 0) }
    }
    fn delete(&self, key: &[u8]) -> c_int {
        unsafe { operations::mdb_del(self.raw, 1, &mut val(key), ptr::null_mut()) }
    }
    fn get(&self, key: &[u8]) -> std::result::Result<Vec<u8>, c_int> {
        let mut value = val(&[]);
        match unsafe { operations::mdb_get(self.raw, 1, &mut val(key), &mut value) } {
            0 => Ok(unsafe { bytes(&value).unwrap() }.to_vec()),
            error => Err(error),
        }
    }
    fn checkpoint(&self) -> [u8; 64] {
        match unsafe { (*self.raw).state.as_ref().unwrap() } {
            State::Read(read) => read.file_checkpoint().unwrap(),
            _ => panic!("immutable checkpoint required"),
        }
    }
    fn commit(mut self) -> c_int {
        let raw = std::mem::replace(&mut self.raw, ptr::null_mut());
        unsafe { transaction::mdb_txn_commit(raw) }
    }
}
impl Drop for Txn {
    fn drop(&mut self) {
        unsafe { transaction::mdb_txn_abort(self.raw) };
    }
}
fn val(value: &[u8]) -> MDB_val {
    MDB_val {
        mv_size: value.len(),
        mv_data: value.as_ptr().cast_mut().cast(),
    }
}

#[test]
fn native_peer_new_checkpoint_replaces_seed_without_losing_old_read() {
    let fixture = Fixture::new(1 << 20, |_| {});
    let writer = fixture.begin(None, 0);
    writer.main();
    assert_eq!(writer.put(b"a", &[1; 64]), 0);
    let old_used = writer.measured();
    assert_eq!(writer.commit(), 0);
    let old = fixture.begin(None, RDONLY);
    old.main();
    let old_checkpoint = old.checkpoint();
    let capacity_hint = map_size::read(&fixture.path).unwrap();
    {
        let native = Environment::open(&fixture.path).unwrap();
        let mut peer = native.begin_write().unwrap();
        peer.put(MAIN, b"a", &[2; 4096]).unwrap();
        peer.commit().unwrap();
    }
    assert_eq!(map_size::read(&fixture.path).unwrap(), capacity_hint);
    let writer = fixture.begin(None, 0);
    assert_eq!(writer.quota(), Some(old_used - 64 + 4096));
    assert_eq!(writer.quota(), Some(writer.measured()));
    writer.main();
    assert_eq!(writer.get(b"a").unwrap(), [2; 4096]);
    assert_eq!(old.get(b"a").unwrap(), [1; 64]);
    let current = writer.raw;
    assert_ne!(
        unsafe {
            (*current)
                .writer_environment
                .as_ref()
                .unwrap()
                .selected_checkpoint()
                .unwrap()
        },
        old_checkpoint
    );
    drop(writer);
    assert_eq!(old.checkpoint(), old_checkpoint);
}

#[test]
fn virtual_main_is_charged_once_at_exact_empty_map_boundary() {
    const CAPACITY: usize = 3 * 4096;
    let fixture = Fixture::new(CAPACITY, |_| {});
    let writer = fixture.begin(None, 0);
    let initial = 8192 + MAIN_OVERHEAD;
    assert_eq!(writer.quota(), Some(initial));
    assert_eq!(writer.measured(), initial);
    writer.main();
    assert_eq!(writer.quota(), Some(initial));
    let value = vec![7; CAPACITY - initial - 1 - 16];
    assert_eq!(writer.put(b"k", &value), 0);
    assert_eq!(writer.quota(), Some(CAPACITY));
    assert_eq!(writer.commit(), 0);
    let writer = fixture.begin(None, 0);
    assert_eq!(writer.quota(), Some(CAPACITY));
    writer.main();
    assert_eq!(
        writer.put(b"k", &value),
        0,
        "same-size replacement fits exact quota"
    );
    assert_eq!(writer.put(b"x", b"x"), MAP_FULL);
}

#[test]
fn projected_main_scalar_cannot_double_charge_named_only_native_base() {
    const CAPACITY: usize = 3 * 4096;
    let fixture = Fixture::new(CAPACITY, |native| {
        let mut writer = native.begin_write().unwrap();
        writer.create_database(b"named", DatabaseKind::Kv).unwrap();
        writer.put(b"named", b"a", b"A").unwrap();
        writer.commit().unwrap();
    });
    // Environment open measured its projected MAIN at this same checkpoint.
    // The actual native base lacks MAIN: its seed plus creation must charge once.
    let writer = fixture.begin(None, 0);
    let actual = 8192 + b"named".len() + 48 + 1 + 1 + 16 + MAIN_OVERHEAD;
    assert_eq!(writer.measured(), actual);
    assert_eq!(writer.quota(), Some(actual));
    writer.main();
    let value = vec![3; CAPACITY - actual - 1 - 16];
    assert_eq!(
        writer.put(b"k", &value),
        0,
        "projected catalog must not cause false MAP_FULL"
    );
    assert_eq!(writer.quota(), Some(CAPACITY));
    assert_eq!(writer.commit(), 0);
    let read = fixture.begin(None, RDONLY);
    read.main();
    assert_eq!(read.get(b"k").unwrap(), value);
}

#[test]
fn nested_abort_keeps_parent_seed_and_commit_propagates_invalidation() {
    let fixture = Fixture::new(1 << 20, |_| {});
    let parent = fixture.begin(None, 0);
    parent.main();
    assert_eq!(parent.put(b"a", &[1; 100]), 0);
    let initial = parent.quota().unwrap();
    {
        let child = fixture.begin(Some(&parent), 0);
        assert_eq!(child.quota(), Some(initial));
        child.main();
        assert_eq!(child.put(b"b", &[2; 200]), 0);
        assert_eq!(child.quota(), Some(initial + 1 + 200 + 16));
    }
    assert_eq!(parent.quota(), Some(initial));
    assert_eq!(parent.get(b"b").err(), Some(NOTFOUND));
    let child = fixture.begin(Some(&parent), 0);
    child.main();
    assert_eq!(child.put(b"b", &[2; 200]), 0);
    assert_eq!(child.commit(), 0);
    let with_child = initial + 1 + 200 + 16;
    assert_eq!(parent.quota(), Some(with_child));
    {
        let child = fixture.begin(Some(&parent), 0);
        let grandchild = fixture.begin(Some(&child), 0);
        grandchild.main();
        assert_eq!(grandchild.delete(b"a"), 0);
        assert_eq!(grandchild.quota(), None);
        drop(grandchild);
        assert_eq!(child.quota(), Some(with_child));
    }
    assert_eq!(parent.quota(), Some(with_child));
    let child = fixture.begin(Some(&parent), 0);
    child.main();
    assert_eq!(child.delete(b"a"), 0);
    assert_eq!(child.quota(), None);
    assert_eq!(child.commit(), 0);
    assert_eq!(parent.quota(), None);
    assert_eq!(parent.put(b"c", &[3; 50]), 0);
    assert_eq!(parent.quota(), Some(parent.measured()));
    let final_used = parent.quota();
    assert_eq!(parent.commit(), 0);
    let writer = fixture.begin(None, 0);
    assert_eq!(writer.quota(), final_used);
    writer.main();
    assert_eq!(writer.get(b"a").err(), Some(NOTFOUND));
    assert_eq!(writer.get(b"b").unwrap(), [2; 200]);
    assert_eq!(writer.get(b"c").unwrap(), [3; 50]);
}

#[test]
fn virtual_main_overhead_keeps_clean_begin_and_invalid_key_error_order() {
    let fixture = Fixture::new(8192, |_| {});
    let checkpoint = ulmdb_storage::read_current(&fixture.path)
        .unwrap()
        .generation;
    let clean = fixture.begin(None, 0);
    assert_eq!(clean.quota(), Some(8192 + MAIN_OVERHEAD));
    assert_eq!(clean.commit(), 0);
    assert_eq!(
        ulmdb_storage::read_current(&fixture.path)
            .unwrap()
            .generation,
        checkpoint
    );
    let writer = fixture.begin(None, 0);
    writer.main();
    assert_eq!(writer.put(&[], b"x"), BAD_VALSIZE);
    assert!(!unsafe { (*writer.raw).capacity_failed });
    assert_eq!(writer.put(b"k", b"x"), MAP_FULL);
    assert_eq!(writer.commit(), BAD_TXN);
}

#[test]
fn unmeasurable_reverse_mapping_keeps_lazy_none_seed() {
    let fixture = Fixture::new(1 << 20, |native| {
        let mut writer = native.begin_write().unwrap();
        writer
            .create_database(b"\0lmdb-name:missing-reverse", DatabaseKind::Kv)
            .unwrap();
        writer.commit().unwrap();
    });
    let writer = fixture.begin(None, 0);
    assert_eq!(
        writer.quota(),
        None,
        "unmeasurable metadata is no scalar certificate"
    );
}

#[test]
fn commit_reconciles_actual_native_roots_even_with_a_small_cached_quota() {
    let fixture = Fixture::new(3 * 4096, |_| {});
    let before = std::fs::read(fixture.path.join("CURRENT")).unwrap();
    let writer = fixture.begin(None, 0);
    writer.main();
    assert_eq!(writer.put(b"a", b"A"), 0);
    let cached = writer.quota();
    // A native root mutation models work without a certified C quota delta.
    // Final reconciliation must inspect the winning roots, not trust the seed.
    unsafe {
        (*writer.raw)
            .writer()
            .unwrap()
            .put(MAIN, b"b", &[9; 8192])
            .unwrap();
    }
    assert_eq!(writer.quota(), cached);
    assert!(writer.measured() > unsafe { (*fixture.env).mapsize });
    assert_eq!(writer.commit(), MAP_FULL);
    assert_eq!(std::fs::read(fixture.path.join("CURRENT")).unwrap(), before);
    let read = fixture.begin(None, RDONLY);
    read.main();
    assert_eq!(read.get(b"a").err(), Some(NOTFOUND));
    assert_eq!(read.get(b"b").err(), Some(NOTFOUND));
}
