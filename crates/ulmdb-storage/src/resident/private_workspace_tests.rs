use super::super::super::{Config, Pool, VersionKey};
use crate::{ReadStore, WriteStore};
fn config() -> Config {
    Config {
        kv_pages: 8,
        tiny_pages: 4,
        max_reserved_bytes: 1 << 20,
        max_metadata_bytes: 16 << 20,
        max_views: 8,
    }
}
fn baseline_and_charge() -> (u64, u64) {
    let p = Pool::create(config()).unwrap();
    let c = PrivateCache::create(&p).unwrap();
    (
        p.stats().unwrap().metadata_bound_bytes,
        c.stats().unwrap().charged_capacity_bytes as u64,
    )
}
fn bounded(copies: u64) -> Arc<Pool> {
    let (base, charge) = baseline_and_charge();
    Pool::create(Config {
        max_metadata_bytes: base + copies * charge,
        ..config()
    })
    .unwrap()
}
#[test]
fn aggregate_idle_cache_empty_arenas_and_foreign_quota() {
    let p = bounded(1);
    let v = p.empty().unwrap();
    let owners = Arc::strong_count(&p);
    let cache = PrivateCache::create(&p).unwrap();
    assert_eq!(Arc::strong_count(&p), owners, "cache must not own Pool");
    let charged = cache.stats().unwrap().charged_capacity_bytes as u64;
    assert_eq!(p.private_quota.used(), charged);
    assert!(matches!(p.begin(v.clone()), Err(Error::LimitExceeded)));
    assert!(matches!(
        PrivateCache::create(&p),
        Err(Error::LimitExceeded)
    ));
    let foreign = bounded(2);
    assert!(
        p.begin_with_private_cache(v.clone(), &PrivateCache::create(&foreign).unwrap())
            .is_err()
    );
    let a = p.begin_with_private_cache(v.clone(), &cache).unwrap();
    assert!(matches!(
        p.begin_with_private_cache(v.clone(), &cache),
        Err(Error::Busy)
    ));
    drop(a);
    assert!(!cache.stats().unwrap().leased);
    assert_eq!(
        p.private_quota.used(),
        charged,
        "idle retained allocation remains charged"
    );
    drop(cache);
    assert_eq!(p.private_quota.used(), 0);
    let a = p.begin(v.clone()).unwrap();
    assert!(matches!(p.begin(v), Err(Error::LimitExceeded)));
    drop(a);
    assert_eq!(p.private_quota.used(), 0);
}
#[test]
fn concurrent_empty_arenas_share_one_atomic_admission() {
    let p = bounded(2);
    let v = p.empty().unwrap();
    let start = Arc::new(std::sync::Barrier::new(4));
    let hold = Arc::new(std::sync::Barrier::new(4));
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let (p, v, start, hold) = (
                Arc::clone(&p),
                v.clone(),
                Arc::clone(&start),
                Arc::clone(&hold),
            );
            std::thread::spawn(move || {
                start.wait();
                let result = p.begin(v);
                hold.wait();
                result.is_ok()
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .map(|t| usize::from(t.join().unwrap()))
            .sum::<usize>(),
        2
    );
    assert_eq!(p.private_quota.used(), 0);
}
#[test]
fn quota_reconcile_failure_and_exact_drop_release() {
    let q = super::super::super::private_quota::Quota::new(100);
    let mut a = q.reserve(60).unwrap();
    let b = q.reserve(40).unwrap();
    assert!(a.reconcile(61).is_err());
    assert_eq!(q.used(), 100);
    drop(b);
    a.reconcile(75).unwrap();
    assert_eq!(q.used(), 75);
    a.reconcile(20).unwrap();
    assert_eq!(q.used(), 20);
    drop(a);
    assert_eq!(q.used(), 0);
    assert!(q.reserve(101).is_err());
    assert_eq!(q.used(), 0);
}
#[test]
fn packed_lookup_matches_btree_and_freeze_order_and_abort_capacity_reuse() {
    let p = bounded(1);
    let cache = PrivateCache::create(&p).unwrap();
    let initial = cache
        .state
        .lock()
        .unwrap()
        .buffers
        .as_ref()
        .map(|b| (b.positions.as_ptr() as usize, b.entries.as_ptr() as usize))
        .unwrap();
    for _ in 0..8 {
        let mut pages = cache.take(&p).unwrap();
        let mut values: Vec<_> = (0..8)
            .map(|_| Private::new(p.reserve(false).unwrap()))
            .collect();
        values.reverse();
        let mut model = std::collections::BTreeMap::new();
        for private in values {
            model.insert(private.slot, private.generation);
            pages.insert(private.slot, private).unwrap();
        }
        for slot in 0..12 {
            assert_eq!(
                pages.get(&slot).unwrap().map(|p| p.generation),
                model.get(&slot).copied()
            );
            assert_eq!(
                pages.get_mut(&slot).unwrap().map(|p| p.generation),
                model.get(&slot).copied()
            );
        }
        let drained: Vec<_> = pages
            .drain_sorted()
            .unwrap()
            .map(|p| (p.slot, p.generation))
            .collect();
        assert_eq!(drained, model.into_iter().collect::<Vec<_>>());
        pages.finish().unwrap();
        let state = cache.state.lock().unwrap();
        let b = state.buffers.as_ref().unwrap();
        assert!(b.entries.is_empty());
        assert!(b.positions.iter().all(|x| *x == 0));
        assert_eq!(
            (b.positions.as_ptr() as usize, b.entries.as_ptr() as usize),
            initial
        );
    }
    let mut pages = cache.take(&p).unwrap();
    let private = Private::new(p.reserve(false).unwrap());
    pages.insert(private.slot, private).unwrap();
    drop(pages); // Abort must return all maps/indices, without owning a snapshot.
    assert_eq!(p.stats().unwrap().free_pages, 8);
    assert!(
        cache
            .state
            .lock()
            .unwrap()
            .buffers
            .as_ref()
            .unwrap()
            .positions
            .iter()
            .all(|x| *x == 0)
    );
}
#[test]
fn wrong_position_slot_generation_and_lease_epoch_fail_closed() {
    let p = bounded(1);
    let c = PrivateCache::create(&p).unwrap();
    let mut a = c.take(&p).unwrap();
    for _ in 0..2 {
        let x = Private::new(p.reserve(false).unwrap());
        a.insert(x.slot, x).unwrap();
    }
    let b = a.buffers.as_mut().unwrap();
    let slot = b.entries[1].slot as usize;
    b.positions[slot] = 1;
    assert!(a.get(&(slot as u32)).is_err());
    a.buffers.as_mut().unwrap().positions[slot] = u16::MAX;
    assert!(a.get_mut(&(slot as u32)).is_err());
    a.buffers.as_mut().unwrap().positions[slot] = 2;
    a.finish().unwrap();
    c.state.lock().unwrap().epoch = u64::MAX;
    assert!(matches!(c.take(&p), Err(Error::LimitExceeded)));
    assert!(!c.stats().unwrap().leased);
    c.state.lock().unwrap().epoch = 0;
    let mut a = c.take(&p).unwrap();
    c.state.lock().unwrap().epoch += 1;
    assert!(a.finish().is_err());
    assert!(c.take(&p).is_err());
    assert_eq!(
        p.private_quota.used(),
        c._header.bytes(),
        "failed return drops buffers but retained poisoned cache header stays charged"
    );
    drop(c);
    assert_eq!(p.private_quota.used(), 0);
}
#[test]
fn failed_freeze_abort_empty_boundaries_and_reused_generations() {
    let p = Pool::create(Config {
        max_views: 2,
        ..config()
    })
    .unwrap();
    let c = PrivateCache::create(&p).unwrap();
    let base = p.empty().unwrap();
    let second = p.empty().unwrap();
    let mut a = p.begin_with_private_cache(base.clone(), &c).unwrap();
    let stale = a.allocate(&[1; 4096], 4096).unwrap();
    assert!(matches!(a.freeze(), Err(Error::Busy)));
    assert!(!c.stats().unwrap().leased);
    assert_eq!(p.stats().unwrap().free_pages, 8);
    drop(second);
    let mut a = p.begin_with_private_cache(base.clone(), &c).unwrap();
    let first = a.allocate(&[2; 3000], 1).unwrap();
    let second = a.allocate(&[3; 2000], 1).unwrap();
    assert!(a.read(second, 0).unwrap().is_empty());
    assert_eq!(a.read(first, 1).unwrap(), [2]);
    let full = a.allocate(&[4; 4096], 4096).unwrap();
    assert_ne!(stale, full);
    let prepared = a.freeze().unwrap();
    assert!(prepared.view().read(stale, 1).is_err());
    assert!(prepared.view().read(second, 0).unwrap().is_empty());
    let mut observed = Vec::new();
    for index in 0..12 {
        if let Some(slot) = prepared.slot(index) {
            observed.push(slot.key);
        }
    }
    assert!(observed.windows(2).all(|w| w[0].slot < w[1].slot));
    assert!(observed.contains(&VersionKey {
        slot: 0,
        generation: 2
    }));
    let view = prepared.activate();
    let held = view.read(full, 1).unwrap();
    assert_eq!(held, [4]);
    assert!(!c.stats().unwrap().leased);
    assert!(p.begin_with_private_cache(base, &c).is_ok());
    assert_eq!(held, [4]);
}
