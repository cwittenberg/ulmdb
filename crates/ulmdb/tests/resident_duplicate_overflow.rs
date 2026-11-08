use std::{fs, ops::Bound, path::PathBuf};
use ulmdb::{
    DatabaseKind, Error,
    resident::{Environment, Options, RedoEncoding, RedoFormat},
};
use ulmdb_storage::resident::Config;
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "resident-dups-wide-{:032x}",
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
            tiny_pages: 128,
            max_reserved_bytes: 64 << 20,
            max_metadata_bytes: 64 << 20,
            max_views: 64,
        },
        wal_capacity: 16 << 20,
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
fn actual_64k_variable_fixed_values_range_abort_atomic_commit_and_reopen() {
    for (format, encoding) in modes() {
        let d = Dir::new();
        let o = options();
        let env = Environment::create_with_encoding(&d.0, o, format, encoding).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"var", DatabaseKind::Duplicates { width: None })
            .unwrap();
        w.create_database(b"fixed", DatabaseKind::Duplicates { width: Some(65536) })
            .unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        for i in 0u8..12 {
            w.insert_duplicate(b"var", b"k", &vec![i; 65536]).unwrap();
        }
        w.insert_duplicate(b"var", b"k", &vec![0x42; 65535])
            .unwrap();
        w.insert_duplicate(b"fixed", b"k", &vec![7; 65536]).unwrap();
        w.insert_duplicate(b"fixed", b"k", &vec![9; 65536]).unwrap();
        assert!(matches!(
            w.insert_duplicate(b"fixed", b"k", &vec![8; 65535]),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(w.duplicate_count(b"fixed", b"k").unwrap(), 2);
        assert!(matches!(
            w.insert_duplicate(b"var", b"k", &vec![8; 65537]),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            w.insert_duplicate(b"var", &vec![0; 4081], b"v"),
            Err(Error::InvalidInput(_))
        ));
        w.put(b"kv", b"state", b"A").unwrap();
        assert_eq!(w.commit().unwrap(), 1);
        let old = env.begin_read();
        let first = old.duplicate_first(b"var", b"k").unwrap().unwrap();
        assert_eq!(first.len(), 65536);
        assert!(first.iter().all(|&b| b == 0));
        assert_eq!(old.database_value_limit(b"var").unwrap(), 65536);
        if format != RedoFormat::LiteralV1 {
            env.enable_incremental(Default::default()).unwrap();
        }
        let mut w = env.begin_write().unwrap();
        {
            let mut child = w.savepoint().unwrap();
            assert_eq!(child.delete_duplicates(b"var", b"k").unwrap(), 13);
            child
                .insert_duplicate(b"var", b"k", &vec![15; 65536])
                .unwrap();
            child.delete_duplicates(b"fixed", b"k").unwrap();
        }
        assert_eq!(w.duplicate_count(b"var", b"k").unwrap(), 13);
        for i in 0u8..4 {
            w.delete_duplicate(b"var", b"k", &vec![i; 65536]).unwrap();
        }
        w.put(b"kv", b"state", b"B").unwrap();
        assert_eq!(w.commit().unwrap(), 2);
        let r = env.begin_read();
        assert!(matches!(
            r.duplicate_count(b"var", &vec![0; 4081]),
            Err(Error::InvalidInput(_))
        ));
        assert!(
            r.kv(b"kv")
                .unwrap()
                .cursor(Bound::Included(&vec![0; 4081]), Bound::Unbounded, false)
                .is_err()
        );
        let range = r
            .duplicate_values(
                b"var",
                b"k",
                Bound::Included(&vec![5; 65536]),
                Bound::Excluded(&vec![8; 65536]),
                true,
            )
            .unwrap()
            .map(|v| v.unwrap()[0])
            .collect::<Vec<_>>();
        assert_eq!(range, vec![7, 6, 5]);
        drop(r);
        env.collect().unwrap();
        env.checkpoint().unwrap();
        assert!(first.iter().all(|&b| b == 0));
        drop(env);
        let env = Environment::open_with_encoding(&d.0, o, format, encoding).unwrap();
        let r = env.begin_read();
        assert_eq!(r.sequence(), 2);
        assert_eq!(r.duplicate_count(b"var", b"k").unwrap(), 9);
        assert_eq!(r.duplicates(b"fixed").unwrap().fixed_width(), Some(65536));
        assert!(
            r.contains_duplicate(b"fixed", b"k", &vec![9; 65536])
                .unwrap()
        );
        assert_eq!(r.get(b"kv", b"state").unwrap(), Some(b"B".as_slice()));
        assert!(first.iter().all(|&b| b == 0));
        let bytes = env.wal_bytes();
        let allocations = env.pool_stats().unwrap().allocations;
        let mut w = env.begin_write().unwrap();
        assert!(!w.insert_duplicate(b"fixed", b"k", &vec![9; 65536]).unwrap());
        assert_eq!(w.commit().unwrap(), 2);
        assert_eq!(env.wal_bytes(), bytes);
        assert_eq!(env.pool_stats().unwrap().allocations, allocations);
    }
}
#[test]
fn wide_duplicate_profile_promotion_rolls_back_with_scope() {
    let d = Dir::new();
    let o = options();
    let env = Environment::create(&d.0, o).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"d", DatabaseKind::Duplicates { width: None })
        .unwrap();
    w.insert_duplicate(b"d", b"k", b"small").unwrap();
    w.commit().unwrap();
    let old = env.begin_read();
    let mut w = env.begin_write().unwrap();
    {
        let mut child = w.savepoint().unwrap();
        child.insert_duplicate(b"d", b"k", &vec![1; 65536]).unwrap();
    }
    w.insert_duplicate(b"d", b"k", b"next").unwrap();
    w.commit().unwrap();
    assert_eq!(env.begin_read().database_value_limit(b"d").unwrap(), 4080);
    let mut w = env.begin_write().unwrap();
    w.insert_duplicate(b"d", b"k", &vec![1; 65536]).unwrap();
    w.commit().unwrap();
    assert_eq!(env.begin_read().database_value_limit(b"d").unwrap(), 65536);
    assert_eq!(old.database_value_limit(b"d").unwrap(), 4080);
}
