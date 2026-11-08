use super::*;
use crate::resident::Config;

fn pool(pages: usize) -> Arc<Pool> {
    Pool::create(Config {
        kv_pages: pages,
        tiny_pages: 2,
        max_reserved_bytes: 1 << 24,
        max_metadata_bytes: 1 << 24,
        max_views: 32,
    })
    .unwrap()
}

#[test]
fn private_patch_changes_only_spans_with_one_tick_and_no_new_owner() {
    let pool = pool(8);
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    let old = arena.allocate(&[17; PAGE], PAGE).unwrap();
    let mut expected = [17; PAGE];
    let patches = [
        (0, &[1, 2][..]),
        (6, &[3, 4][..]),
        (PAGE - 1, &[5][..]),
        (PAGE, &[][..]),
    ];
    for &(offset, bytes) in &patches {
        expected[offset..offset + bytes.len()].copy_from_slice(bytes);
    }
    let revision = arena.revision();
    let allocations = pool.stats().unwrap().allocations;
    assert_eq!(
        arena.try_patch_many(old, PAGE, &patches, PAGE).unwrap(),
        Some(old)
    );
    assert_eq!(arena.revision(), revision + 1);
    assert_eq!(pool.stats().unwrap().allocations, allocations);
    assert_eq!(arena.read(old, PAGE).unwrap(), expected);
    let frozen = arena.freeze().unwrap();
    let slots: Vec<_> = frozen.slots().collect();
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].ranges, [(0, PAGE as u16)]);
    assert_eq!(slots[0].copy_source, None);
}

#[test]
fn committed_source_is_copied_once_preserves_borrow_abort_and_source_hint() {
    let pool = pool(8);
    let mut init = pool.begin(pool.empty().unwrap()).unwrap();
    let old = init.allocate(&[17; PAGE], PAGE).unwrap();
    let view = init.freeze().unwrap().activate();
    let held = view.read(old, PAGE).unwrap();
    let pointer = held.as_ptr();
    let allocations = pool.stats().unwrap().allocations;
    let mut arena = pool.begin(view.clone()).unwrap();
    let new = arena
        .try_patch_many(
            old,
            PAGE,
            &[(6, &[0, 1]), (10, &[2, 3]), (32, &[4, 5]), (34, &[6, 7])],
            PAGE,
        )
        .unwrap()
        .unwrap();
    assert_ne!(new, old);
    assert_eq!(arena.revision(), 1);
    assert_eq!(pool.stats().unwrap().allocations, allocations + 1);
    assert_eq!(held, [17; PAGE]);
    assert_eq!(held.as_ptr(), pointer);
    let new_key = unpack(new).unwrap();
    let source = unpack(old).unwrap();
    let frozen = arena.freeze().unwrap();
    let slots: Vec<_> = frozen.slots().collect();
    assert_eq!(slots.len(), 1);
    assert_eq!(
        slots[0].key,
        VersionKey {
            slot: new_key.0,
            generation: new_key.1
        }
    );
    assert_eq!(
        slots[0].copy_source,
        Some(VersionKey {
            slot: source.0,
            generation: source.1
        })
    );
    assert_eq!(slots[0].ranges, [(0, PAGE as u16)]);
    assert_eq!(&slots[0].bytes[6..8], [0, 1]);
    drop(slots);
    drop(frozen);
    assert_eq!(view.read(old, PAGE).unwrap(), [17; PAGE]);
}

#[test]
fn all_spans_are_preflighted_before_any_private_write_or_revision() {
    let pool = pool(8);
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    let old = arena.allocate(&[17; PAGE], PAGE).unwrap();
    let revision = arena.revision();
    let one = [9u8];
    let bad_sets: &[&[(usize, &[u8])]] = &[
        &[(0, &one), (PAGE, &one)],
        &[(0, &one), (usize::MAX, &one)],
        &[(0, &one), (0, &one)],
        &[(2, &one), (1, &one)],
        &[(0, &one), (1, &one), (2, &one), (3, &one), (4, &one)],
    ];
    for patches in bad_sets {
        assert!(arena.try_patch_many(old, PAGE, patches, PAGE).is_err());
        assert_eq!(arena.revision(), revision);
        assert_eq!(arena.read(old, PAGE).unwrap(), [17; PAGE]);
    }
    for reference in [
        Reference::NULL,
        Reference { segment: 2, ..old },
        Reference {
            offset: old.offset + (1 << 28),
            ..old
        },
        Reference {
            offset: old.offset + 1,
            ..old
        },
    ] {
        assert!(
            arena
                .try_patch_many(reference, PAGE, &[(0, &one)], PAGE)
                .is_err()
        );
        assert_eq!(arena.revision(), revision);
        assert_eq!(arena.read(old, PAGE).unwrap(), [17; PAGE]);
    }
}

#[test]
fn noops_overflow_exact_extent_and_unsupported_shapes_preserve_bytes() {
    let pool = pool(8);
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    let old = arena.allocate(&[17; PAGE], PAGE).unwrap();
    let tiny = arena.allocate(&[17; 128], 16).unwrap();
    arena.revision = u64::MAX;
    for patches in [&[][..], &[(0, &[17][..]), (PAGE, &[][..])][..]] {
        assert_eq!(
            arena.try_patch_many(old, PAGE, patches, PAGE).unwrap(),
            Some(old)
        );
    }
    assert!(arena.try_patch_many(old, PAGE, &[(0, &[9])], PAGE).is_err());
    assert!(
        arena
            .try_patch_many(tiny, PAGE, &[(0, &[9])], PAGE)
            .is_err()
    );
    assert_eq!(
        arena
            .try_patch_many(Reference::NULL, 128, &[(usize::MAX, &[9])], 16)
            .unwrap(),
        None
    );
    assert_eq!(arena.revision(), u64::MAX);
    assert_eq!(arena.read(old, PAGE).unwrap(), [17; PAGE]);
    assert_eq!(arena.read(tiny, 128).unwrap(), [17; 128]);
}

#[test]
fn exhausted_pool_and_quarantine_never_modify_committed_source() {
    let pool = pool(1);
    let mut init = pool.begin(pool.empty().unwrap()).unwrap();
    let old = init.allocate(&[17; PAGE], PAGE).unwrap();
    let view = init.freeze().unwrap().activate();
    let mut arena = pool.begin(view.clone()).unwrap();
    assert!(arena.try_patch_many(old, PAGE, &[(0, &[9])], PAGE).is_err());
    assert_eq!(arena.revision(), 1); // Existing allocation ticks before reservation.
    assert_eq!(pool.stats().unwrap().held_maps, 1);
    assert_eq!(view.read(old, PAGE).unwrap(), [17; PAGE]);
    pool.quarantine();
    let revision = arena.revision();
    assert!(arena.try_patch_many(old, PAGE, &[(0, &[9])], PAGE).is_err());
    assert_eq!(arena.revision(), revision);
}

#[test]
fn committed_noop_at_revision_max_and_cow_then_private_keep_original_hint() {
    let pool = pool(8);
    let mut init = pool.begin(pool.empty().unwrap()).unwrap();
    let old = init.allocate(&[17; PAGE], PAGE).unwrap();
    let view = init.freeze().unwrap().activate();
    let held = view.read(old, PAGE).unwrap();
    let before = pool.stats().unwrap();
    let mut arena = pool.begin(view.clone()).unwrap();
    arena.revision = u64::MAX;
    assert_eq!(
        arena
            .try_patch_many(old, PAGE, &[(0, &[17])], PAGE)
            .unwrap(),
        Some(old)
    );
    assert_eq!(pool.stats().unwrap().allocations, before.allocations);
    assert_eq!(arena.revision(), u64::MAX);
    arena.revision = 0;
    let new = arena
        .try_patch_many(old, PAGE, &[(0, &[1]), (32, &[2])], PAGE)
        .unwrap()
        .unwrap();
    assert_eq!(
        arena
            .try_patch_many(new, PAGE, &[(8, &[3]), (34, &[4])], PAGE)
            .unwrap(),
        Some(new)
    );
    assert_eq!(arena.revision(), 2);
    assert_eq!(pool.stats().unwrap().allocations, before.allocations + 1);
    assert_eq!(held, [17; PAGE]);
    let frozen = arena.freeze().unwrap();
    let slots: Vec<_> = frozen.slots().collect();
    assert_eq!(slots.len(), 1);
    let source = unpack(old).unwrap();
    assert_eq!(
        slots[0].copy_source,
        Some(VersionKey {
            slot: source.0,
            generation: source.1
        })
    );
    assert_eq!(slots[0].bytes[0], 1);
    assert_eq!(slots[0].bytes[8], 3);
    assert_eq!(slots[0].bytes[32], 2);
    assert_eq!(slots[0].bytes[34], 4);
    drop(slots);
    drop(frozen);
    let after = pool.stats().unwrap();
    assert_eq!(after.held_maps, before.held_maps);
    assert_eq!(after.view_roots, before.view_roots);
    assert_eq!(held, [17; PAGE]);
}

#[cfg(feature = "resident-private-index")]
#[test]
fn abort_restores_private_metadata_quota_and_held_owners() {
    let pool = pool(8);
    let mut init = pool.begin(pool.empty().unwrap()).unwrap();
    let old = init.allocate(&[17; PAGE], PAGE).unwrap();
    let view = init.freeze().unwrap().activate();
    let before = pool.stats().unwrap();
    let quota = pool.private_quota.used();
    {
        let mut arena = pool.begin(view.clone()).unwrap();
        assert!(pool.private_quota.used() > quota);
        let new = arena
            .try_patch_many(old, PAGE, &[(0, &[9])], PAGE)
            .unwrap()
            .unwrap();
        assert_eq!(arena.read(new, 1).unwrap(), [9]);
        assert_eq!(view.read(old, 1).unwrap(), [17]);
    }
    assert_eq!(pool.private_quota.used(), quota);
    let after = pool.stats().unwrap();
    assert_eq!(after.held_maps, before.held_maps);
    assert_eq!(after.view_roots, before.view_roots);
    // Quota rejection before arena construction creates no map or view owner.
    let reservation = pool
        .private_quota
        .reserve(pool.private_quota.limit() - quota)
        .unwrap();
    assert!(pool.begin(view.clone()).is_err());
    assert_eq!(pool.stats().unwrap().held_maps, before.held_maps);
    assert_eq!(pool.stats().unwrap().view_roots, before.view_roots);
    drop(reservation);
    assert_eq!(pool.private_quota.used(), quota);
}
