//! Real file-Arena rollback/reopen and immutable-reader coverage for pruning.
use std::{collections::BTreeMap, ops::Bound, path::PathBuf};
use ulmdb_core::storage::{ReadStore, Reference};
use ulmdb_kv::{Cursor, audit_tree, build_sorted, delete, get, height, put};
use ulmdb_storage::{Arena, SegmentTable, create_layout, new_identity};

struct Directory(PathBuf);
impl Directory {
    fn new(parent: PathBuf) -> Self {
        let path = parent.join(format!("ulmdb-kv-prune-{:032x}", new_identity().unwrap()));
        create_layout(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

type Model = BTreeMap<Vec<u8>, Vec<u8>>;
fn key(n: u64) -> Vec<u8> {
    n.to_be_bytes().to_vec()
}
fn check<S: ReadStore>(store: &S, root: Reference, model: &Model) {
    assert_eq!(
        audit_tree(store, root, 10_000).unwrap().leaf_records,
        model.len() as u64
    );
    for reverse in [false, true] {
        let actual: Vec<_> = Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, reverse)
            .unwrap()
            .map(|pair| {
                let (key, value) = pair.unwrap();
                (key.to_vec(), value.to_vec())
            })
            .collect();
        let mut expected: Vec<_> = model.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        if reverse {
            expected.reverse();
        }
        assert_eq!(actual, expected);
    }
    for (key, value) in model {
        assert_eq!(get(store, root, key).unwrap(), Some(value.as_slice()));
    }
}

#[test]
fn deep_pruning_rolls_back_private_ancestors_and_preserves_mapped_borrows_after_reopen() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        let directory = Directory::new(parent);
        let original: Model = (0..1536u64).map(|n| (key(n), vec![n as u8; 512])).collect();
        let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
        let root = build_sorted(
            &mut arena,
            original
                .iter()
                .map(|(k, v)| Ok((k.as_slice(), v.as_slice()))),
        )
        .unwrap()
        .root;
        assert!(height(&arena, root).unwrap() >= 3);
        let (base, _) = arena.seal(&directory.0).unwrap();
        let borrowed = get(&base, root, &key(768)).unwrap().unwrap();
        let held = base.clone();
        let mut writer = Arena::new(base.clone(), 2).unwrap();
        let mut changed = root;
        let mut model = original.clone();
        for n in 0..256 {
            changed = delete(&mut writer, changed, &key(n)).unwrap();
            model.remove(&key(n));
        }
        check(&writer, changed, &model);
        // This checkpoint includes private, already-rewritten branch pages.
        // Rollback must restore their prior bytes, not just truncate new pages.
        let saved_root = changed;
        let checkpoint = writer.checkpoint().unwrap();
        let accounting = (writer.dirty_bytes(), writer.revision());
        for n in 256..1536 {
            changed = delete(&mut writer, changed, &key(n)).unwrap();
        }
        assert!(changed.is_null());
        writer.rollback_checkpoint(&checkpoint).unwrap();
        assert_eq!((writer.dirty_bytes(), writer.revision()), accounting);
        changed = saved_root;
        check(&writer, changed, &model);
        check(&held, root, &original);
        assert_eq!(borrowed, &[0; 512]);

        for n in (256..1535).rev() {
            changed = delete(&mut writer, changed, &key(n)).unwrap();
            model.remove(&key(n));
        }
        assert_eq!(height(&writer, changed).unwrap(), 1);
        assert_eq!(audit_tree(&writer, changed, 10).unwrap().pages, 1);
        check(&writer, changed, &model);
        for n in [0, 768, 1536] {
            changed = put(&mut writer, changed, &key(n), b"after collapse").unwrap();
            model.insert(key(n), b"after collapse".to_vec());
        }
        check(&writer, changed, &model);
        let (table, _) = writer.seal(&directory.0).unwrap();
        let entries = table.entries();
        drop(table);
        let reopened = SegmentTable::load(&directory.0, &entries).unwrap();
        check(&reopened, changed, &model);
        check(&held, root, &original);
        assert_eq!(borrowed, &[0; 512]);
    }
}
