use std::path::{Path, PathBuf};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix};
use ulmdb_storage::{Arena, ReadStore, SegmentTable, WriteStore};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-savepoints-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn prefix() -> Prefix {
    Prefix::new(Address::V4(0x0a000000), 8).unwrap()
}
fn initialize(path: &Path) -> Environment {
    let env = Environment::create(path).unwrap();
    let mut writer = env.begin_write().unwrap();
    writer.create_database(b"kv", DatabaseKind::Kv).unwrap();
    writer
        .create_database(b"dups", DatabaseKind::Duplicates { width: Some(4) })
        .unwrap();
    writer
        .create_database(b"prefix", DatabaseKind::Ipv4)
        .unwrap();
    writer.put(b"kv", b"base", &vec![1; 8192]).unwrap();
    writer.insert_duplicate(b"dups", b"key", b"base").unwrap();
    writer.insert_prefix(b"prefix", prefix(), b"base").unwrap();
    writer.commit().unwrap();
    env
}

#[test]
fn nested_release_then_outer_rollback_restores_private_bytes_and_catalog() {
    let path = Directory::new();
    let env = initialize(&path.0);
    let stable = env.begin_read();
    let mut writer = env.begin_write().unwrap();
    let private = vec![2; 8192];
    writer.put(b"kv", b"private", &private).unwrap();
    let original_dirty = writer.dirty_bytes();
    {
        let mut outer = writer.savepoint().unwrap();
        outer.put(b"kv", b"private", &vec![3; 8192]).unwrap();
        outer.put(b"kv", b"base", &vec![4; 8192]).unwrap();
        outer.delete_duplicate(b"dups", b"key", b"base").unwrap();
        outer.insert_duplicate(b"dups", b"key", b"next").unwrap();
        outer.insert_prefix(b"prefix", prefix(), b"next").unwrap();
        outer
            .create_database(b"temporary", DatabaseKind::Kv)
            .unwrap();
        outer.put(b"temporary", b"key", b"discard").unwrap();
        {
            let mut inner = outer.savepoint().unwrap();
            inner.put(b"kv", b"private", &vec![5; 8192]).unwrap();
            inner.drop_database(b"prefix").unwrap();
            inner
                .create_database(b"released-child", DatabaseKind::Kv)
                .unwrap();
            inner.release().unwrap();
        }
        outer.abort().unwrap();
    }
    assert_eq!(writer.dirty_bytes(), original_dirty);
    assert_eq!(
        writer.get(b"kv", b"private").unwrap(),
        Some(private.as_slice())
    );
    assert_eq!(
        writer.get(b"kv", b"base").unwrap(),
        Some(vec![1; 8192].as_slice())
    );
    assert!(matches!(
        writer.get(b"temporary", b"key"),
        Err(Error::MissingDatabase)
    ));
    assert!(matches!(
        writer.get(b"released-child", b"key"),
        Err(Error::MissingDatabase)
    ));
    assert_eq!(writer.duplicate_count(b"dups", b"key").unwrap(), 1);
    assert!(writer.contains_duplicate(b"dups", b"key", b"base").unwrap());
    assert_eq!(
        writer
            .longest_match(b"prefix", Address::V4(0x0a000001))
            .unwrap()
            .unwrap()
            .1,
        b"base"
    );
    assert_eq!(writer.commit().unwrap(), 2);
    assert!(stable.get(b"kv", b"private").unwrap().is_none());
    assert_eq!(
        stable.get(b"kv", b"base").unwrap(),
        Some(vec![1; 8192].as_slice())
    );
    let now = env.begin_read();
    assert_eq!(
        now.get(b"kv", b"private").unwrap(),
        Some(private.as_slice())
    );
    assert_eq!(now.duplicate_count(b"dups", b"key").unwrap(), 1);
}

#[test]
fn inner_abort_outer_release_publishes_cross_index_changes_atomically() {
    let path = Directory::new();
    let env = initialize(&path.0);
    let before = env.begin_read();
    let mut writer = env.begin_write().unwrap();
    {
        let mut outer = writer.savepoint().unwrap();
        outer.put(b"kv", b"marker", b"outer").unwrap();
        outer.insert_duplicate(b"dups", b"key", b"next").unwrap();
        outer.insert_prefix(b"prefix", prefix(), b"outer").unwrap();
        {
            let mut inner = outer.savepoint().unwrap();
            inner.put(b"kv", b"marker", b"inner").unwrap();
            inner.delete_duplicates(b"dups", b"key").unwrap();
            inner.delete_prefix(b"prefix", prefix()).unwrap();
            inner.abort().unwrap();
        }
        assert_eq!(
            outer.get(b"kv", b"marker").unwrap(),
            Some(b"outer".as_slice())
        );
        assert_eq!(outer.duplicate_count(b"dups", b"key").unwrap(), 2);
        outer.release().unwrap();
    }
    assert!(before.get(b"kv", b"marker").unwrap().is_none());
    assert_eq!(writer.commit().unwrap(), 2);
    let now = env.begin_read();
    assert_eq!(
        now.get(b"kv", b"marker").unwrap(),
        Some(b"outer".as_slice())
    );
    assert_eq!(now.duplicate_count(b"dups", b"key").unwrap(), 2);
    assert_eq!(
        now.longest_match(b"prefix", Address::V4(0x0a000001))
            .unwrap()
            .unwrap()
            .1,
        b"outer"
    );
    drop(env);
    let reopened = Environment::open(&path.0).unwrap();
    assert_eq!(
        reopened
            .begin_read()
            .duplicate_count(b"dups", b"key")
            .unwrap(),
        2
    );
}

#[test]
fn failed_child_release_and_unwind_restore_parent_liveness() {
    let path = Directory::new();
    let env = initialize(&path.0);
    let mut writer = env.begin_write().unwrap();
    let dirty = writer.dirty_bytes();
    {
        let mut child = writer.savepoint().unwrap();
        child.put(b"kv", b"discard", b"value").unwrap();
        assert!(
            child
                .insert_duplicate(b"dups", b"key", b"bad-width")
                .is_err()
        );
        assert!(child.get(b"kv", b"base").is_err());
        assert!(child.release().is_err());
    }
    assert_eq!(writer.dirty_bytes(), dirty);
    assert!(writer.get(b"kv", b"discard").unwrap().is_none());
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut child = writer.savepoint().unwrap();
        child.put(b"kv", b"discard", b"unwind").unwrap();
        panic!("exercise savepoint Drop during unwind");
    }));
    assert!(panic.is_err());
    assert!(writer.get(b"kv", b"discard").unwrap().is_none());
    writer.put(b"kv", b"kept", b"parent-live").unwrap();
    writer.commit().unwrap();
    assert_eq!(
        env.begin_read().get(b"kv", b"kept").unwrap(),
        Some(b"parent-live".as_slice())
    );
}

#[test]
fn dropping_only_child_changes_is_a_no_op_commit_and_burns_database_ids() {
    let path = Directory::new();
    let env = initialize(&path.0);
    let before = ulmdb_storage::read_current(&path.0).unwrap();
    let mut writer = env.begin_write().unwrap();
    {
        let mut child = writer.savepoint().unwrap();
        child
            .create_database(b"temporary", DatabaseKind::Kv)
            .unwrap();
        child.put(b"kv", b"base", &vec![2; 8192]).unwrap();
        child.insert_duplicate(b"dups", b"key", b"next").unwrap();
    }
    assert_eq!(writer.dirty_bytes(), 0);
    assert_eq!(writer.commit().unwrap(), before.sequence);
    let after = ulmdb_storage::read_current(&path.0).unwrap();
    assert_eq!(after.generation, before.generation);
    let mut writer = env.begin_write().unwrap();
    {
        let mut child = writer.savepoint().unwrap();
        child
            .create_database(b"temporary", DatabaseKind::Kv)
            .unwrap();
    }
    writer
        .create_database(b"accepted", DatabaseKind::Kv)
        .unwrap();
    writer.commit().unwrap();
    assert_eq!(
        ulmdb_storage::read_current(&path.0).unwrap().next_database,
        before.next_database + 3
    );
}

#[test]
fn selected_savepoint_rolls_back_repeatedly_and_keeps_private_entry_state() {
    let path = Directory::new();
    let env = initialize(&path.0);
    let before = ulmdb_storage::read_current(&path.0).unwrap();
    let mut writer = env.begin_write().unwrap();
    let private = vec![9; 8192];
    writer.put(b"kv", b"private", &private).unwrap();
    let dirty = writer.dirty_bytes();
    {
        let mut selected = writer.savepoint().unwrap();
        for attempt in 0..6 {
            selected
                .put(b"kv", b"private", &vec![attempt; 8192])
                .unwrap();
            selected
                .create_database(b"attempt", DatabaseKind::Kv)
                .unwrap();
            {
                let mut nested = selected.savepoint().unwrap();
                nested.delete_duplicates(b"dups", b"key").unwrap();
                nested
                    .insert_prefix(b"prefix", prefix(), b"attempt")
                    .unwrap();
                nested.release().unwrap();
            }
            selected.rollback().unwrap();
            assert_eq!(selected.dirty_bytes(), dirty);
            assert_eq!(
                selected.get(b"kv", b"private").unwrap(),
                Some(private.as_slice())
            );
            assert_eq!(selected.duplicate_count(b"dups", b"key").unwrap(), 1);
            assert_eq!(
                selected
                    .longest_match(b"prefix", Address::V4(0x0a000001))
                    .unwrap()
                    .unwrap()
                    .1,
                b"base"
            );
            assert!(matches!(
                selected.get(b"attempt", b"key"),
                Err(Error::MissingDatabase)
            ));
        }
        assert!(
            selected
                .insert_duplicate(b"dups", b"key", b"invalid-width")
                .is_err()
        );
        selected.rollback().unwrap();
        selected.put(b"kv", b"accepted", b"value").unwrap();
        selected.release().unwrap();
    }
    writer.commit().unwrap();
    let after = ulmdb_storage::read_current(&path.0).unwrap();
    assert_eq!(after.next_database, before.next_database + 6);
    assert_eq!(
        env.begin_read().get(b"kv", b"private").unwrap(),
        Some(private.as_slice())
    );
    assert_eq!(
        env.begin_read().get(b"kv", b"accepted").unwrap(),
        Some(b"value".as_slice())
    );
}

#[test]
fn arena_before_images_are_bounded_per_object_and_nested_release_preserves_outer() {
    let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
    let mut value = vec![1; 65_536];
    let payload = arena.put_payload(&value).unwrap();
    let original_bytes = arena.dirty_bytes();
    let original_revision = arena.revision();
    let outer = arena.checkpoint().unwrap();
    for n in 0..1000 {
        value.fill((n % 255 + 2) as u8);
        assert_eq!(arena.replace_payload(payload, &value).unwrap(), payload);
    }
    assert_eq!(arena.savepoint_undo_bytes(), value.len() + 16);
    assert_eq!(arena.dirty_bytes(), original_bytes);
    let intermediate = value.clone();
    let inner = arena.checkpoint().unwrap();
    value.fill(7);
    arena.replace_payload(payload, &value).unwrap();
    assert_eq!(arena.savepoint_undo_bytes(), 2 * (value.len() + 16));
    arena.rollback_checkpoint(&inner).unwrap();
    assert_eq!(arena.payload(payload).unwrap(), intermediate);
    let inner = arena.checkpoint().unwrap();
    value.fill(8);
    arena.replace_payload(payload, &value).unwrap();
    arena.release_checkpoint(&inner).unwrap();
    assert_eq!(arena.savepoint_undo_bytes(), value.len() + 16);
    let appended = arena.allocate(b"appended", 16).unwrap();
    arena.rollback_checkpoint(&outer).unwrap();
    assert_eq!(arena.payload(payload).unwrap(), vec![1; 65_536]);
    assert_eq!(arena.dirty_bytes(), original_bytes);
    assert_eq!(arena.revision(), original_revision);
    assert_eq!(arena.savepoint_undo_bytes(), 0);
    assert!(arena.read(appended, 8).is_err());
    // Allocation bookkeeping is restored too: newly allocated/reused offsets
    // must accept exactly the new object's extent after rollback.
    let replacement = arena.allocate(b"new", 16).unwrap();
    assert_eq!(replacement, appended);
    arena.rewrite(replacement, b"now", 16).unwrap();
}

#[test]
fn checkpoint_identity_depth_and_undo_limit_fail_without_partial_modification() {
    let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
    let mut other = Arena::new(SegmentTable::default(), 1).unwrap();
    let payload = arena.put_payload(&vec![1; 4 * 1024 * 1024]).unwrap();
    let revision = arena.revision();
    let mut tokens = Vec::new();
    for _ in 0..Arena::MAX_SAVEPOINT_DEPTH {
        tokens.push(arena.checkpoint().unwrap());
    }
    assert!(matches!(arena.checkpoint(), Err(Error::LimitExceeded)));
    assert!(other.rollback_checkpoint(&tokens[0]).is_err());
    // 64 copies of this 4MiB+header object exceed the 256MiB undo budget.
    // The bound is checked before allocating/copying any before-image.
    assert!(matches!(
        arena.replace_payload(payload, &vec![2; 4 * 1024 * 1024]),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(arena.savepoint_undo_bytes(), 0);
    assert_eq!(arena.revision(), revision);
    assert!(
        arena
            .payload(payload)
            .unwrap()
            .iter()
            .all(|byte| *byte == 1)
    );
    assert!(arena.release_checkpoint(&tokens[0]).is_err());
    arena.rollback_checkpoint(&tokens[0]).unwrap();
    assert_eq!(arena.savepoint_depth(), 0);
    assert!(arena.rollback_checkpoint(&tokens[1]).is_err());
    let current = arena.checkpoint().unwrap();
    arena.release_checkpoint(&current).unwrap();
    assert!(arena.release_checkpoint(&current).is_err());
}

#[test]
fn forgotten_nested_guard_cannot_bypass_outer_rollback_or_active_seal_guard() {
    let path = Directory::new();
    let env = initialize(&path.0);
    let mut writer = env.begin_write().unwrap();
    {
        let mut outer = writer.savepoint().unwrap();
        let mut inner = outer.savepoint().unwrap();
        inner.put(b"kv", b"forgotten", b"value").unwrap();
        std::mem::forget(inner);
        assert!(outer.release().is_err()); // Drop rolls back outer + descendants.
    }
    assert!(writer.get(b"kv", b"forgotten").unwrap().is_none());
    assert_eq!(writer.commit().unwrap(), 1);
    let mut writer = env.begin_write().unwrap();
    let mut leaked = writer.savepoint().unwrap();
    leaked.put(b"kv", b"forgotten", b"value").unwrap();
    std::mem::forget(leaked);
    assert!(writer.commit().is_err());
    assert!(env.begin_read().get(b"kv", b"forgotten").unwrap().is_none());
}
