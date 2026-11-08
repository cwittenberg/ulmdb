use super::super::{Config, PrivateCache};
use super::*;
use crate::WriteStore;
#[test]
fn actual_chunk_clone_refcounts_are_bounded_by_touched_membership() {
    let pool = Pool::create(Config {
        kv_pages: 65,
        tiny_pages: 0,
        max_reserved_bytes: 1 << 20,
        max_metadata_bytes: 16 << 20,
        max_views: 8,
    })
    .unwrap();
    let cache = PrivateCache::create(&pool).unwrap();
    let mut arena = pool
        .begin_with_private_cache(pool.empty().unwrap(), &cache)
        .unwrap();
    for _ in 0..63 {
        arena.allocate(&[7; 4096], 4096).unwrap();
    }
    let base = arena.freeze().unwrap().activate();
    let counts: Vec<_> = (0..63)
        .map(|s| Arc::strong_count(base.version(s).unwrap()))
        .collect();
    let mut arena = pool.begin_with_private_cache(base.clone(), &cache).unwrap();
    arena.allocate(&[8; 4096], 4096).unwrap();
    let prepared = arena.freeze().unwrap();
    // One touched existing chunk copies exactly63 live Version owners.
    let increments: usize = (0..63)
        .map(|s| {
            let delta = Arc::strong_count(base.version(s).unwrap()) - counts[s as usize];
            assert_eq!(delta, 1);
            delta
        })
        .sum();
    assert_eq!(increments, 63);
    assert_eq!(prepared.view().member_count(), 64);
    drop(prepared);
    for s in 0..63 {
        assert_eq!(
            Arc::strong_count(base.version(s).unwrap()),
            counts[s as usize]
        );
    }
    drop(base);
    assert_eq!(pool.stats().unwrap().held_maps, 0);
    assert!(!cache.stats().unwrap().leased);
}
