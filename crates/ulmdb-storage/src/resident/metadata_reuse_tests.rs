use super::super::{Config, PrivateCache};
use super::*;
fn config() -> Config {
    Config {
        kv_pages: 8,
        tiny_pages: 4,
        max_reserved_bytes: 1 << 20,
        max_metadata_bytes: 8 << 20,
        max_views: 8,
    }
}
#[test]
fn warmed_range_and_frozen_capacity_return_without_pins_or_extra_lease() {
    let pool = Pool::create(config()).unwrap();
    let cache = PrivateCache::create(&pool).unwrap();
    let base = pool.empty().unwrap();
    let reserved = cache.stats().unwrap().aggregate_reserved_bytes;
    assert!(cache.stats().unwrap().frozen_capacity_bytes >= 12 * std::mem::size_of::<NewVersion>());
    let mut pointers = None;
    for generation in 1..=64 {
        let mut arena = pool.begin_with_private_cache(base.clone(), &cache).unwrap();
        let at = arena.allocate(&[generation as u8; PAGE], PAGE).unwrap();
        let prepared = arena.freeze().unwrap();
        let now = (
            prepared.slot(0).unwrap().ranges.as_ptr(),
            prepared.new.as_ptr(),
        );
        assert_eq!(prepared.slot(0).unwrap().ranges.len(), 1);
        if let Some(old) = pointers {
            assert_eq!(old, now);
        } else {
            pointers = Some(now);
        }
        assert!(cache.stats().unwrap().leased);
        assert!(matches!(
            pool.begin_with_private_cache(base.clone(), &cache),
            Err(Error::Busy)
        ));
        assert_eq!(Arc::strong_count(&prepared.new[0].version), 2);
        assert_eq!(prepared.view().read(at, 1).unwrap(), [generation as u8]);
        drop(prepared); // rejected prepublication candidate
        assert!(!cache.stats().unwrap().leased);
        assert_eq!(pool.stats().unwrap().held_maps, 0);
        assert_eq!(cache.stats().unwrap().aggregate_reserved_bytes, reserved);
    }
    let mut arena = pool.begin_with_private_cache(base.clone(), &cache).unwrap();
    let at = arena.allocate(&[9; PAGE], PAGE).unwrap();
    let view = arena.freeze().unwrap().activate();
    let held = view.read(at, 1).unwrap();
    assert_eq!(held, [9]);
    assert!(!cache.stats().unwrap().leased);
    // Empty cached scratch does not pin the selected Version; only the view does.
    assert_eq!(pool.stats().unwrap().held_maps, 1);
    drop(view);
    assert_eq!(pool.stats().unwrap().held_maps, 0);
    drop(cache);
    assert_eq!(pool.private_quota.used(), 0);
}
#[test]
fn failed_freeze_and_abort_clear_sealed_owners_and_restore_admitted_capacity() {
    let pool = Pool::create(Config {
        max_views: 2,
        ..config()
    })
    .unwrap();
    let cache = PrivateCache::create(&pool).unwrap();
    let base = pool.empty().unwrap();
    let blocker = pool.empty().unwrap();
    let reserved = cache.stats().unwrap().aggregate_reserved_bytes;
    let mut arena = pool.begin_with_private_cache(base.clone(), &cache).unwrap();
    arena.allocate(&[3; PAGE], PAGE).unwrap();
    assert!(matches!(arena.freeze(), Err(Error::Busy)));
    assert!(!cache.stats().unwrap().leased);
    assert_eq!(pool.stats().unwrap().held_maps, 0);
    drop(blocker);
    for _ in 0..16 {
        let mut arena = pool.begin_with_private_cache(base.clone(), &cache).unwrap();
        arena.allocate(&[4; PAGE], PAGE).unwrap();
        drop(arena);
        assert_eq!(pool.stats().unwrap().held_maps, 0);
        assert!(!cache.stats().unwrap().leased);
        assert_eq!(cache.stats().unwrap().aggregate_reserved_bytes, reserved);
    }
}

#[test]
fn retained_frozen_capacity_is_admitted_and_failed_cache_creation_releases_quota() {
    let pool = Pool::create(config()).unwrap();
    let cache = PrivateCache::create(&pool).unwrap();
    let bound = pool.stats().unwrap().metadata_bound_bytes;
    let admitted = cache.stats().unwrap().charged_capacity_bytes as u64;
    assert!(admitted > cache.stats().unwrap().frozen_capacity_bytes as u64);
    drop(cache);
    drop(pool);
    let pool = Pool::create(Config {
        max_metadata_bytes: bound + admitted - 1,
        ..config()
    })
    .unwrap();
    assert!(matches!(
        PrivateCache::create(&pool),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(pool.private_quota.used(), 0);
    assert_eq!(pool.stats().unwrap().held_maps, 0);
}

#[test]
fn tiny_append_reuses_only_unpublished_maps_and_discards_old_range_lengths() {
    let pool = Pool::create(config()).unwrap();
    let cache = PrivateCache::create(&pool).unwrap();
    let base = pool.empty().unwrap();
    let mut arena = pool.begin_with_private_cache(base.clone(), &cache).unwrap();
    let first = arena.allocate(&[1; 3000], 1).unwrap();
    arena.allocate(&[2; 500], 1).unwrap();
    let prepared = arena.freeze().unwrap();
    let ranges = prepared.slot(0).unwrap().ranges;
    assert_eq!(ranges, [(0, 3000), (3000, 500)]);
    let pointer = ranges.as_ptr();
    drop(prepared); // unpublished Tiny is eligible for reuse
    let mut arena = pool.begin_with_private_cache(base.clone(), &cache).unwrap();
    let second = arena.allocate(&[3], 1).unwrap();
    arena.allocate(&[4; 2], 1).unwrap();
    let prepared = arena.freeze().unwrap();
    assert_eq!(prepared.slot(0).unwrap().ranges, [(0, 1), (1, 2)]);
    assert_eq!(prepared.slot(0).unwrap().ranges.as_ptr(), pointer);
    assert_eq!(unpack(first).unwrap().0, unpack(second).unwrap().0);
    assert_ne!(first, second);
    assert!(prepared.view().read(first, 1).is_err());
    let view = prepared.activate();
    assert_eq!(view.read(second, 1).unwrap(), [3]);
    drop(view);
    assert_eq!(pool.stats().unwrap().retired_tiny_maps, 1);
    assert_eq!(pool.stats().unwrap().held_maps, 0);
    let mut arena = pool.begin_with_private_cache(base, &cache).unwrap();
    let third = arena.allocate(&[5], 1).unwrap();
    assert_ne!(unpack(second).unwrap().0, unpack(third).unwrap().0);
    drop(arena);
    assert!(!cache.stats().unwrap().leased);
}
