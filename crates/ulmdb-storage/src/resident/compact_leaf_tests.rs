//! Real Resident ownership and mutation checks for private compact validity.
use super::*;
use crate::resident::Config;

fn pool(pages: usize) -> Arc<Pool> {
    Pool::create(Config {
        kv_pages: pages,
        tiny_pages: 0,
        max_reserved_bytes: 1 << 24,
        max_metadata_bytes: 1 << 24,
        max_views: 32,
    })
    .unwrap()
}

fn leaf(rows: &[(u64, u64)]) -> [u8; PAGE] {
    assert!(rows.len() <= (PAGE - 32) / 16);
    let mut bytes = [0; PAGE];
    bytes[..2].copy_from_slice(&2u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&(rows.len() as u16).to_le_bytes());
    bytes[8..10].copy_from_slice(&32u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&((32 + rows.len() * 16) as u16).to_le_bytes());
    bytes[12..14].copy_from_slice(&8u16.to_le_bytes());
    bytes[14..16].copy_from_slice(&8u16.to_le_bytes());
    for (index, &(key, value)) in rows.iter().enumerate() {
        let start = 32 + index * 16;
        bytes[start..start + 8].copy_from_slice(&key.to_be_bytes());
        bytes[start + 8..start + 16].copy_from_slice(&value.to_be_bytes());
    }
    bytes
}

fn admitted(arena: &Arena, at: Reference) -> bool {
    arena.admitted_compact_leaf(at, arena.read(at, PAGE).unwrap())
}

fn cached(arena: &Arena, at: Reference) -> bool {
    let (slot, generation, _) = unpack(at).unwrap();
    let private = arena.private.get(&slot).unwrap().unwrap();
    assert_eq!(private.generation, generation);
    private.has_compact_leaf_certificate()
}

fn assert_rows<S: ReadStore>(store: &S, at: Reference, rows: &[(u64, u64)]) {
    ulmdb_kv::audit_tree(store, at, 1).unwrap();
    for &(key, value) in rows {
        assert_eq!(
            ulmdb_kv::get(store, at, &key.to_be_bytes()).unwrap(),
            Some(&value.to_be_bytes()[..])
        );
    }
    let actual = ulmdb_kv::Cursor::new(
        store,
        at,
        std::ops::Bound::Unbounded,
        std::ops::Bound::Unbounded,
        false,
    )
    .unwrap()
    .map(|row| {
        let (key, value) = row.unwrap();
        (
            u64::from_be_bytes(key.try_into().unwrap()),
            u64::from_be_bytes(value.try_into().unwrap()),
        )
    })
    .collect::<Vec<_>>();
    assert_eq!(actual, rows);
}

#[cfg(feature = "resident-direct-page-patch")]
fn append(arena: &mut Arena, at: Reference, key: u64, value: u64) -> Reference {
    let bytes = arena.read(at, PAGE).unwrap();
    let count = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
    let upper = u16::from_le_bytes(bytes[10..12].try_into().unwrap());
    let next_count = (count + 1).to_le_bytes();
    let next_upper = (upper + 16).to_le_bytes();
    let key = key.to_be_bytes();
    let value = value.to_be_bytes();
    arena
        .try_patch_many(
            at,
            PAGE,
            &[
                (6, &next_count),
                (10, &next_upper),
                (usize::from(upper), &key),
                (usize::from(upper) + 8, &value),
            ],
            PAGE,
        )
        .unwrap()
        .unwrap()
}

#[test]
fn raw_allocation_earns_validity_only_for_its_exact_private_borrow() {
    let pool = pool(8);
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    let bytes = leaf(&[(1, 10), (2, 20)]);
    let at = arena.allocate(&bytes, PAGE).unwrap();
    assert!(!cached(&arena, at));
    assert!(!arena.admitted_compact_leaf(at, &bytes));
    assert!(!cached(&arena, at));

    let foreign_pool = self::pool(8);
    let mut foreign = foreign_pool.begin(foreign_pool.empty().unwrap()).unwrap();
    let other = foreign.allocate(&bytes, PAGE).unwrap();
    assert!(!arena.admitted_compact_leaf(at, foreign.read(other, PAGE).unwrap()));
    assert!(!cached(&arena, at));
    assert!(admitted(&arena, at));
    assert!(cached(&arena, at));
    assert!(!arena.admitted_compact_leaf(at, &bytes));
    assert!(!arena.admitted_compact_leaf(other, foreign.read(other, PAGE).unwrap()));
    assert_rows(&arena, at, &[(1, 10), (2, 20)]);
}

#[cfg(feature = "resident-direct-page-patch")]
#[test]
fn exact_appends_and_whole_values_preserve_contents_without_new_owners() {
    let pool = pool(8);
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    let at = arena.allocate(&leaf(&[(1, 10)]), PAGE).unwrap();
    assert!(admitted(&arena, at));
    let allocations = pool.stats().unwrap().allocations;
    let revision = arena.revision();
    let mut rows = vec![(1, 10)];
    for key in 2..=12 {
        assert_eq!(append(&mut arena, at, key, key * 10), at);
        assert!(cached(&arena, at));
        rows.push((key, key * 10));
    }
    assert_eq!(arena.revision(), revision + 11);
    assert_eq!(pool.stats().unwrap().allocations, allocations);
    assert_rows(&arena, at, &rows);

    assert_eq!(
        arena
            .patch(at, PAGE, 32 + 4 * 16 + 8, &777u64.to_be_bytes(), PAGE)
            .unwrap(),
        at
    );
    assert!(cached(&arena, at));
    rows[4].1 = 777;
    assert_rows(&arena, at, &rows);
    assert_eq!(pool.stats().unwrap().allocations, allocations);
}

#[test]
fn unsupported_partial_value_and_rewrite_revalidate_before_reuse() {
    let pool = pool(8);
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    let at = arena.allocate(&leaf(&[(1, 10), (2, 20)]), PAGE).unwrap();
    assert!(admitted(&arena, at));
    assert_eq!(arena.patch(at, PAGE, 47, &[11], PAGE).unwrap(), at);
    assert!(!cached(&arena, at));
    assert_rows(&arena, at, &[(1, 11), (2, 20)]);
    assert!(admitted(&arena, at));

    assert_eq!(
        arena.rewrite(at, &leaf(&[(1, 30), (2, 40)]), PAGE).unwrap(),
        at
    );
    assert!(!cached(&arena, at));
    assert_rows(&arena, at, &[(1, 30), (2, 40)]);
    assert!(admitted(&arena, at));

    // A page-shaped raw edit must not retain validity merely because its
    // header and searched last key still look plausible.
    arena.patch(at, PAGE, PAGE - 1, &[1], PAGE).unwrap();
    assert!(!cached(&arena, at));
    assert!(!admitted(&arena, at));
    let revision = arena.revision();
    assert!(matches!(
        ulmdb_kv::put(&mut arena, at, &3u64.to_be_bytes(), &50u64.to_be_bytes()),
        Err(Error::Corrupt("nonzero compact page tail"))
    ));
    assert_eq!(arena.revision(), revision);
    assert_eq!(arena.read(at, PAGE).unwrap()[PAGE - 1], 1);
}

#[test]
fn unsupported_key_edit_cannot_hide_an_unordered_source() {
    let pool = pool(8);
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    let at = arena.allocate(&leaf(&[(1, 10), (2, 20)]), PAGE).unwrap();
    assert!(admitted(&arena, at));
    arena
        .patch(at, PAGE, 32, &99u64.to_be_bytes(), PAGE)
        .unwrap();
    assert!(!cached(&arena, at));
    assert!(!admitted(&arena, at));
    let revision = arena.revision();
    assert!(matches!(
        ulmdb_kv::put(&mut arena, at, &100u64.to_be_bytes(), &50u64.to_be_bytes()),
        Err(Error::Corrupt("unordered tree keys"))
    ));
    assert_eq!(arena.revision(), revision);
}

#[test]
fn base_cow_stays_checked_and_abort_preserves_retained_old_view() {
    let pool = pool(8);
    let mut init = pool.begin(pool.empty().unwrap()).unwrap();
    let old = init.allocate(&leaf(&[(1, 10), (2, 20)]), PAGE).unwrap();
    assert!(admitted(&init, old));
    let view = init.freeze().unwrap().activate();
    let held = view.read(old, PAGE).unwrap();
    let pointer = held.as_ptr();
    let before = pool.stats().unwrap();
    {
        let mut arena = pool.begin(view.clone()).unwrap();
        assert!(!admitted(&arena, old));
        let new = arena
            .patch(old, PAGE, 40, &30u64.to_be_bytes(), PAGE)
            .unwrap();
        assert_ne!(new, old);
        assert!(!cached(&arena, new));
        assert_rows(&arena, new, &[(1, 30), (2, 20)]);
        assert_rows(&view, old, &[(1, 10), (2, 20)]);
        assert!(admitted(&arena, new));
        assert_eq!(
            arena
                .patch(new, PAGE, 40, &40u64.to_be_bytes(), PAGE)
                .unwrap(),
            new
        );
        assert!(cached(&arena, new));
        assert_rows(&arena, new, &[(1, 40), (2, 20)]);
        assert_eq!(held.as_ptr(), pointer);
        assert_eq!(held, &leaf(&[(1, 10), (2, 20)]));
    }
    assert_eq!(pool.stats().unwrap().held_maps, before.held_maps);
    assert_rows(&view, old, &[(1, 10), (2, 20)]);
    assert_eq!(held.as_ptr(), pointer);
}

#[test]
fn sealing_drops_writer_validity_and_next_writer_copies_shared_bytes() {
    let pool = pool(8);
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    let old = arena.allocate(&leaf(&[(1, 10)]), PAGE).unwrap();
    assert!(admitted(&arena, old));
    let view = arena.freeze().unwrap().activate();
    let mut next = pool.begin(view.clone()).unwrap();
    assert!(!admitted(&next, old));
    let new = next
        .patch(old, PAGE, 40, &20u64.to_be_bytes(), PAGE)
        .unwrap();
    assert_ne!(old, new);
    assert!(!cached(&next, new));
    assert!(admitted(&next, new));
    assert_rows(&next, new, &[(1, 20)]);
    assert_rows(&view, old, &[(1, 10)]);
}

#[test]
fn nested_undo_release_and_ancestor_rollback_discard_stale_validity() {
    let pool = pool(8);
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    let at = arena.allocate(&leaf(&[(1, 10), (2, 20)]), PAGE).unwrap();
    assert!(admitted(&arena, at));
    let initial_revision = arena.revision();
    let outer = arena.checkpoint().unwrap();
    arena
        .patch(at, PAGE, 40, &30u64.to_be_bytes(), PAGE)
        .unwrap();
    assert!(cached(&arena, at));
    let inner = arena.checkpoint().unwrap();
    arena.patch(at, PAGE, PAGE - 1, &[1], PAGE).unwrap();
    assert!(!admitted(&arena, at));
    arena.rollback_checkpoint(&inner).unwrap();
    assert!(!cached(&arena, at));
    assert_rows(&arena, at, &[(1, 30), (2, 20)]);
    assert!(admitted(&arena, at));

    let inner = arena.checkpoint().unwrap();
    arena
        .patch(at, PAGE, 40, &40u64.to_be_bytes(), PAGE)
        .unwrap();
    let removed = arena.allocate(&leaf(&[(3, 50)]), PAGE).unwrap();
    assert!(admitted(&arena, removed));
    arena.release_checkpoint(&inner).unwrap();
    assert!(cached(&arena, at));
    assert_rows(&arena, at, &[(1, 40), (2, 20)]);
    arena.rollback_checkpoint(&outer).unwrap();
    assert!(!cached(&arena, at));
    assert_rows(&arena, at, &[(1, 10), (2, 20)]);
    assert!(arena.read(removed, PAGE).is_err());
    assert_eq!(arena.revision(), initial_revision);
    assert_eq!(arena.savepoint_depth(), 0);
    assert_eq!(arena.savepoint_undo_bytes(), 0);
    assert!(admitted(&arena, at));
}

#[test]
fn reused_slot_never_inherits_validity_or_accepts_a_stale_reference() {
    let pool = pool(1);
    let old = {
        let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
        let at = arena.allocate(&leaf(&[(1, 10)]), PAGE).unwrap();
        assert!(admitted(&arena, at));
        at
    };
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    let mut corrupt = leaf(&[(2, 20)]);
    corrupt[PAGE - 1] = 1;
    let new = arena.allocate(&corrupt, PAGE).unwrap();
    let a = unpack(old).unwrap();
    let b = unpack(new).unwrap();
    assert_eq!(a.0, b.0);
    assert!(a.1 < b.1);
    assert!(!cached(&arena, new));
    assert!(arena.read(old, PAGE).is_err());
    assert!(!arena.admitted_compact_leaf(old, arena.read(new, PAGE).unwrap()));
    assert!(!admitted(&arena, new));
    assert!(matches!(
        ulmdb_kv::audit_tree(&arena, new, 1),
        Err(Error::Corrupt("nonzero compact page tail"))
    ));
}

#[test]
fn rejected_preflight_keeps_certified_bytes_revision_and_owner() {
    let pool = pool(8);
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    let bytes = leaf(&[(1, 10)]);
    let at = arena.allocate(&bytes, PAGE).unwrap();
    assert!(admitted(&arena, at));
    let allocations = pool.stats().unwrap().allocations;
    let revision = arena.revision();
    assert!(arena.patch(at, PAGE, PAGE, &[1], PAGE).is_err());
    assert_eq!(arena.revision(), revision);
    assert!(cached(&arena, at));
    arena.revision = u64::MAX;
    assert!(
        arena
            .patch(at, PAGE, 40, &20u64.to_be_bytes(), PAGE)
            .is_err()
    );
    assert_eq!(arena.revision(), u64::MAX);
    assert!(cached(&arena, at));
    assert_eq!(arena.read(at, PAGE).unwrap(), bytes);
    assert_eq!(pool.stats().unwrap().allocations, allocations);
    assert_rows(&arena, at, &[(1, 10)]);
}
