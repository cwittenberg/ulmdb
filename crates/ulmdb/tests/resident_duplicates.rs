//! Real sorted/fixed duplicate trees over Resident pool/WAL/checkpoint owners.
use std::{fs, ops::Bound, path::PathBuf};
use ulmdb::{
    Address, DatabaseKind, Error, Prefix,
    resident::{Environment, IncrementalLimits, Options, RedoEncoding, RedoFormat},
};
use ulmdb_storage::resident::Config;
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "resident-dups-{:032x}",
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
            kv_pages: 128,
            tiny_pages: 96,
            max_reserved_bytes: 8 << 20,
            max_metadata_bytes: 32 << 20,
            max_views: 128,
        },
        wal_capacity: 512 << 10,
        graph_read_budget: 4_000_000,
        auto_collect_below: 16,
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
fn prefix() -> Prefix {
    Prefix::new(Address::V4(0x0a000000), 8).unwrap()
}
fn values(r: &ulmdb::resident::ReadTxn) -> Vec<Vec<u8>> {
    r.duplicate_values(b"dups", b"k", Bound::Unbounded, Bound::Unbounded, false)
        .unwrap()
        .map(|v| v.unwrap().to_vec())
        .collect()
}
#[test]
fn inline_tree_demotion_atomic_kv_prefix_savepoint_and_old_borrow_recovery() {
    for (format, encoding) in modes() {
        let d = Dir::new();
        let o = options();
        let env = Environment::create_with_encoding(&d.0, o, format, encoding).unwrap();
        let mut w = env.begin_write().unwrap();
        for (name, kind) in [
            (b"dups".as_slice(), DatabaseKind::Duplicates { width: None }),
            (b"fixed", DatabaseKind::Duplicates { width: Some(4) }),
            (b"kv", DatabaseKind::Kv),
            (b"ip", DatabaseKind::Ipv4),
        ] {
            w.create_database(name, kind).unwrap();
        }
        for i in (0u64..64).rev() {
            assert!(w.insert_duplicate(b"dups", b"k", &i.to_be_bytes()).unwrap());
        }
        assert!(
            !w.insert_duplicate(b"dups", b"k", &0u64.to_be_bytes())
                .unwrap()
        );
        assert_eq!(w.duplicate_count(b"dups", b"k").unwrap(), 64);
        for i in 0u32..3 {
            w.insert_duplicate(b"fixed", b"f", &i.to_be_bytes())
                .unwrap();
        }
        w.put(b"kv", b"state", b"A").unwrap();
        w.insert_prefix(b"ip", prefix(), b"A").unwrap();
        assert_eq!(w.commit().unwrap(), 1);
        let old = env.begin_read();
        let old_dups = old.duplicates(b"dups").unwrap();
        let first = old_dups.first(b"k").unwrap().unwrap();
        let mut held = old_dups
            .values(b"k", Bound::Unbounded, Bound::Unbounded, false)
            .unwrap();
        assert_eq!(first, &0u64.to_be_bytes());
        assert_eq!(old.database_stats(b"dups").unwrap().entries, 64);
        assert!(
            old.database_stats(b"dups").unwrap().leaf_pages >= 2,
            "promoted bucket has a separate value tree"
        );
        let mut w = env.begin_write().unwrap();
        assert!(
            w.insert_duplicate(b"dups", b"k", &64u64.to_be_bytes())
                .unwrap()
        );
        {
            let mut child = w.savepoint().unwrap();
            assert!(
                child
                    .delete_duplicate(b"dups", b"k", &0u64.to_be_bytes())
                    .unwrap()
            );
            child
                .insert_duplicate(b"dups", b"k", &70u64.to_be_bytes())
                .unwrap();
            assert_eq!(child.delete_duplicates(b"dups", b"k").unwrap(), 65);
            child.drop_database(b"fixed").unwrap();
            child
                .create_database(b"ghost", DatabaseKind::Duplicates { width: None })
                .unwrap();
            child.insert_duplicate(b"ghost", b"g", b"discard").unwrap();
            {
                let mut nested = child.savepoint().unwrap();
                nested.insert_duplicate(b"dups", b"k", b"discard").unwrap();
                nested.release().unwrap();
            }
            child.abort().unwrap();
        }
        assert_eq!(w.duplicate_count(b"dups", b"k").unwrap(), 65);
        assert!(matches!(
            w.insert_duplicate(b"fixed", b"f", b"bad"),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(
            w.duplicate_count(b"fixed", b"f").unwrap(),
            3,
            "width rejection must precede mutation/poison"
        );
        for i in 0u64..61 {
            assert!(w.delete_duplicate(b"dups", b"k", &i.to_be_bytes()).unwrap());
        }
        assert!(
            !w.delete_duplicate(b"dups", b"k", &0u64.to_be_bytes())
                .unwrap()
        );
        w.put(b"kv", b"state", b"B").unwrap();
        w.insert_prefix(b"ip", prefix(), b"B").unwrap();
        assert_eq!(w.commit().unwrap(), 2);
        assert_eq!(first, &0u64.to_be_bytes());
        assert_eq!(held.next().unwrap().unwrap(), &0u64.to_be_bytes());
        let r = env.begin_read();
        assert_eq!(
            values(&r),
            (61u64..65)
                .map(|i| i.to_be_bytes().to_vec())
                .collect::<Vec<_>>()
        );
        let reverse = r
            .duplicate_values(
                b"dups",
                b"k",
                Bound::Included(&62u64.to_be_bytes()),
                Bound::Excluded(&64u64.to_be_bytes()),
                true,
            )
            .unwrap()
            .map(|v| v.unwrap().to_vec())
            .collect::<Vec<_>>();
        assert_eq!(
            reverse,
            vec![63u64.to_be_bytes().to_vec(), 62u64.to_be_bytes().to_vec()]
        );
        assert_eq!(r.get(b"kv", b"state").unwrap(), Some(b"B".as_slice()));
        assert_eq!(
            r.longest_match(b"ip", Address::V4(0x0a010203))
                .unwrap()
                .unwrap()
                .1,
            b"B"
        );
        assert_eq!(r.database_stats(b"dups").unwrap().entries, 4);
        assert_eq!(
            r.database_stats(b"dups").unwrap().leaf_pages,
            1,
            "demoted values live inline in the parent page"
        );
        env.collect().unwrap();
        env.checkpoint().unwrap();
        assert_eq!(first, &0u64.to_be_bytes());
        assert_eq!(held.count(), 63);
        drop(old_dups);
        drop(r);
        drop(env);
        let env = Environment::open_with_encoding(&d.0, o, format, encoding).unwrap();
        let r = env.begin_read();
        assert_eq!(r.sequence(), 2);
        assert_eq!(values(&r).len(), 4);
        assert_eq!(
            first,
            &0u64.to_be_bytes(),
            "old pool borrow survives a fresh physical owner"
        );
        assert_eq!(old.duplicate_count(b"dups", b"k").unwrap(), 64);
        assert_eq!(r.duplicates(b"fixed").unwrap().fixed_width(), Some(4));
        assert!(matches!(
            r.duplicates(b"ghost"),
            Err(Error::MissingDatabase)
        ));
    }
}
#[test]
fn exact_size_zero_width_and_binary_pair_cursors_preserve_admission_errors() {
    for (format, encoding) in modes() {
        let d = Dir::new();
        let o = options();
        let env = Environment::create_with_encoding(&d.0, o, format, encoding).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"dups", DatabaseKind::Duplicates { width: None })
            .unwrap();
        w.create_database(b"zero", DatabaseKind::Duplicates { width: Some(0) })
            .unwrap();
        assert!(matches!(
            w.create_database(b"wide", DatabaseKind::Duplicates { width: Some(65537) }),
            Err(Error::InvalidInput(_))
        ));
        for (key, value) in [(vec![1; 4081], vec![1]), (vec![1], vec![1; 65537])] {
            assert!(matches!(
                w.insert_duplicate(b"dups", &key, &value),
                Err(Error::InvalidInput(_))
            ));
        }
        assert!(w.insert_duplicate(b"zero", b"k", b"").unwrap());
        assert!(!w.insert_duplicate(b"zero", b"k", b"").unwrap());
        assert!(matches!(
            w.insert_duplicate(b"zero", b"k", b"x"),
            Err(Error::InvalidInput(_))
        ));
        w.insert_duplicate(b"dups", b"\0", b"\0").unwrap();
        w.insert_duplicate(b"dups", b"\0", b"z").unwrap();
        w.insert_duplicate(b"dups", b"z", b"last").unwrap();
        let large = vec![7; 4080];
        w.insert_duplicate(b"dups", b"large", &large).unwrap();
        w.insert_duplicate(b"dups", &large, b"large-key").unwrap();
        assert_eq!(w.commit().unwrap(), 1);
        let r = env.begin_read();
        assert_eq!(
            r.duplicate_seek_pair(b"dups", b"large", &large).unwrap(),
            Some(large.as_slice())
        );
        assert_eq!(
            r.duplicate_first(b"zero", b"k").unwrap(),
            Some(b"".as_slice())
        );
        assert!(r.contains_duplicate(b"dups", b"\0", b"z").unwrap());
        let mut pairs = r
            .duplicate_pairs(
                b"dups",
                Bound::Included(b"\0"),
                Bound::Included(b"z"),
                false,
            )
            .unwrap();
        assert_eq!(
            pairs.next().unwrap().unwrap(),
            (b"\0".as_slice(), b"\0".as_slice())
        );
        assert_eq!(pairs.next_key().unwrap().unwrap().0, large.as_slice());
        assert_eq!(pairs.next_key().unwrap().unwrap().0, b"large");
        env.collect().unwrap();
        env.checkpoint().unwrap();
        drop(pairs);
        drop(r);
        drop(env);
        let env = Environment::open_with_encoding(&d.0, o, format, encoding).unwrap();
        assert_eq!(
            env.begin_read().duplicate_first(b"dups", b"large").unwrap(),
            Some(large.as_slice())
        );
    }
}
#[test]
fn incremental_certificates_trace_child_trees_through_writes_and_retirement() {
    for (format, encoding) in modes()
        .into_iter()
        .filter(|(f, _)| *f != RedoFormat::LiteralV1)
    {
        let d = Dir::new();
        let o = options();
        let env = Environment::create_with_encoding(&d.0, o, format, encoding).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"dups", DatabaseKind::Duplicates { width: Some(4) })
            .unwrap();
        w.create_database(b"long", DatabaseKind::Duplicates { width: None })
            .unwrap();
        w.insert_duplicate(b"long", b"large-value", &vec![9; 4080])
            .unwrap();
        w.insert_duplicate(b"long", &vec![6; 4080], b"large-key")
            .unwrap();
        for i in 0u32..24 {
            w.insert_duplicate(b"dups", b"k", &i.to_be_bytes()).unwrap();
        }
        w.commit().unwrap();
        let old = env.begin_read();
        let borrowed = old.duplicate_first(b"dups", b"k").unwrap().unwrap();
        let graph = old
            .bootstrap_objects(ulmdb::resident::object_graph::Limits {
                reads: 4_000_000,
                objects: 32768,
                edges: 131072,
            })
            .unwrap();
        assert!(graph.edges.iter().any(|e| matches!(
            e.kind,
            ulmdb_core::object_graph::EdgeKind::DuplicateRoot { .. }
        )));
        drop(graph);
        env.enable_incremental(IncrementalLimits::default())
            .unwrap();
        #[cfg(feature = "resident-prefix-audit-cache")]
        env.enable_collect_cache(Default::default()).unwrap();
        for i in 24u32..48 {
            let mut w = env.begin_write().unwrap();
            w.insert_duplicate(b"dups", b"k", &i.to_be_bytes()).unwrap();
            if i % 4 == 0 {
                w.insert_duplicate(b"long", b"large-value", &vec![i as u8; 4080])
                    .unwrap();
            }
            if i % 2 == 0 {
                w.delete_duplicate(b"dups", b"k", &(i - 24).to_be_bytes())
                    .unwrap();
            }
            w.commit().unwrap();
            if i % 4 == 0 {
                env.collect().unwrap();
            }
            assert_eq!(borrowed, &0u32.to_be_bytes());
        }
        assert!(env.incremental_metrics().unwrap().unwrap().active_objects > 0);
        env.checkpoint().unwrap();
        assert_eq!(borrowed, &0u32.to_be_bytes());
        drop(old);
        drop(env);
        let env = Environment::open_with_encoding(&d.0, o, format, encoding).unwrap();
        let r = env.begin_read();
        assert_eq!(r.duplicate_count(b"dups", b"k").unwrap(), 36);
        assert!(
            r.contains_duplicate(b"dups", b"k", &47u32.to_be_bytes())
                .unwrap()
        );
        assert_eq!(r.duplicate_count(b"long", b"large-value").unwrap(), 7);
    }
}
