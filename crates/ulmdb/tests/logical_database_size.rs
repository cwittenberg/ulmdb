#![cfg(target_os = "linux")]
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use ulmdb::chunked::CacheOptions;
use ulmdb::ordering::{DatabaseComparators, DatabaseOrdering, OrderingSpec};
use ulmdb::{DatabaseKind, Environment, LogicalDatabaseSize, PublicationMode, ReadTxn};
use ulmdb_storage::chunk_cache::{ChunkCache, Limits};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "logical-metadata-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn cache() -> CacheOptions {
    CacheOptions::new(
        ChunkCache::new(Limits {
            mapped_bytes: 128 << 10,
            windows: 8,
            chunk_bytes: 16 << 10,
            max_window_bytes: 64 << 10,
        })
        .unwrap(),
    )
}

#[test]
fn cold_admission_releases_distant_overflow_header_windows_within_each_page() {
    for mode in [PublicationMode::Legacy, PublicationMode::Cohort, PublicationMode::Indexed] {
        let d = Dir::new();
        let env = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut write = env.begin_write().unwrap();
        write.create_database(b"kv", DatabaseKind::Kv).unwrap();
        for key in 0..64u32 {
            write.put(b"kv", &key.to_be_bytes(), &vec![key as u8; 65_536]).unwrap();
        }
        write.commit().unwrap();
        drop(env);
        // A single leaf refers to many far-apart headers. The cache can hold
        // the page and one header window, never all64 payload windows.
        let options = cache();
        let read = ReadTxn::open_coordinated_snapshot_with_cache_options(
            &d.0, Default::default(), None, Some(options.clone()),
        ).unwrap();
        let before = options.cache.stats().unwrap().pinned_windows;
        assert_eq!(read.logical_database_size(b"kv").unwrap(), LogicalDatabaseSize {
            entries: 64, key_bytes: 256, value_bytes: 64 * 65_536,
        });
        assert_eq!(options.cache.stats().unwrap().pinned_windows, before);
        assert!(matches!(read.get(b"kv", &0u32.to_be_bytes()), Err(ulmdb::Error::LimitExceeded)));
    }
}
#[test]
fn cached_large_values_duplicate_counts_custom_order_and_private_roots_are_metadata_only() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir::new();
        let env = Environment::create_with_publication(&d.0, mode).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let callbacks = DatabaseComparators {
            keys: Some(Arc::new(move |a, b| {
                c.fetch_add(1, Ordering::Relaxed);
                a.cmp(b)
            })),
            duplicates: None,
        };
        let mut w = env.begin_write().unwrap();
        w.create_database_ordered(
            b"kv",
            DatabaseKind::Kv,
            DatabaseOrdering {
                keys: OrderingSpec::Custom,
                ..Default::default()
            },
        )
        .unwrap();
        w.put_with_ordering(b"kv", b"large", &vec![7; 4 << 20], &callbacks)
            .unwrap();
        for i in 0..64 {
            w.put_with_ordering(
                b"kv",
                format!("k{i:03}").as_bytes(),
                &vec![i as u8; 128],
                &callbacks,
            )
            .unwrap();
        }
        w.put_with_ordering(b"kv", b"", b"", &callbacks).unwrap();
        w.create_database(b"dups", DatabaseKind::Duplicates { width: None })
            .unwrap();
        for key in [b"k".as_slice(), b"xyz"] {
            for i in 0..9u64 {
                w.insert_duplicate(b"dups", key, &i.to_be_bytes()).unwrap();
            }
            w.insert_duplicate(b"dups", key, &vec![0xff; 600]).unwrap();
        }
        w.create_database(b"fixed", DatabaseKind::Duplicates { width: Some(8) })
            .unwrap();
        for i in 0..17u64 {
            w.insert_duplicate(b"fixed", b"f", &i.to_be_bytes())
                .unwrap();
        }
        w.commit().unwrap();
        drop(env);
        let options = cache();
        let ctl = options.cache.clone();
        let read = ReadTxn::open_coordinated_snapshot_with_cache_options(
            &d.0,
            Default::default(),
            None,
            Some(options.clone()),
        )
        .unwrap();
        assert!(read.uses_chunk_cache());
        let before = ctl.stats().unwrap().pinned_windows;
        calls.store(0, Ordering::Relaxed);
        let expected = LogicalDatabaseSize {
            entries: 66,
            key_bytes: 261,
            value_bytes: (4 << 20) + (64 * 128),
        };
        assert_eq!(read.logical_database_size(b"kv").unwrap(), expected);
        let duplicates = LogicalDatabaseSize {
            entries: 20,
            key_bytes: 40,
            value_bytes: 1344,
        };
        assert_eq!(read.logical_database_size(b"dups").unwrap(), duplicates);
        assert_eq!(duplicates.tuple_bytes().unwrap(), 1704);
        assert_eq!(
            read.logical_database_size(b"fixed").unwrap(),
            LogicalDatabaseSize {
                entries: 17,
                key_bytes: 17,
                value_bytes: 136
            }
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(
            ctl.stats().unwrap().pinned_windows,
            before,
            "metadata windows escaped"
        );
        assert!(ctl.stats().unwrap().mapped_bytes <= 128 << 10);
        // Actual ordinary body borrowing remains larger than this authorized
        // window; successful scalar counting therefore cannot be that path.
        assert!(matches!(
            read.get_with_ordering(b"kv", b"large", &callbacks),
            Err(ulmdb::Error::LimitExceeded)
        ));
        drop(read);
        let env = Environment::open_with_cache_options(
            &d.0,
            Default::default(),
            Default::default(),
            None,
            Some(options),
        )
        .unwrap();
        let mut w = env.begin_write().unwrap();
        w.put_with_ordering(b"kv", b"n", b"private", &callbacks)
            .unwrap();
        let changed = LogicalDatabaseSize {
            entries: 67,
            key_bytes: 262,
            value_bytes: expected.value_bytes + 7,
        };
        assert_eq!(w.logical_database_size(b"kv").unwrap(), changed);
        {
            let mut child = w.savepoint().unwrap();
            child
                .put_with_ordering(b"kv", b"n", b"child", &callbacks)
                .unwrap();
            assert_eq!(
                child.logical_database_size(b"kv").unwrap().value_bytes,
                expected.value_bytes + 5
            );
            child.abort().unwrap();
        }
        assert_eq!(w.logical_database_size(b"kv").unwrap(), changed);
        // An independent immutable private scope does not change measurement.
        {
            let _scope = w.database_read_scope().unwrap();
            assert_eq!(w.logical_database_size(b"kv").unwrap(), changed);
        }
        w.abort();
    }
}
