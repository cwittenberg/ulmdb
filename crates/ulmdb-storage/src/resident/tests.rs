use super::*;
use crate::{ReadStore, Reference, WriteStore};
use std::collections::BTreeSet;
fn pool(pages: usize, tiny: usize) -> std::sync::Arc<Pool> {
    Pool::create(Config {
        kv_pages: pages,
        tiny_pages: tiny,
        max_reserved_bytes: 64 * 1024 * 1024,
        max_metadata_bytes: 64 * 1024 * 1024,
        max_views: 32,
    })
    .unwrap()
}
#[test]
fn actual_kv_pages_wrap_with_membership_generation_and_old_borrow_backpressure() {
    let p = pool(4, 32);
    let mut view = p.empty().unwrap();
    let mut root = Reference::NULL;
    for round in 0..12u8 {
        let mut a = p.begin(view.clone()).unwrap();
        root = ulmdb_kv::put(&mut a, root, b"key", &[round; 32]).unwrap();
        let next = a.freeze().unwrap().activate();
        drop(view);
        view = next;
        let mut keep = BTreeSet::new();
        let vroot = root;
        keep.insert(VersionKey {
            slot: ((vroot.offset & ((1 << 28) - 1)) / 4096 - 1) as u32,
            generation: vroot.offset >> 28,
        });
        // The first ownership slice explicitly retains all allocated tiny pages;
        // actual graph retirement is the next facade certificate, not inferred here.
        for k in view.members() {
            if k.slot >= 4 {
                keep.insert(k);
            }
        }
        view = view.retain(&keep).unwrap();
        assert_eq!(
            ulmdb_kv::get(&view, root, b"key").unwrap(),
            Some([round; 32].as_slice())
        );
    }
    assert!(p.stats().unwrap().reused_pages > 0);
    assert_eq!(p.stats().unwrap().maps_created, 36);
    let old = view.clone();
    let held = ulmdb_kv::get(&old, root, b"key").unwrap().unwrap();
    let mut a = p.begin(view.clone()).unwrap();
    let new = ulmdb_kv::put(&mut a, root, b"key", b"next").unwrap();
    view = a.freeze().unwrap().activate();
    assert_eq!(held, [11; 32]);
    assert_eq!(
        ulmdb_kv::get(&view, new, b"key").unwrap(),
        Some(b"next".as_slice())
    );
}
#[test]
fn stale_free_cross_extent_foreign_and_quarantine_reject_before_borrow() {
    let p = pool(1, 1);
    let empty = p.empty().unwrap();
    let mut a = p.begin(empty.clone()).unwrap();
    let old = a.allocate(&[7; 4096], 4096).unwrap();
    let view = a.freeze().unwrap().activate();
    assert!(
        p.begin(view.clone())
            .unwrap()
            .allocate(&[8; 4096], 4096)
            .is_err()
    );
    assert!(
        view.read(
            Reference {
                offset: old.offset + (1 << 28),
                ..old
            },
            1
        )
        .is_err()
    );
    assert!(
        view.read(
            Reference {
                offset: old.offset + 4090,
                ..old
            },
            16
        )
        .is_err()
    );
    let foreign = pool(1, 1);
    assert!(foreign.begin(view.clone()).is_err());
    drop(view);
    let mut a = p.begin(empty.clone()).unwrap();
    let new = a.allocate(&[9; 4096], 4096).unwrap();
    assert_ne!(new, old);
    let ready = a.freeze().unwrap();
    assert!(ready.view().read(old, 1).is_err());
    ready.quarantine();
    assert!(p.stats().unwrap().poisoned);
    assert_eq!(p.stats().unwrap().quarantined_maps, 2);
    assert!(p.begin(empty).is_err());
}
#[test]
fn tiny_range_budget_and_private_exact_object_guards_do_not_corrupt_prior_bytes() {
    let p = pool(1, 1);
    let empty = p.empty().unwrap();
    let mut a = p.begin(empty).unwrap();
    let mut refs = Vec::new();
    for n in 0..256u16 {
        refs.push(a.allocate(&[n as u8], 1).unwrap());
    }
    assert!(a.allocate(&[99], 1).is_err());
    for (n, r) in refs.iter().enumerate() {
        assert_eq!(a.read(*r, 1).unwrap(), [n as u8]);
    }
    let bad = Reference {
        offset: refs[0].offset + 1,
        ..refs[0]
    };
    assert!(a.rewrite(bad, &[5; 2], 1).is_err());
    assert!(a.patch(refs[0], 2, 0, &[1], 1).is_err());
    let v = a.freeze().unwrap().activate();
    assert_eq!(v.member_count(), 1);
    assert_eq!(p.stats().unwrap().maps_created, 2);
}
#[test]
fn aborted_generation_and_escaped_candidate_capture_never_allow_aba() {
    let p = pool(1, 0);
    let empty = p.empty().unwrap();
    let first = {
        let mut a = p.begin(empty.clone()).unwrap();
        a.allocate(&[1; 4096], 4096).unwrap()
    };
    let mut a = p.begin(empty.clone()).unwrap();
    let next = a.allocate(&[2; 4096], 4096).unwrap();
    assert_ne!(first, next);
    assert!(a.read(first, 1).is_err());
    let prepared = a.freeze().unwrap();
    let capture = prepared.view().clone();
    drop(prepared);
    assert!(
        p.begin(empty.clone())
            .unwrap()
            .allocate(&[3; 4096], 4096)
            .is_err()
    );
    assert_eq!(capture.read(next, 1).unwrap(), [2]);
    drop(capture);
    let mut a = p.begin(empty).unwrap();
    let third = a.allocate(&[3; 4096], 4096).unwrap();
    assert_ne!(third, next);
    assert!(a.read(next, 1).is_err());
}
#[test]
fn quarantine_fences_existing_private_mutators_and_preserves_old_readers() {
    let p = pool(2, 1);
    let empty = p.empty().unwrap();
    let mut a = p.begin(empty.clone()).unwrap();
    let old = a.allocate(&[1; 4096], 4096).unwrap();
    let view = a.freeze().unwrap().activate();
    let held = view.read(old, 32).unwrap();
    let mut a = p.begin(view.clone()).unwrap();
    let private = a.allocate(&[2; 4096], 4096).unwrap();
    p.quarantine();
    assert!(a.rewrite(private, &[3; 4096], 4096).is_err());
    assert!(a.patch(private, 4096, 0, &[3], 4096).is_err());
    assert!(a.freeze().is_err());
    assert_eq!(held, [1; 32]);
    drop(view);
    drop(empty);
    assert_eq!(p.stats().unwrap().quarantined_maps, 3);
}
#[test]
fn both_prefix_families_use_packed_checked_tiny_objects_and_old_snapshots() {
    use ulmdb_core::{Address, Prefix};
    let p = pool(2, 16);
    let empty = p.empty().unwrap();
    let mut a = p.begin(empty).unwrap();
    let mut v4 = Reference::NULL;
    let mut v6 = Reference::NULL;
    let p4 = Prefix::new(Address::V4(0), 0).unwrap();
    let p6 = Prefix::new(Address::V6(0), 0).unwrap();
    v4 = ulmdb_prefix::insert(&mut a, v4, p4, b"old4").unwrap();
    v6 = ulmdb_prefix::insert(&mut a, v6, p6, b"old6").unwrap();
    for n in 0..16u32 {
        v4 = ulmdb_prefix::insert(
            &mut a,
            v4,
            Prefix::new(Address::V4(0x0a00_0000 | (n << 16)), 16).unwrap(),
            &n.to_le_bytes(),
        )
        .unwrap();
        v6 = ulmdb_prefix::insert(
            &mut a,
            v6,
            Prefix::new(
                Address::V6((0x2001_0db8u128 << 96) | ((n as u128) << 64)),
                64,
            )
            .unwrap(),
            &n.to_le_bytes(),
        )
        .unwrap();
    }
    let old = a.freeze().unwrap().activate();
    assert!(old.member_count() < 16);
    let held = ulmdb_prefix::get(&old, v4, Address::V4(0xc000_0201))
        .unwrap()
        .unwrap()
        .1;
    assert_eq!(held, b"old4");
    let mut a = p.begin(old.clone()).unwrap();
    v4 = ulmdb_prefix::insert(&mut a, v4, p4, b"new4").unwrap();
    v6 = ulmdb_prefix::insert(&mut a, v6, p6, b"new6").unwrap();
    let new = a.freeze().unwrap().activate();
    assert_eq!(held, b"old4");
    assert_eq!(
        ulmdb_prefix::get(&new, v4, Address::V4(0xc000_0201))
            .unwrap()
            .unwrap()
            .1,
        b"new4"
    );
    assert_eq!(
        ulmdb_prefix::get(&new, v6, Address::V6(0xfd00u128 << 112))
            .unwrap()
            .unwrap()
            .1,
        b"new6"
    );
    assert!(
        ulmdb_prefix::subnets(&new, v4, p4)
            .unwrap()
            .all(|r| r.is_ok())
    );
    assert!(
        ulmdb_prefix::subnets(&new, v6, p6)
            .unwrap()
            .all(|r| r.is_ok())
    );
}
#[test]
fn valid_zero_length_current_and_one_past_payload_reads_are_both_preserved() {
    let p = pool(1, 4);
    let mut a = p.begin(p.empty().unwrap()).unwrap();
    a.allocate(&[1; 3000], 16).unwrap();
    let second = a.allocate(&[2; 2000], 16).unwrap();
    assert!(a.read(second, 0).unwrap().is_empty());
    let v = a.freeze().unwrap().activate();
    assert!(v.read(second, 0).unwrap().is_empty());
    let p = pool(1, 2);
    let mut a = p.begin(p.empty().unwrap()).unwrap();
    a.allocate(&[0; 4080], 16).unwrap();
    let empty = a.put_payload(&[]).unwrap();
    assert_eq!(a.payload(empty).unwrap(), b"");
    let v = a.freeze().unwrap().activate();
    assert_eq!(v.payload(empty).unwrap(), b"");
}
#[test]
fn bytes_only_serving_path_matches_traced_membership_for_valid_stale_and_empty_extents() {
    let p = pool(2, 4);
    let mut a = p.begin(p.empty().unwrap()).unwrap();
    let first = a.allocate(&[3; 3000], 1).unwrap();
    let second = a.allocate(&[4; 2000], 1).unwrap();
    let page = a.allocate(&[7; 4096], 4096).unwrap();
    let view = a.freeze().unwrap().activate();
    for base in [first, second, page] {
        for offset in 0..=4097u64 {
            for len in [0, 1, 8, 16, 24, 96, 256, 2000, 3000, 4096, usize::MAX] {
                for stale in [false, true] {
                    let at = Reference {
                        segment: base.segment,
                        offset: base.offset + offset + if stale { 1 << 28 } else { 0 },
                    };
                    let serving = view.read(at, len);
                    let traced = view.read_with_key(at, len).map(|(bytes, _)| bytes);
                    assert_eq!(serving.ok(), traced.ok(), "{at:?} len{len}");
                }
            }
        }
    }
    for segment in [0, 2, u64::MAX] {
        let at = Reference {
            segment,
            offset: first.offset,
        };
        assert_eq!(view.read(at, 1).is_ok(), view.read_with_key(at, 1).is_ok());
    }
}
#[test]
fn committed_full_kv_copy_hint_survives_private_edits_but_new_and_tiny_stay_literal() {
    let p = pool(8, 4);
    let mut a = p.begin(p.empty().unwrap()).unwrap();
    let old = a.allocate(&[1; 4096], 4096).unwrap();
    let tiny = a.allocate(&[2; 96], 16).unwrap();
    let prepared = a.freeze().unwrap();
    assert!(prepared.slots().all(|s| s.copy_source.is_none()));
    let view = prepared.activate();
    let held = view.read(old, 4096).unwrap();
    let mut a = p.begin(view.clone()).unwrap();
    let copied = a.rewrite(old, &[3; 4096], 4096).unwrap();
    assert_eq!(a.rewrite(copied, &[4; 4096], 4096).unwrap(), copied);
    assert_eq!(a.patch(copied, 4096, 24, &[5; 8], 4096).unwrap(), copied);
    let fresh = a.allocate(&[6; 4096], 4096).unwrap();
    a.rewrite(tiny, &[7; 96], 16).unwrap();
    let prepared = a.freeze().unwrap();
    let oldkey = super::pool::unpack(old).unwrap();
    let copiedkey = super::pool::unpack(copied).unwrap();
    let freshkey = super::pool::unpack(fresh).unwrap();
    for s in prepared.slots() {
        if s.key.slot == copiedkey.0 {
            assert_eq!(
                s.copy_source,
                Some(VersionKey {
                    slot: oldkey.0,
                    generation: oldkey.1
                })
            );
            assert_eq!(&s.bytes[24..32], &[5; 8]);
        } else {
            assert!(s.copy_source.is_none());
        }
        if s.key.slot == freshkey.0 {
            assert!(s.copy_source.is_none());
        }
    }
    assert_eq!(held, &[1; 4096]);
}
