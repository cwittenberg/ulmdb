use super::super::{Config, Pool};
use super::*;
use crate::WriteStore;

fn populated(pages: usize) -> (Arc<Pool>, View, Vec<Reference>) {
    let pool = Pool::create(Config {
        kv_pages: pages,
        tiny_pages: 0,
        max_reserved_bytes: 64 * 1024 * 1024,
        max_metadata_bytes: 64 * 1024 * 1024,
        max_views: 32,
    })
    .unwrap();
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    let references = (0..pages)
        .map(|n| arena.allocate(&[n as u8; 4096], 4096).unwrap())
        .collect();
    (pool, arena.freeze().unwrap().activate(), references)
}

#[test]
fn exact_removal_preserves_borrows_and_copies_only_affected_chunks() {
    let (_pool, original, refs) = populated(130);
    let keys: Vec<_> = original.members().collect();
    let held = original.read(refs[0], 16).unwrap();
    let removed = [keys[0], keys[64]];
    let next = original.without_versions(&removed).unwrap();
    assert_eq!(next.member_count(), 128);
    assert_eq!(original.member_count(), 130);
    assert!(next.read(refs[0], 1).is_err());
    assert!(next.read(refs[64], 1).is_err());
    assert_eq!(held, [0; 16]);
    assert_eq!(next.read(refs[129], 16).unwrap(), [129; 16]);
    for index in [0, 1] {
        assert!(!Arc::ptr_eq(
            original.root.chunks[index].as_ref().unwrap(),
            next.root.chunks[index].as_ref().unwrap(),
        ));
    }
    assert!(Arc::ptr_eq(
        original.root.chunks[2].as_ref().unwrap(),
        next.root.chunks[2].as_ref().unwrap(),
    ));
    let unchanged = original.without_versions(&[]).unwrap();
    assert!(Arc::ptr_eq(&original.root, &unchanged.root));
}

#[test]
fn invalid_removals_are_atomic_and_do_not_consume_view_leases() {
    let (pool, original, refs) = populated(3);
    let keys: Vec<_> = original.members().collect();
    let mut wrong_generation = keys[1];
    wrong_generation.generation += 1;
    let invalid = [
        vec![keys[0], keys[0]],
        vec![keys[1], keys[0]],
        vec![keys[0], wrong_generation],
        vec![VersionKey {
            slot: u32::MAX,
            generation: 1,
        }],
        vec![keys[0]; 4],
    ];
    for _ in 0..64 {
        for remove in &invalid {
            assert!(matches!(
                original.without_versions(remove),
                Err(Error::Corrupt(_))
            ));
            assert_eq!(original.members().collect::<Vec<_>>(), keys);
            assert_eq!(original.read(refs[0], 16).unwrap(), [0; 16]);
        }
    }
    // More invalid attempts than the view quota must not prevent a valid view.
    let next = original.without_versions(&keys[..1]).unwrap();
    assert_eq!(next.member_count(), 2);
    assert!(!pool.stats().unwrap().poisoned);
}

#[test]
fn logical_removal_does_not_reuse_a_slot_until_last_old_owner_releases_it() {
    let (pool, old, refs) = populated(1);
    let key = old.members().next().unwrap();
    let next = old.without_versions(&[key]).unwrap();
    let held = old.read(refs[0], 16).unwrap();
    assert!(
        pool.begin(next.clone())
            .unwrap()
            .allocate(&[9; 4096], 4096)
            .is_err()
    );
    assert_eq!(held, [0; 16]);
    drop(old);
    let mut arena = pool.begin(next).unwrap();
    let reused = arena.allocate(&[9; 4096], 4096).unwrap();
    assert_ne!(reused, refs[0]);
    let ready = arena.freeze().unwrap().activate();
    assert!(ready.read(refs[0], 1).is_err());
    assert_eq!(ready.read(reused, 16).unwrap(), [9; 16]);
    assert_eq!(pool.stats().unwrap().reused_pages, 1);
}
