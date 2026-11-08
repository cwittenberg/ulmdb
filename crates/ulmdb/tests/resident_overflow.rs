use std::{fs, path::PathBuf};
use ulmdb::{
    Address, DatabaseKind, Prefix,
    resident::{Environment, Options, RedoEncoding, RedoFormat},
};
use ulmdb_storage::resident::Config;
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "resident-overflow-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn options() -> Options {
    Options {
        pool: Config {
            kv_pages: 2048,
            tiny_pages: 64,
            max_reserved_bytes: 128 << 20,
            max_metadata_bytes: 64 << 20,
            max_views: 64,
        },
        wal_capacity: 32 << 20,
        graph_read_budget: 16_000_000,
        auto_collect_below: 32,
    }
}
fn modes() -> [(RedoFormat, RedoEncoding); 4] {
    [
        (RedoFormat::LiteralV1, RedoEncoding::Owned),
        (RedoFormat::CompactV2, RedoEncoding::Owned),
        (RedoFormat::FullPostimageV2, RedoEncoding::Owned),
        (RedoFormat::CompactV2, RedoEncoding::Borrowed),
    ]
}
#[test]
fn multi_mib_kv_and_prefix_savepoints_old_borrows_checkpoint_and_reopen() {
    for (format, encoding) in modes() {
        let d = Dir::new();
        let o = options();
        let a = vec![7; (2 << 20) + 37];
        let b = vec![8; (2 << 20) + 39];
        let p = vec![9; (1 << 20) + 17];
        let prefix = Prefix::new(Address::V4(0x0a000000), 8).unwrap();
        let env = Environment::create_with_encoding(&d.0, o, format, encoding).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
        w.put(b"kv", b"key", &a).unwrap();
        w.insert_prefix(b"ip", prefix, &p).unwrap();
        assert_eq!(w.get(b"kv", b"key").unwrap(), Some(a.as_slice()));
        assert_eq!(w.commit().unwrap(), 1);
        let old = env.begin_read();
        let borrow = old.get(b"kv", b"key").unwrap().unwrap();
        assert_eq!(borrow, a);
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"key", &b).unwrap();
        let before = env.overflow_stats().mapped_bytes;
        {
            let mut child = w.savepoint().unwrap();
            child.put(b"kv", b"key", &vec![6; (1 << 20) + 1]).unwrap();
            assert!(env.overflow_stats().mapped_bytes > before);
        }
        assert_eq!(env.overflow_stats().mapped_bytes, before);
        assert_eq!(w.get(b"kv", b"key").unwrap(), Some(b.as_slice()));
        assert_eq!(w.commit().unwrap(), 2);
        env.collect().unwrap();
        env.checkpoint().unwrap();
        assert_eq!(borrow, a);
        drop(env);
        let env = Environment::open_with_encoding(&d.0, o, format, encoding).unwrap();
        let r = env.begin_read();
        assert_eq!(r.sequence(), 2);
        assert_eq!(r.get(b"kv", b"key").unwrap(), Some(b.as_slice()));
        assert_eq!(
            r.longest_match(b"ip", Address::V4(0x0a010203))
                .unwrap()
                .unwrap()
                .1,
            p
        );
        assert_eq!(borrow, a);
        assert!(r.database_stats(b"kv").unwrap().overflow_bytes >= b.len() as u64);
    }
}

#[test]
fn incremental_extent_closure_and_budget_rejection_preserve_parent_progress() {
    for (format, encoding) in modes()
        .into_iter()
        .filter(|(f, _)| *f != RedoFormat::LiteralV1)
    {
        let d = Dir::new();
        let o = options();
        let env = Environment::create_with_encoding(&d.0, o, format, encoding).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
        w.put(b"kv", b"small", b"unchanged").unwrap();
        w.commit().unwrap();
        env.enable_incremental(Default::default()).unwrap();
        #[cfg(feature = "resident-prefix-audit-cache")]
        env.enable_collect_cache(Default::default()).unwrap();
        let value = vec![4; (1 << 20) + 17];
        let prefix = Prefix::new(Address::V4(0), 0).unwrap();
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"big", &value).unwrap();
        w.insert_prefix(b"ip", prefix, &value).unwrap();
        w.commit().unwrap();
        let r = env.begin_read();
        let old = r.get(b"kv", b"big").unwrap().unwrap();
        let graph = r
            .bootstrap_objects(ulmdb::resident::object_graph::Limits {
                reads: 16_000_000,
                objects: 32768,
                edges: 131072,
            })
            .unwrap();
        assert!(graph.edges.iter().any(|e| matches!(
            e.kind,
            ulmdb_core::object_graph::EdgeKind::ExtentSource { .. }
        )));
        drop(graph);
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"big", &vec![5; (1 << 20) + 19]).unwrap();
        w.put(b"kv", b"small", b"B").unwrap();
        w.commit().unwrap();
        env.collect().unwrap();
        env.checkpoint().unwrap();
        assert_eq!(old, value);
        assert!(env.incremental_metrics().unwrap().unwrap().active_objects > 0);
    }
    // A too-small WAL rejects before reserving/mutating pages or profile metadata.
    let d = Dir::new();
    let mut o = options();
    o.wal_capacity = 256 << 10;
    let env = Environment::create(&d.0, o).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    let before = env.pool_stats().unwrap().allocations;
    assert!(matches!(
        w.put(b"kv", b"big", &vec![0; (1 << 20) + 1]),
        Err(ulmdb::Error::LimitExceeded)
    ));
    assert_eq!(env.pool_stats().unwrap().allocations, before);
    assert_eq!(env.overflow_stats().mapped_bytes, 0);
    assert!(matches!(
        w.put(b"kv", b"wide", &vec![0; (16 << 20) + 1]),
        Err(ulmdb::Error::InvalidInput(_))
    ));
    w.put(b"kv", b"small", b"works").unwrap();
    assert_eq!(w.commit().unwrap(), 1);
}

#[test]
fn exact_sixteen_mib_ceiling_is_real_and_reopens_in_every_wal_encoding() {
    let value = vec![0x5a; 16 << 20];
    for (format, encoding) in modes() {
        let d = Dir::new();
        let mut o = options();
        o.pool.kv_pages = 4500;
        o.wal_capacity = 64 << 20;
        let env = Environment::create_with_encoding(&d.0, o, format, encoding).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"ceiling", &value).unwrap();
        assert_eq!(w.commit().unwrap(), 1);
        drop(env);
        let env = Environment::open_with_encoding(&d.0, o, format, encoding).unwrap();
        let r = env.begin_read();
        assert_eq!(r.sequence(), 1);
        assert!(r.get(b"kv", b"ceiling").unwrap().unwrap() == value.as_slice());
        let bytes = env.wal_bytes();
        let allocations = env.pool_stats().unwrap().allocations;
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"ceiling", &value).unwrap();
        assert_eq!(w.commit().unwrap(), 1);
        assert_eq!(env.wal_bytes(), bytes);
        assert_eq!(env.pool_stats().unwrap().allocations, allocations);
    }
}

#[test]
fn aborted_profile_promotion_preserves_small_descriptor_and_old_snapshot() {
    let d = Dir::new();
    let o = options();
    let env = Environment::create(&d.0, o).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"A").unwrap();
    w.commit().unwrap();
    let old = env.begin_read();
    assert_eq!(old.database_value_limit(b"kv").unwrap(), 4080);
    let mut w = env.begin_write().unwrap();
    {
        let mut child = w.savepoint().unwrap();
        child.put(b"kv", b"key", &vec![9; (1 << 20) + 1]).unwrap();
    }
    w.put(b"kv", b"key", b"B").unwrap();
    w.commit().unwrap();
    assert_eq!(env.begin_read().database_value_limit(b"kv").unwrap(), 4080);
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"key", &vec![8; (1 << 20) + 1]).unwrap();
    w.commit().unwrap();
    assert_eq!(
        env.begin_read().database_value_limit(b"kv").unwrap(),
        16 << 20
    );
    assert_eq!(old.database_value_limit(b"kv").unwrap(), 4080);
    assert_eq!(old.get(b"kv", b"key").unwrap(), Some(b"A".as_slice()));
}
