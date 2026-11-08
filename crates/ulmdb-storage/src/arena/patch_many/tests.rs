use super::*;

struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-file-multi-patch-{}",
            crate::new_identity().unwrap()
        ));
        crate::create_layout(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn private_spans_share_rewrite_undo_and_nested_rollback() {
    let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
    let at = arena.allocate(&[17; 4096], 4096).unwrap();
    let revision = arena.revision();
    let outer = arena.checkpoint().unwrap();
    arena
        .try_patch_many(at, 4096, &[(0, b"one"), (2048, b"two"), (4096, b"")], 4096)
        .unwrap();
    assert_eq!(arena.revision(), revision + 1);
    assert_eq!(arena.savepoint_undo_bytes(), 4096);
    let outer_image = arena.read(at, 4096).unwrap().to_vec();
    let inner = arena.checkpoint().unwrap();
    arena
        .try_patch_many(at, 4096, &[(0, b"new"), (4095, b"x")], 4096)
        .unwrap();
    arena.patch(at, 4096, 200, b"single", 4096).unwrap();
    assert_eq!(arena.savepoint_undo_bytes(), 8192);
    arena.rollback_checkpoint(&inner).unwrap();
    assert_eq!(arena.read(at, 4096).unwrap(), outer_image);
    assert_eq!(arena.revision(), revision + 1);
    arena.rollback_checkpoint(&outer).unwrap();
    assert_eq!(arena.read(at, 4096).unwrap(), [17; 4096]);
    assert_eq!(arena.revision(), revision);
    assert_eq!(arena.dirty_bytes(), 4096);
}

#[test]
fn invalid_last_span_and_revision_precede_all_stores() {
    let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
    let at = arena.allocate(&[17; 4096], 4096).unwrap();
    let revision = arena.revision();
    let bad: &[&[(usize, &[u8])]] = &[
        &[(0, b"x"), (4096, b"y")],
        &[(0, b"x"), (usize::MAX, b"y")],
        &[(0, b"x"), (0, b"y")],
        &[(2, b"x"), (1, b"y")],
        &[(0, b"a"), (1, b"b"), (2, b"c"), (3, b"d"), (4, b"e")],
    ];
    for spans in bad {
        assert!(arena.try_patch_many(at, 4096, spans, 4096).is_err());
        assert_eq!(arena.revision(), revision);
        assert_eq!(arena.read(at, 4096).unwrap(), [17; 4096]);
    }
    for reference in [
        Reference::NULL,
        Reference {
            offset: at.offset + 1,
            ..at
        },
        Reference {
            offset: at.offset + 4096,
            ..at
        },
    ] {
        assert!(
            arena
                .try_patch_many(reference, 4096, &[(0, b"x")], 4096)
                .is_err()
        );
    }
    arena.set_revision_for_test(u64::MAX);
    assert!(
        arena
            .try_patch_many(at, 4096, &[(0, b"x"), (4000, b"y")], 4096)
            .is_err()
    );
    assert_eq!(
        arena
            .try_patch_many(at, 4096, &[(4095, &[17])], 4096)
            .unwrap(),
        Some(at)
    );
    assert_eq!(arena.read(at, 4096).unwrap(), [17; 4096]);
    assert_eq!(arena.savepoint_undo_bytes(), 0);
    assert_eq!(arena.revision(), u64::MAX);
    assert_eq!(arena.read(at, 4096).unwrap(), [17; 4096]);
}

#[test]
fn immutable_page_copies_once_and_keeps_held_mapping_and_abort() {
    let dir = Directory::new();
    let mut source = Arena::new(SegmentTable::default(), 1).unwrap();
    let old = source.allocate(&[17; 4096], 4096).unwrap();
    let (base, _) = source.seal(&dir.0).unwrap();
    let held = base.read(old, 4096).unwrap();
    let pointer = held.as_ptr();
    let mut arena = Arena::new(base.clone(), 2).unwrap();
    assert_eq!(
        arena
            .try_patch_many(old, 4096, &[(0, &[17])], 4096)
            .unwrap(),
        Some(old)
    );
    assert_eq!(arena.dirty_bytes(), 0);
    let point = arena.checkpoint().unwrap();
    let new = arena
        .try_patch_many(old, 4096, &[(0, b"x"), (4095, b"y")], 4096)
        .unwrap()
        .unwrap();
    assert_ne!(new, old);
    assert_eq!(arena.objects.len(), 1);
    assert_eq!(arena.dirty_bytes(), 4096);
    assert_eq!(arena.revision(), 1);
    arena.try_patch_many(new, 4096, &[(3, b"z")], 4096).unwrap();
    assert_eq!(arena.objects.len(), 1);
    arena.rollback_checkpoint(&point).unwrap();
    assert_eq!(arena.dirty_bytes(), 0);
    assert!(arena.read(new, 4096).is_err());
    assert_eq!(held, [17; 4096]);
    assert_eq!(held.as_ptr(), pointer);
}

#[test]
fn mapped_page_preflights_capacity_and_restores_private_bytes() {
    let dir = Directory::new();
    let mut source = Arena::new(SegmentTable::default(), 1).unwrap();
    let old = source.allocate(&[17; 4096], 4096).unwrap();
    let (base, _) = source.seal(&dir.0).unwrap();
    let mut full = Arena::new_mapped(base.clone(), 2, &dir.0, 4096, MappedFlush::None).unwrap();
    assert!(matches!(
        full.try_patch_many(old, 4096, &[(0, b"x")], 4096),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(full.dirty_bytes(), 0);
    assert_eq!(full.revision(), 0);
    let mut arena = Arena::new_mapped(base, 3, &dir.0, 8192, MappedFlush::None).unwrap();
    let at = arena
        .try_patch_many(old, 4096, &[(0, b"x"), (4095, b"y")], 4096)
        .unwrap()
        .unwrap();
    let before = arena.read(at, 4096).unwrap().to_vec();
    let point = arena.checkpoint().unwrap();
    arena
        .try_patch_many(at, 4096, &[(0, b"a"), (4095, b"b")], 4096)
        .unwrap();
    arena.rollback_checkpoint(&point).unwrap();
    assert_eq!(arena.read(at, 4096).unwrap(), before);
    assert_eq!(arena.dirty_bytes(), 4096);
}

#[test]
fn page_shapes_are_explicit_and_largest_slot_offsets_are_supported() {
    for size in [256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536] {
        let base = SegmentTable::default()
            .with_page_size(ulmdb_core::PageSize::new(size).unwrap())
            .unwrap();
        let mut arena = Arena::new(base, 1).unwrap();
        assert!(arena.supports_patch_many(size, size));
        assert!(!arena.supports_patch_many(size - 1, size));
        assert_eq!(
            arena
                .try_patch_many(Reference::NULL, size - 1, &[(usize::MAX, b"bad")], size)
                .unwrap(),
            None
        );
        let at = arena.allocate(&vec![17; size], size).unwrap();
        arena
            .try_patch_many(at, size, &[(0, b"a"), (size - 1, b"b"), (size, b"")], size)
            .unwrap();
        assert_eq!(arena.read(at, size).unwrap()[0], b'a');
        assert_eq!(arena.read(at, size).unwrap()[size - 1], b'b');
    }
}
