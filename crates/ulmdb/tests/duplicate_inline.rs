use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Bound,
    path::PathBuf,
};
use ulmdb::{DatabaseKind, Environment};
use ulmdb_storage::Reference;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-inline-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn create(path: &Directory) -> Environment {
    let env = Environment::create(&path.0).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"dups", DatabaseKind::Duplicates { width: None })
        .unwrap();
    w.commit().unwrap();
    env
}
fn stored_bucket(path: &Directory, name: &[u8], key: &[u8]) -> Vec<u8> {
    let manifest = ulmdb_storage::read_current(&path.0).unwrap();
    let store = ulmdb_storage::SegmentTable::load(&path.0, &manifest.segments).unwrap();
    let descriptor = ulmdb_kv::get(&store, manifest.catalog, name)
        .unwrap()
        .unwrap();
    assert_eq!(&descriptor[12..14], &3u16.to_le_bytes());
    let root = Reference::decode(&descriptor[16..32]).unwrap();
    ulmdb_kv::get(&store, root, key).unwrap().unwrap().to_vec()
}
fn is_inline(bytes: &[u8]) -> bool {
    Reference::decode(bytes).unwrap().is_null()
}

#[test]
fn singleton_dirty_bytes_avoid_a_separate_child_page_per_pair() {
    let path = Directory::new();
    let env = create(&path);
    let mut w = env.begin_write().unwrap();
    for n in 0..128u64 {
        assert!(
            w.insert_duplicate(b"dups", &n.to_be_bytes(), &n.to_le_bytes())
                .unwrap()
        );
    }
    let dirty = w.dirty_bytes();
    eprintln!(
        "128 singleton pairs: logical_bytes=2048 dirty_bytes={dirty}; prior subtree-only fixture=536576"
    );
    assert!(dirty <= 16_384, "singletons must pack into shared KV pages");
    assert!(dirty < 536_576 / 10);
    for n in 0..128u64 {
        assert!(
            !w.insert_duplicate(b"dups", &n.to_be_bytes(), &n.to_le_bytes())
                .unwrap()
        );
    }
    assert_eq!(w.dirty_bytes(), dirty);
    w.commit().unwrap();
    let read = env.begin_read();
    let view = read.duplicates(b"dups").unwrap();
    for n in 0..128u64 {
        assert_eq!(view.count(&n.to_be_bytes()).unwrap(), 1);
        assert_eq!(
            view.first(&n.to_be_bytes()).unwrap(),
            Some(n.to_le_bytes().as_slice())
        );
        assert!(view.contains(&n.to_be_bytes(), &n.to_le_bytes()).unwrap());
    }
    assert!(is_inline(&stored_bucket(
        &path,
        b"dups",
        &10u64.to_be_bytes()
    )));
}

#[test]
fn promotion_and_demotion_by_count_and_bytes_preserve_snapshot_borrows() {
    let path = Directory::new();
    let env = create(&path);
    let mut w = env.begin_write().unwrap();
    for n in 0..8u64 {
        w.insert_duplicate(b"dups", b"count", &n.to_be_bytes())
            .unwrap();
    }
    let a = vec![1; 200];
    let b = vec![2; 200];
    let c = vec![3; 200];
    w.insert_duplicate(b"dups", b"bytes", &a).unwrap();
    w.insert_duplicate(b"dups", b"bytes", &b).unwrap();
    w.commit().unwrap();
    assert!(is_inline(&stored_bucket(&path, b"dups", b"count")));
    assert!(is_inline(&stored_bucket(&path, b"dups", b"bytes")));
    let old = env.begin_read();
    let borrowed = old.duplicate_first(b"dups", b"bytes").unwrap().unwrap();
    let mut w = env.begin_write().unwrap();
    w.insert_duplicate(b"dups", b"count", &8u64.to_be_bytes())
        .unwrap();
    w.insert_duplicate(b"dups", b"bytes", &c).unwrap();
    w.commit().unwrap();
    assert!(!is_inline(&stored_bucket(&path, b"dups", b"count")));
    assert!(!is_inline(&stored_bucket(&path, b"dups", b"bytes")));
    assert_eq!(old.duplicate_count(b"dups", b"count").unwrap(), 8);
    assert_eq!(borrowed, a);
    let promoted = env.begin_read();
    let mut w = env.begin_write().unwrap();
    w.delete_duplicate(b"dups", b"count", &8u64.to_be_bytes())
        .unwrap();
    w.delete_duplicate(b"dups", b"bytes", &c).unwrap();
    w.commit().unwrap();
    assert!(is_inline(&stored_bucket(&path, b"dups", b"count")));
    assert!(is_inline(&stored_bucket(&path, b"dups", b"bytes")));
    assert_eq!(promoted.duplicate_count(b"dups", b"count").unwrap(), 9);
    assert_eq!(promoted.duplicate_count(b"dups", b"bytes").unwrap(), 3);
    assert_eq!(borrowed, a);
    let huge = vec![4; 65_536];
    let mut w = env.begin_write().unwrap();
    w.insert_duplicate(b"dups", b"bytes", &huge).unwrap();
    w.commit().unwrap();
    assert!(!is_inline(&stored_bucket(&path, b"dups", b"bytes")));
    let mut w = env.begin_write().unwrap();
    w.delete_duplicate(b"dups", b"bytes", &huge).unwrap();
    w.commit().unwrap();
    assert!(is_inline(&stored_bucket(&path, b"dups", b"bytes")));
    drop(env);
    let reopened = Environment::open(&path.0).unwrap();
    assert_eq!(
        reopened
            .begin_read()
            .duplicate_count(b"dups", b"count")
            .unwrap(),
        8
    );
    assert_eq!(borrowed, a);
}

#[test]
fn empty_fixed_values_and_savepoints_cross_both_representation_transitions() {
    let path = Directory::new();
    let env = create(&path);
    let mut w = env.begin_write().unwrap();
    w.create_database(b"zero", DatabaseKind::Duplicates { width: Some(0) })
        .unwrap();
    w.insert_duplicate(b"zero", b"", b"").unwrap();
    for n in 0..8u64 {
        w.insert_duplicate(b"dups", b"key", &n.to_be_bytes())
            .unwrap();
    }
    let dirty = w.dirty_bytes();
    {
        let mut selected = w.savepoint().unwrap();
        selected
            .insert_duplicate(b"dups", b"key", &8u64.to_be_bytes())
            .unwrap();
        assert_eq!(selected.duplicate_count(b"dups", b"key").unwrap(), 9);
        selected.rollback().unwrap();
        assert_eq!(selected.dirty_bytes(), dirty);
        assert_eq!(selected.duplicate_count(b"dups", b"key").unwrap(), 8);
        selected
            .insert_duplicate(b"dups", b"key", &8u64.to_be_bytes())
            .unwrap();
        selected.release().unwrap();
    }
    w.commit().unwrap();
    assert_eq!(stored_bucket(&path, b"zero", b"").len(), 28);
    assert!(is_inline(&stored_bucket(&path, b"zero", b"")));
    assert!(!is_inline(&stored_bucket(&path, b"dups", b"key")));
    let mut w = env.begin_write().unwrap();
    {
        let mut child = w.savepoint().unwrap();
        child
            .delete_duplicate(b"dups", b"key", &8u64.to_be_bytes())
            .unwrap();
        assert_eq!(child.duplicate_count(b"dups", b"key").unwrap(), 8);
    }
    assert_eq!(w.duplicate_count(b"dups", b"key").unwrap(), 9);
    assert_eq!(w.commit().unwrap(), 2);
    assert_eq!(
        env.begin_read().duplicate_first(b"zero", b"").unwrap(),
        Some(b"".as_slice())
    );
}

type Pair = (Vec<u8>, Vec<u8>);
#[test]
fn cross_key_cursor_mixed_buckets_bounds_reverse_and_key_skip() {
    let path = Directory::new();
    let env = create(&path);
    let mut w = env.begin_write().unwrap();
    let mut model: BTreeMap<Vec<u8>, BTreeSet<Vec<u8>>> = BTreeMap::new();
    for (key, count) in [
        (b"".as_slice(), 1),
        (b"a", 3),
        (b"b", 18),
        (b"c", 2),
        (b"z", 1),
    ] {
        for n in 0..count {
            let value = vec![n];
            w.insert_duplicate(b"dups", key, &value).unwrap();
            model.entry(key.to_vec()).or_default().insert(value);
        }
    }
    w.commit().unwrap();
    let read = env.begin_read();
    let view = read.duplicates(b"dups").unwrap();
    let all: Vec<Pair> = model
        .iter()
        .flat_map(|(key, values)| values.iter().map(move |value| (key.clone(), value.clone())))
        .collect();
    for reverse in [false, true] {
        let actual = view
            .pairs(Bound::Unbounded, Bound::Unbounded, reverse)
            .unwrap()
            .map(|pair| {
                let (k, v) = pair.unwrap();
                (k.to_vec(), v.to_vec())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            if reverse {
                all.iter().rev().cloned().collect()
            } else {
                all.clone()
            }
        );
        let mut cursor = read
            .duplicate_pairs(b"dups", Bound::Unbounded, Bound::Unbounded, reverse)
            .unwrap();
        let mut seen = Vec::new();
        while let Some(pair) = cursor.next_key() {
            let (k, v) = pair.unwrap();
            seen.push((k.to_vec(), v.to_vec()));
        }
        let mut expected = model
            .iter()
            .map(|(key, values)| {
                (
                    key.clone(),
                    if reverse {
                        values.last().unwrap()
                    } else {
                        values.first().unwrap()
                    }
                    .clone(),
                )
            })
            .collect::<Vec<_>>();
        if reverse {
            expected.reverse();
        }
        assert_eq!(seen, expected);
        let expected = all
            .iter()
            .filter(|(key, _)| key.as_slice() > b"a" && key.as_slice() <= b"c")
            .cloned()
            .collect::<Vec<_>>();
        let actual = view
            .pairs(Bound::Excluded(b"a"), Bound::Included(b"c"), reverse)
            .unwrap()
            .map(|pair| {
                let (k, v) = pair.unwrap();
                (k.to_vec(), v.to_vec())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            if reverse {
                expected.into_iter().rev().collect()
            } else {
                expected
            }
        );
    }
    for reverse in [false, true] {
        let mut cursor = view
            .pairs(Bound::Included(b"b"), Bound::Included(b"c"), reverse)
            .unwrap();
        let first = cursor.next().unwrap().unwrap().0.to_vec();
        let next = cursor.next_key().unwrap().unwrap().0.to_vec();
        assert_eq!(
            (first, next),
            if reverse {
                (b"c".to_vec(), b"b".to_vec())
            } else {
                (b"b".to_vec(), b"c".to_vec())
            }
        );
        assert!(cursor.next_key().is_none());
    }
}

#[test]
fn opens_legacy_minor1_codec3_subtrees_and_upgrades_without_rewriting_old_buckets() {
    use sha2::{Digest, Sha256};
    let path = Directory::new();
    ulmdb_storage::create_layout(&path.0).unwrap();
    let mut arena = ulmdb_storage::Arena::new(ulmdb_storage::SegmentTable::default(), 1).unwrap();
    let child = ulmdb_kv::put(&mut arena, Reference::NULL, b"legacy-value", b"").unwrap();
    let mut bucket = [0; 24];
    bucket[..16].copy_from_slice(&child.encode());
    bucket[16..].copy_from_slice(&1u64.to_le_bytes());
    let root = ulmdb_kv::put(&mut arena, Reference::NULL, b"old-key", &bucket).unwrap();
    let mut descriptor = [0; 80];
    descriptor[..8].copy_from_slice(&1u64.to_le_bytes());
    descriptor[8..10].copy_from_slice(&2u16.to_le_bytes());
    descriptor[10..12].copy_from_slice(&1u16.to_le_bytes());
    descriptor[12..14].copy_from_slice(&3u16.to_le_bytes());
    descriptor[16..32].copy_from_slice(&root.encode());
    descriptor[32..40].copy_from_slice(&4096u64.to_le_bytes());
    descriptor[40..48].copy_from_slice(&67_108_864u64.to_le_bytes());
    descriptor[48..56].copy_from_slice(&65_536u64.to_le_bytes());
    descriptor[56..64].copy_from_slice(&u64::MAX.to_le_bytes());
    let catalog = ulmdb_kv::put(&mut arena, Reference::NULL, b"dups", &descriptor).unwrap();
    let (segments, entry) = arena.seal(&path.0).unwrap();
    let mut entry = entry.unwrap();
    drop(segments);
    let filename = |hash: &[u8; 32]| {
        hash.iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
            + ".seg"
    };
    let old = path.0.join("segments").join(filename(&entry.hash));
    let mut bytes = std::fs::read(&old).unwrap();
    bytes[10..12].copy_from_slice(&1u16.to_le_bytes());
    bytes[56..64].copy_from_slice(&1u64.to_le_bytes());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    std::fs::write(&old, &bytes).unwrap();
    entry.hash = Sha256::digest(&bytes).into();
    let final_path = path.0.join("segments").join(filename(&entry.hash));
    std::fs::rename(&old, &final_path).unwrap();
    let mut permissions = std::fs::metadata(&final_path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(final_path, permissions).unwrap();
    let mut manifest = ulmdb_storage::Manifest::empty().unwrap();
    manifest.format_minor = 1;
    manifest.sequence = 1;
    manifest.next_database = 2;
    manifest.next_segment = 2;
    manifest.catalog = catalog;
    manifest.segments = (vec![entry]).into();
    ulmdb_storage::install_snapshot(&path.0, &manifest).unwrap();
    let env = Environment::open(&path.0).unwrap();
    let old_read = env.begin_read();
    assert_eq!(
        old_read.duplicate_first(b"dups", b"old-key").unwrap(),
        Some(b"legacy-value".as_slice())
    );
    assert_eq!(old_read.duplicate_count(b"dups", b"old-key").unwrap(), 1);
    let mut w = env.begin_write().unwrap();
    w.insert_duplicate(b"dups", b"new-key", b"new-value")
        .unwrap();
    assert_eq!(w.commit().unwrap(), 2);
    assert_eq!(
        ulmdb_storage::read_current(&path.0).unwrap().format_minor,
        2
    );
    assert!(is_inline(&stored_bucket(&path, b"dups", b"new-key")));
    assert!(!is_inline(&stored_bucket(&path, b"dups", b"old-key")));
    assert_eq!(old_read.duplicate_count(b"dups", b"new-key").unwrap(), 0);
    drop(env);
    let reopened = Environment::open(&path.0).unwrap();
    assert_eq!(
        reopened
            .begin_read()
            .duplicate_count(b"dups", b"old-key")
            .unwrap(),
        1
    );
}
