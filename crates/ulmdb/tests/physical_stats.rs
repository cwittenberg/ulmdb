//! Real split/overflow/duplicate/private-view statistics, not count-based estimates.
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use ulmdb::{DatabaseKind, Environment, PageSize, PublicationMode};
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
struct Dir(PathBuf);

#[test]
fn resident_metrics_follow_exact_pinned_pool_across_checkpoint_and_reopen() {
    let d = Dir::new();
    let options = ulmdb::resident::Options::default();
    let env = ulmdb::resident::Environment::create(&d.0, options).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    for index in 0..128u32 {
        w.put(b"kv", &index.to_be_bytes(), &[7; 128]).unwrap();
    }
    w.commit().unwrap();
    let old = env.begin_read();
    let before = old.database_stats(b"kv").unwrap();
    assert_eq!(before.entries, 128);
    assert!(before.leaf_pages > 1 && before.branch_pages > 0);
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", &0u32.to_be_bytes(), &[9; 256]).unwrap();
    w.commit().unwrap();
    env.checkpoint().unwrap();
    assert_eq!(old.database_stats(b"kv").unwrap(), before);
    drop(env);
    let env = ulmdb::resident::Environment::open(&d.0, options).unwrap();
    let read = env.begin_read();
    assert_eq!(read.database_stats(b"kv").unwrap().entries, 128);
    assert_eq!(
        read.get(b"kv", &0u32.to_be_bytes()).unwrap(),
        Some([9; 256].as_slice())
    );
    assert_eq!(old.database_stats(b"kv").unwrap(), before);
}
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "physical-stats-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn actual_split_pages_and_overflow_extents_follow_persisted_geometry() {
    for mode in MODES {
        for size in [256, 4096, 65536] {
            let d = Dir::new();
            let e = Environment::create_with_page_size(&d.0, mode, PageSize::new(size).unwrap())
                .unwrap();
            let mut w = e.begin_write().unwrap();
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            let empty = w.database_stats(b"kv").unwrap();
            assert_eq!(
                (
                    empty.entries,
                    empty.depth,
                    empty.tree_bytes,
                    empty.overflow_bytes
                ),
                (0, 0, 0, 0)
            );
            for n in 0..512u64 {
                w.put(b"kv", &n.to_be_bytes(), &[7; 512]).unwrap();
            }
            w.put(b"kv", b"big", &vec![9; 100_000]).unwrap();
            let private = w.database_stats(b"kv").unwrap();
            assert_eq!(private.entries, 513);
            assert_eq!(private.page_size, size);
            assert!(private.depth >= 2 && private.branch_pages > 0 && private.leaf_pages > 1);
            assert_eq!(
                private.tree_bytes,
                (private.branch_pages + private.leaf_pages) * size as u64
            );
            let expected = 100_016 + if size == 256 { 512 * 528 } else { 0 };
            assert_eq!(private.overflow_bytes, expected);
            assert_eq!(
                private.overflow_objects,
                1 + if size == 256 { 512 } else { 0 }
            );
            assert!(private.overflow_pages > 0);
            assert_eq!(
                private.reachable_bytes,
                private.tree_bytes + private.overflow_bytes
            );
            w.commit().unwrap();
            let read = e.begin_read();
            assert_eq!(read.database_stats(b"kv").unwrap(), private);
            let cat = read.catalog_physical_stats().unwrap();
            assert_eq!(cat.entries, 1);
            assert!(cat.leaf_pages > 0);
            drop(read);
            drop(e);
            let e = Environment::open(&d.0).unwrap();
            assert_eq!(e.database_stats(b"kv").unwrap(), private);
        }
    }
}
#[test]
fn payload_page_sharing_is_counted_once_and_old_snapshots_keep_exact_extents() {
    for mode in MODES {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"single", DatabaseKind::Kv).unwrap();
        w.put(b"single", b"k", &vec![1; 100_000]).unwrap();
        w.create_database(b"shared", DatabaseKind::Kv).unwrap();
        for n in 0..32u32 {
            w.put(b"shared", &n.to_be_bytes(), &vec![2; 600]).unwrap();
        }
        w.commit().unwrap();
        let old = e.begin_read();
        let a = old.database_stats(b"single").unwrap();
        assert_eq!(a.overflow_bytes, 100_016);
        assert_eq!(a.overflow_pages, 25);
        assert_eq!((a.leaf_pages, a.branch_pages, a.depth), (1, 0, 1));
        let shared = old.database_stats(b"shared").unwrap();
        assert_eq!(shared.overflow_objects, 32);
        assert_eq!(shared.overflow_bytes, 32 * 616);
        assert!(
            shared.overflow_pages < shared.overflow_objects,
            "small overflow objects share storage pages"
        );
        let borrowed = old.get(b"single", b"k").unwrap().unwrap();
        let mut w = e.begin_write().unwrap();
        w.put(b"single", b"k", &vec![3; 200_000]).unwrap();
        w.commit().unwrap();
        let b = e.database_stats(b"single").unwrap();
        assert_eq!(b.overflow_bytes, 200_016);
        assert_eq!(b.overflow_pages, 49);
        assert_eq!(old.database_stats(b"single").unwrap(), a);
        assert_eq!(borrowed, &vec![1; 100_000]);
    }
}
#[test]
fn duplicate_inner_trees_custom_callbacks_private_scopes_and_savepoints() {
    use ulmdb::ordering::{DatabaseComparators, DatabaseOrdering, OrderingSpec};
    for mode in MODES {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let order: ulmdb::ordering::CustomComparator = Arc::new(move |a, b| {
            c.fetch_add(1, Ordering::Relaxed);
            b.cmp(a)
        });
        let callbacks = DatabaseComparators {
            keys: None,
            duplicates: Some(order),
        };
        let mut w = e.begin_write().unwrap();
        w.create_database_ordered(
            b"dups",
            DatabaseKind::Duplicates { width: Some(4) },
            DatabaseOrdering {
                keys: OrderingSpec::Bytewise,
                duplicates: OrderingSpec::Custom,
            },
        )
        .unwrap();
        for n in 0..512u32 {
            w.insert_duplicate_with_ordering(b"dups", b"key", &n.to_be_bytes(), &callbacks)
                .unwrap();
        }
        w.commit().unwrap();
        let old = e.begin_read();
        count.store(0, Ordering::Relaxed);
        let base = old.database_stats(b"dups").unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 0);
        assert_eq!(base.entries, 512);
        assert_eq!(base.depth, 1);
        assert!(base.max_tree_depth >= 2 && base.branch_pages > 0 && base.leaf_pages >= 3);
        assert_eq!(base.overflow_bytes, 0);
        let mut w = e.begin_write().unwrap();
        let h = w.open_database(b"dups").unwrap();
        let a = w.open_database_view_with_ordering(&h, &callbacks).unwrap();
        let b = w.open_database_view_with_ordering(&h, &callbacks).unwrap();
        w.view_insert_duplicate(&a, b"key", &900u32.to_be_bytes())
            .unwrap();
        count.store(0, Ordering::Relaxed);
        let changed = w.view_database_stats(&a).unwrap();
        assert_eq!(changed.entries, 513);
        assert_eq!(w.view_database_stats(&b).unwrap(), base);
        assert_eq!(count.load(Ordering::Relaxed), 0);
        {
            let scope = w.database_read_scope().unwrap();
            let loan = scope.loan_database_view(&a).unwrap();
            assert_eq!(scope.database_stats(&loan).unwrap(), changed);
        }
        {
            let mut child = w.savepoint().unwrap();
            child.view_delete_duplicates(&a, b"key").unwrap();
            assert_eq!(child.view_database_stats(&a).unwrap().entries, 0);
            child.abort().unwrap();
        }
        assert_eq!(w.view_database_stats(&a).unwrap(), changed);
        w.select_database_views(&[&a]).unwrap();
        w.commit().unwrap();
        assert_eq!(e.database_stats(b"dups").unwrap(), changed);
        assert_eq!(old.database_stats(b"dups").unwrap(), base);
    }
}
#[test]
fn radix_nodes_and_payloads_remain_distinct_from_btree_pages() {
    use ulmdb::{Address, Prefix};
    let d = Dir::new();
    let e = Environment::create(&d.0).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
    for n in 0..8u32 {
        w.insert_prefix(
            b"ip",
            Prefix::new(Address::V4(0x0a000000 | (n << 16)), 16).unwrap(),
            b"v",
        )
        .unwrap();
    }
    w.commit().unwrap();
    let s = e.database_stats(b"ip").unwrap();
    assert_eq!(s.entries, 8);
    assert!(s.radix_nodes >= 8);
    assert_eq!(s.radix_bytes, s.radix_nodes * 96);
    assert_eq!(s.overflow_bytes, 8 * 17);
    assert_eq!((s.branch_pages, s.leaf_pages, s.tree_bytes), (0, 0, 0));
    assert_eq!(s.reachable_bytes, s.radix_bytes + s.overflow_bytes);
}
