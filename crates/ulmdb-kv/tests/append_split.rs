//! Observable density and snapshot correctness; no private split-policy calls.
use std::{
    collections::BTreeMap,
    fs,
    ops::Bound,
    path::{Path, PathBuf},
};
use ulmdb_core::storage::{ReadStore, Reference};
use ulmdb_kv::{Cursor, PAGE_SIZE, audit_tree, delete, get, put};
use ulmdb_storage::{Arena, SegmentTable, create_layout, new_identity};

type Oracle = BTreeMap<Vec<u8>, Vec<u8>>;
struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        let path = parent.join(format!("ulmdb-kv-append-{:032x}", new_identity().unwrap()));
        create_layout(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}
fn key(n: u64) -> Vec<u8> {
    (2 * n + 2).to_be_bytes().to_vec()
}
fn assert_contents<S: ReadStore>(store: &S, root: Reference, oracle: &Oracle) {
    let audit = audit_tree(store, root, 100_000).unwrap();
    assert_eq!(audit.leaf_records, oracle.len() as u64);
    for (key, value) in oracle {
        assert_eq!(get(store, root, key).unwrap(), Some(value.as_slice()));
    }
    assert_eq!(get(store, root, &[0xff; 4096]).unwrap(), None);
    for reverse in [false, true] {
        let actual: Vec<_> = Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, reverse)
            .unwrap()
            .map(|pair| {
                let (k, v) = pair.unwrap();
                (k.to_vec(), v.to_vec())
            })
            .collect();
        let mut expected: Vec<_> = oracle.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        if reverse {
            expected.reverse();
        }
        assert_eq!(actual, expected);
    }
    if oracle.len() > 4 {
        let lower = oracle.keys().nth(oracle.len() / 4).unwrap();
        let upper = oracle.keys().nth(oracle.len() * 3 / 4).unwrap();
        for reverse in [false, true] {
            let actual: Vec<_> = Cursor::new(
                store,
                root,
                Bound::Excluded(lower.as_slice()),
                Bound::Included(upper.as_slice()),
                reverse,
            )
            .unwrap()
            .map(|pair| {
                let (k, v) = pair.unwrap();
                (k.to_vec(), v.to_vec())
            })
            .collect();
            let mut expected: Vec<_> = oracle
                .iter()
                .filter(|(k, _)| *k > lower && *k <= upper)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if reverse {
                expected.reverse();
            }
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn sorted_eight_byte_records_are_dense_and_updates_preserve_sealed_readers() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        assert!(parent.is_dir());
        let directory = Directory::new(&parent);
        let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
        let mut root = Reference::NULL;
        let mut oracle = Oracle::new();
        for n in 0..8192u64 {
            let (key, value) = (key(n), (17 * n + 1).to_be_bytes().to_vec());
            root = put(&mut arena, root, &key, &value).unwrap();
            oracle.insert(key, value);
        }
        let audit = audit_tree(&arena, root, 100_000).unwrap();
        assert_eq!(audit.leaf_records, 8192);
        // Format capacity is156 of these26-byte records per leaf. Permit some
        // slack, but reject the former roughly half-full105-leaf result.
        assert!(
            audit.leaf_pages <= 56,
            "append occupancy regressed: {audit:?}"
        );
        assert!(audit.height <= 3);
        let (base, _) = arena.seal(&directory.0).unwrap();
        assert_contents(&base, root, &oracle);
        let held = base.clone();
        let held_value = get(&held, root, &key(4096)).unwrap().unwrap();
        let mut abandoned = Arena::new(base.clone(), 2).unwrap();
        let abandoned_root = put(&mut abandoned, root, &key(4096), b"aborted").unwrap();
        assert_eq!(
            get(&abandoned, abandoned_root, &key(4096)).unwrap(),
            Some(&b"aborted"[..])
        );
        drop(abandoned);
        assert_eq!(held_value, (17u64 * 4096 + 1).to_be_bytes());

        let mut writer = Arena::new(base.clone(), 2).unwrap();
        let mut changed_root = root;
        let mut changed = oracle.clone();
        let mut seed = 0x92d6_8ca2_6f17_3b05;
        for _ in 0..64 {
            let n = next(&mut seed) % 8192;
            let (key, value) = (key(n), (n ^ 0xfeed_beef_dead_cafe).to_be_bytes().to_vec());
            changed_root = put(&mut writer, changed_root, &key, &value).unwrap();
            changed.insert(key, value);
        }
        // The bound covers the measured64-update trace's changed leaves and
        // ancestors; it counts real persisted CoW bytes, not a mocked allocator.
        assert!(
            writer.dirty_bytes() <= 44 * PAGE_SIZE,
            "dense-tree random replacement dirtied {} bytes",
            writer.dirty_bytes()
        );
        assert_contents(&writer, changed_root, &changed);
        assert_contents(&held, root, &oracle);
        let (updated, _) = writer.seal(&directory.0).unwrap();
        let entries = updated.entries();
        drop(updated);
        drop(base);
        let reopened = SegmentTable::load(&directory.0, &entries).unwrap();
        assert_contents(&reopened, changed_root, &changed);
        assert_contents(&held, root, &oracle);
        assert_eq!(held_value, (17u64 * 4096 + 1).to_be_bytes());
    }
}

fn varied_key(n: u64) -> Vec<u8> {
    let mut key = n.to_be_bytes().to_vec();
    key.extend(std::iter::repeat_n(
        (n % 251) as u8,
        (n as usize * 37) % 401,
    ));
    key
}
fn varied_value(n: u64) -> Vec<u8> {
    let length = [0, 1, 7, 511, 512, 513, 8192, 65_536][n as usize % 8];
    vec![(n % 251) as u8; length]
}

#[test]
fn descending_shuffled_and_variable_records_keep_order_after_interior_edits_and_deletion() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        for mode in ["descending", "shuffled", "variable"] {
            let directory = Directory::new(&parent);
            let mut order: Vec<u64> = (0..384).collect();
            if mode == "descending" {
                order.reverse();
            } else {
                let mut seed = 0xb683_c1d9_52a4_70ef;
                for i in (1..order.len()).rev() {
                    let at = (next(&mut seed) % (i + 1) as u64) as usize;
                    order.swap(i, at);
                }
            }
            let mut oracle = Oracle::new();
            let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
            let mut root = put(&mut arena, Reference::NULL, b"", b"").unwrap();
            oracle.insert(Vec::new(), Vec::new());
            for n in order {
                let (key, value) = if mode == "variable" {
                    (varied_key(n), varied_value(n))
                } else {
                    (key(n), n.to_be_bytes().to_vec())
                };
                root = put(&mut arena, root, &key, &value).unwrap();
                oracle.insert(key, value);
            }
            // A maximum facade key and a 64 KiB value exercise overflow keys,
            // separators and values through real arena payload allocation.
            let big_key = vec![0xfe; 4096];
            root = put(&mut arena, root, &big_key, &vec![0xa5; 65_536]).unwrap();
            oracle.insert(big_key.clone(), vec![0xa5; 65_536]);
            assert_contents(&arena, root, &oracle);
            let audit = audit_tree(&arena, root, 100_000).unwrap();
            assert!(audit.overflow_keys > 0 && audit.overflow_values > 0);
            let (base, _) = arena.seal(&directory.0).unwrap();
            let mut writer = Arena::new(base.clone(), 2).unwrap();
            let mut changed_root = root;
            let mut changed = oracle.clone();
            for (i, key) in oracle.keys().enumerate() {
                if i % 3 == 0 {
                    changed_root = delete(&mut writer, changed_root, key).unwrap();
                    changed.remove(key);
                } else if i % 5 == 0 {
                    let replacement = vec![0x3c; (i * 193) % 2049];
                    changed_root = put(&mut writer, changed_root, key, &replacement).unwrap();
                    changed.insert(key.clone(), replacement);
                }
            }
            for n in 0..128u64 {
                let mut interior = key(n);
                interior.push(0);
                changed_root = put(&mut writer, changed_root, &interior, &varied_value(n)).unwrap();
                changed.insert(interior, varied_value(n));
            }
            changed_root = put(&mut writer, changed_root, &big_key, b"").unwrap();
            changed.insert(big_key, Vec::new());
            assert_contents(&writer, changed_root, &changed);
            assert_contents(&base, root, &oracle);
            let (updated, _) = writer.seal(&directory.0).unwrap();
            let reopened = SegmentTable::load(&directory.0, &updated.entries()).unwrap();
            assert_contents(&reopened, changed_root, &changed);
            let mut empty = Arena::new(reopened, 3).unwrap();
            let mut empty_root = changed_root;
            for key in changed.keys() {
                empty_root = delete(&mut empty, empty_root, key).unwrap();
            }
            assert_contents(&empty, empty_root, &Oracle::new());
            assert_contents(&updated, changed_root, &changed);
        }
    }
}
