use super::super::Config;
use super::*;

fn pool(slots: usize) -> Arc<Pool> {
    Pool::create(Config {
        kv_pages: slots,
        tiny_pages: 0,
        max_reserved_bytes: 128 << 20,
        max_metadata_bytes: 256 << 20,
        max_views: 16,
    })
    .unwrap()
}

fn version(pool: &Arc<Pool>, slot: u32, generation: u64, byte: u8) -> Arc<Version> {
    let mut permit = pool.replay_permit(VersionKey { slot, generation }).unwrap();
    permit.write(0, &[byte; 4096]).unwrap();
    permit.ranges.push((0, 4096));
    permit.seal()
}

#[test]
fn overflow_groups_preserve_order_membership_old_bytes_and_untouched_owners() {
    let pool = pool(16384);
    let slots = [4095, 4096, 4159, 4160, 8191, 8192, 12287, 16383];
    let old = pool
        .empty()
        .unwrap()
        .changed(
            slots.into_iter().map(|s| version(&pool, s, 1, s as u8)),
            &[],
        )
        .unwrap();
    let keys: Vec<_> = old.members().collect();
    assert_eq!(keys.iter().map(|k| k.slot).collect::<Vec<_>>(), slots);
    let at = super::super::pool::tagged(4096, 1, 0);
    let held = old.read(at, 4096).unwrap();
    let pointer = held.as_ptr();
    let next = old
        .without_versions(&[VersionKey {
            slot: 4096,
            generation: 1,
        }])
        .unwrap();
    assert_eq!(next.member_count(), slots.len() - 1);
    assert_eq!(old.member_count(), slots.len());
    assert_eq!(held, [0; 4096]);
    assert_eq!(old.read(at, 4096).unwrap().as_ptr(), pointer);
    assert!(next.read(at, 1).is_err());
    assert!(!next.contains_version(VersionKey {
        slot: 4096,
        generation: 1
    }));
    for slot in slots.into_iter().filter(|s| *s != 4096) {
        assert!(next.contains_version(VersionKey {
            slot,
            generation: 1
        }));
        assert_eq!(
            next.read(super::super::pool::tagged(slot, 1, 0), 16)
                .unwrap(),
            [slot as u8; 16]
        );
    }
    assert!(Arc::ptr_eq(
        old.root.chunks[63].as_ref().unwrap(),
        next.root.chunks[63].as_ref().unwrap()
    ));
    let before = old.root.overflow.as_ref().unwrap();
    let after = next.root.overflow.as_ref().unwrap();
    assert!(!Arc::ptr_eq(before, after));
    assert!(!Arc::ptr_eq(
        before.groups[0].as_ref().unwrap(),
        after.groups[0].as_ref().unwrap()
    ));
    for group in [1, 2] {
        assert!(Arc::ptr_eq(
            before.groups[group].as_ref().unwrap(),
            after.groups[group].as_ref().unwrap()
        ));
    }
    let before = before.groups[0].as_ref().unwrap();
    let after = after.groups[0].as_ref().unwrap();
    assert!(!Arc::ptr_eq(
        before.chunks[0].as_ref().unwrap(),
        after.chunks[0].as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        before.chunks[1].as_ref().unwrap(),
        after.chunks[1].as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        before.chunks[63].as_ref().unwrap(),
        after.chunks[63].as_ref().unwrap()
    ));
    // Dropping a prospective changed view is a rollback: the original directory
    // and its byte ownership remain intact, with no retained extra view lease.
    drop(next);
    assert_eq!(old.members().collect::<Vec<_>>(), keys);
    assert_eq!(pool.stats().unwrap().view_roots, 1);
}

#[test]
fn overflow_last_configured_slot_is_not_reused_until_old_view_drops() {
    let pool = pool(8192);
    let key = VersionKey {
        slot: 8191,
        generation: 1,
    };
    let old = pool
        .empty()
        .unwrap()
        .changed([version(&pool, key.slot, 1, 7)], &[])
        .unwrap();
    let at = super::super::pool::tagged(key.slot, 1, 0);
    let held = old.read(at, 16).unwrap();
    let empty = old.without_versions(&[key]).unwrap();
    assert_eq!(empty.member_count(), 0);
    assert!(matches!(
        pool.replay_permit(VersionKey {
            generation: 2,
            ..key
        }),
        Err(Error::Corrupt(_))
    ));
    assert_eq!(held, [7; 16]);
    drop(old);
    let next = empty
        .changed([version(&pool, key.slot, 2, 9)], &[])
        .unwrap();
    assert!(!next.contains_version(key));
    assert!(next.read(at, 1).is_err());
    assert_eq!(
        next.read(super::super::pool::tagged(key.slot, 2, 0), 16)
            .unwrap(),
        [9; 16]
    );
    assert_eq!(pool.stats().unwrap().reused_pages, 0); // Replay is not a live-allocation statistic.
}

#[test]
fn invalid_direct_changes_do_not_panic_or_leak_leases_at_any_directory_level() {
    let pool = pool(8192);
    let key = VersionKey {
        slot: 4096,
        generation: 1,
    };
    let old = pool
        .empty()
        .unwrap()
        .changed([version(&pool, key.slot, 1, 3)], &[])
        .unwrap();
    let invalid = [
        VersionKey {
            slot: u32::MAX,
            generation: 1,
        },
        VersionKey {
            slot: 65534,
            generation: 1,
        },
        VersionKey {
            slot: 8192,
            generation: 1,
        },
        VersionKey {
            slot: 4095,
            generation: 1,
        },
        VersionKey {
            slot: 4096,
            generation: 2,
        },
    ];
    for _ in 0..32 {
        for key in invalid {
            assert!(matches!(
                old.changed(std::iter::empty(), &[key]),
                Err(Error::Corrupt(_))
            ));
        }
    }
    assert_eq!(old.member_count(), 1);
    assert_eq!(old.members().collect::<Vec<_>>(), vec![key]);
    assert_eq!(pool.stats().unwrap().view_roots, 1);
    assert!(!old.contains_version(VersionKey {
        slot: 8192,
        generation: 1
    }));
    assert!(!old.contains_version(VersionKey {
        slot: u32::MAX,
        generation: 1
    }));
    assert_eq!(
        old.read(super::super::pool::tagged(key.slot, 1, 0), 16)
            .unwrap(),
        [3; 16]
    );
}

#[test]
fn maximum_physical_slot_routes_to_last_group_and_capacity_charge_grows() {
    let pool = pool(1);
    let mut root = Root::empty(&pool).unwrap();
    *root.chunk_mut(65533, true).unwrap() = Some(Arc::new(Chunk::default()));
    assert!(
        root.overflow.as_ref().unwrap().groups[14]
            .as_ref()
            .unwrap()
            .chunks[63]
            .is_some()
    );
    assert!(root.chunks.iter().all(Option::is_none));
    assert!(matches!(
        root.chunk_mut(65536, true),
        Err(Error::Corrupt(_))
    ));
    assert_eq!(metadata_bound_bytes(4096), Some(66560));
    assert!(metadata_bound_bytes(8192).unwrap() > metadata_bound_bytes(4096).unwrap());
    assert!(metadata_bound_bytes(16384).unwrap() > metadata_bound_bytes(8192).unwrap());
    assert!(metadata_bound_bytes(65534).is_some());
    assert!(metadata_bound_bytes(65535).is_none());
}

#[test]
fn overflow_change_copies_version_owners_only_in_the_touched_chunk() {
    let pool = pool(16384);
    let old = pool
        .empty()
        .unwrap()
        .changed(
            (4096..4160)
                .chain(8192..8256)
                .map(|slot| version(&pool, slot, 1, slot as u8)),
            &[],
        )
        .unwrap();
    let counts: Vec<_> = (4096..4160)
        .chain(8192..8256)
        .map(|slot| (slot, Arc::strong_count(old.version(slot).unwrap())))
        .collect();
    let next = old
        .without_versions(&[VersionKey {
            slot: 4096,
            generation: 1,
        }])
        .unwrap();
    for &(slot, count) in &counts {
        let expected_delta = usize::from(slot > 4096 && slot < 4160);
        assert_eq!(
            Arc::strong_count(old.version(slot).unwrap()),
            count + expected_delta
        );
    }
    drop(next);
    for &(slot, count) in &counts {
        assert_eq!(Arc::strong_count(old.version(slot).unwrap()), count);
    }
}
