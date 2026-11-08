use super::*;
#[test]
fn private_range_repeats_capture_one_whole_object_per_checkpoint() {
    let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
    let at = arena.allocate(&[0; 4096], 4096).unwrap();
    let original_revision = arena.revision();
    let outer = arena.checkpoint().unwrap();
    for n in 1..1000u64 {
        assert_eq!(
            arena.patch(at, 4096, 40, &n.to_le_bytes(), 4096).unwrap(),
            at
        );
    }
    assert_eq!(arena.dirty_bytes(), 4096);
    assert_eq!(arena.savepoint_undo_bytes(), 4096);
    let outer_revision = arena.revision();
    let inner = arena.checkpoint().unwrap();
    arena.patch(at, 4096, 200, b"inner", 4096).unwrap();
    assert_eq!(arena.savepoint_undo_bytes(), 8192);
    arena.rollback_checkpoint(&inner).unwrap();
    assert_eq!(
        &arena.read(at, 4096).unwrap()[40..48],
        &999u64.to_le_bytes()
    );
    assert_eq!(&arena.read(at, 4096).unwrap()[200..205], &[0; 5]);
    assert_eq!(arena.revision(), outer_revision);
    assert_eq!(arena.savepoint_undo_bytes(), 4096);
    let inner = arena.checkpoint().unwrap();
    arena.patch(at, 4096, 100, b"accepted", 4096).unwrap();
    arena.release_checkpoint(&inner).unwrap();
    assert_eq!(arena.savepoint_undo_bytes(), 4096);
    arena.rollback_checkpoint(&outer).unwrap();
    assert_eq!(arena.read(at, 4096).unwrap(), [0; 4096]);
    assert_eq!(arena.revision(), original_revision);
}
#[test]
fn journal_budget_revision_and_invalid_ranges_leave_data_and_frames_unchanged() {
    let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
    let size = Arena::MAX_SAVEPOINT_UNDO_BYTES / Arena::MAX_SAVEPOINT_DEPTH + 1;
    let at = arena.allocate(&vec![0; size], 16).unwrap();
    let revision = arena.revision();
    let mut frames = Vec::new();
    for _ in 0..Arena::MAX_SAVEPOINT_DEPTH {
        frames.push(arena.checkpoint().unwrap());
    }
    assert!(matches!(
        arena.patch(at, size, 100, b"changed", 16),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(arena.savepoint_undo_bytes(), 0);
    assert_eq!(arena.savepoint_depth(), Arena::MAX_SAVEPOINT_DEPTH);
    assert_eq!(arena.revision(), revision);
    assert_eq!(&arena.read(at, size).unwrap()[100..107], &[0; 7]);
    for token in frames.iter().rev() {
        arena.release_checkpoint(token).unwrap();
    }
    arena.set_revision_for_test(u64::MAX);
    assert!(matches!(
        arena.patch(at, size, 0, b"x", 16),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(arena.patch(at, size, 0, b"\0", 16).unwrap(), at);
    assert_eq!(arena.revision(), u64::MAX);
    assert!(arena.patch(at, size, usize::MAX, b"xx", 16).is_err());
    assert!(arena.patch(at, size - 1, 0, b"x", 16).is_err());
    assert_eq!(arena.savepoint_undo_bytes(), 0);
    assert_eq!(arena.read(at, 1).unwrap(), b"\0");
}
#[test]
fn base_copy_is_one_private_object_and_old_mapping_remains_unchanged() {
    let path = std::env::temp_dir().join(format!("ulmdb-patch-{}", crate::new_identity().unwrap()));
    crate::create_layout(&path).unwrap();
    let mut source = Arena::new(SegmentTable::default(), 1).unwrap();
    let old = source.allocate(&[0; 4096], 4096).unwrap();
    let (base, _) = source.seal(&path).unwrap();
    let old_bytes = base.read(old, 4096).unwrap();
    let mut writer = Arena::new(base.clone(), 2).unwrap();
    assert_eq!(writer.patch(old, 4096, 16, &[0; 8], 4096).unwrap(), old);
    assert_eq!(writer.dirty_bytes(), 0);
    let new = writer.patch(old, 4096, 16, b"12345678", 4096).unwrap();
    assert_ne!(new, old);
    assert_eq!(writer.dirty_bytes(), 4096);
    assert_eq!(writer.objects.len(), 1);
    assert_eq!(old_bytes, [0; 4096]);
    assert_eq!(&writer.read(new, 4096).unwrap()[16..24], b"12345678");
    assert_eq!(writer.patch(new, 4096, 16, b"abcdefgh", 4096).unwrap(), new);
    assert_eq!(writer.dirty_bytes(), 4096);
    drop(writer);
    drop(base);
    std::fs::remove_dir_all(path).unwrap();
}
