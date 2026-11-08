mod lookup_strategy_support;
use lookup_strategy_support::*;
use std::ops::Bound;
use ulmdb::{
    DatabaseKind, Environment, Error,
    lookup::{Algorithm, Backend, HashLimits, Options, hash_index_bytes},
};

#[test]
fn file_lookup_matches_native_bytes_pointers_ranges_and_input_errors() {
    let d = Dir::new();
    let env = Environment::create(&d.0).unwrap();
    let rows = fixtures();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    for (k, v) in &rows {
        w.put(b"kv", k, v).unwrap();
    }
    w.commit().unwrap();
    let read = env.begin_read();
    let native = read.kv(b"kv").unwrap();
    for options in [Options::default(), hash()] {
        let prepared = read.prepare_lookup(b"kv", options).unwrap();
        assert_eq!(prepared.identity().backend, Backend::File);
        assert_eq!(prepared.identity().sequence, read.sequence());
        assert_eq!(prepared.identity().commit, read.commit_identity());
        assert_eq!(prepared.identity().name, b"kv");
        assert!(!prepared.identity().root.is_null());
        for (key, value) in &rows {
            let actual = prepared.get(key).unwrap().unwrap();
            assert_eq!(actual, value);
            assert_eq!(actual.as_ptr(), native.get(key).unwrap().unwrap().as_ptr());
        }
        for missing in [b"absent".as_slice(), b"\xff\xff"] {
            assert_eq!(prepared.get(missing).unwrap(), None);
        }
        let oversized = vec![1; read.file_manifest().unwrap().page_size.native_key_limit() + 1];
        assert_eq!(
            error_key(prepared.get(&oversized)),
            error_key(native.get(&oversized))
        );
        let bounds = [
            Bound::Unbounded,
            Bound::Included(b"key-00000019".as_slice()),
            Bound::Excluded(b"key-00000100".as_slice()),
        ];
        for lower in bounds {
            for upper in bounds {
                for reverse in [false, true] {
                    let actual = prepared
                        .cursor(lower, upper, reverse)
                        .unwrap()
                        .collect::<ulmdb::Result<Vec<_>>>()
                        .unwrap();
                    let expected = native
                        .cursor(lower, upper, reverse)
                        .unwrap()
                        .collect::<ulmdb::Result<Vec<_>>>()
                        .unwrap();
                    assert_eq!(actual, expected);
                }
            }
        }
        let stats = prepared.stats();
        if matches!(options, Options::Hash(_)) {
            assert_eq!(stats.algorithm, Algorithm::Hash);
            assert!(stats.enumerated);
            assert_eq!(stats.entries, rows.len());
            assert_eq!(stats.index_bytes, hash_index_bytes(rows.len()).unwrap());
            assert_eq!(
                stats.total_value_bytes,
                rows.iter().map(|r| r.1.len()).sum()
            );
            assert_eq!(
                stats.referenced_key_bytes,
                rows.iter().map(|r| r.0.len()).sum()
            );
            assert!(stats.capacity >= rows.len() * 2);
        } else {
            assert!(!stats.enumerated);
            assert_eq!(stats.index_bytes, 0);
            assert_eq!(stats.algorithm, Algorithm::Ordered);
        }
    }
}

#[test]
fn limits_reject_whole_index_and_exact_bound_succeeds() {
    let d = Dir::new();
    let env = Environment::create(&d.0).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"empty", DatabaseKind::Kv).unwrap();
    for (k, v) in [
        (b"a".as_slice(), b"".as_slice()),
        (b"b", b"abc"),
        (b"c", &[7; 1024]),
    ] {
        w.put(b"kv", k, v).unwrap();
    }
    w.commit().unwrap();
    let read = env.begin_read();
    let exact = HashLimits {
        max_entries: 3,
        max_index_bytes: hash_index_bytes(3).unwrap(),
        max_value_bytes: 1024,
        max_total_value_bytes: 1027,
    };
    for denied in [
        HashLimits {
            max_entries: 2,
            ..exact
        },
        HashLimits {
            max_index_bytes: exact.max_index_bytes - 1,
            ..exact
        },
        HashLimits {
            max_value_bytes: 1023,
            ..exact
        },
        HashLimits {
            max_total_value_bytes: 1026,
            ..exact
        },
    ] {
        assert!(matches!(
            read.prepare_lookup(b"kv", Options::Hash(denied)),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(read.get(b"kv", b"b").unwrap(), Some(b"abc".as_slice()));
    }
    let lookup = read.prepare_lookup(b"kv", Options::Hash(exact)).unwrap();
    assert_eq!(lookup.stats().index_bytes, exact.max_index_bytes);
    assert_eq!(lookup.get(b"c").unwrap(), Some([7; 1024].as_slice()));
    let empty = read
        .prepare_lookup(
            b"empty",
            Options::Hash(HashLimits {
                max_entries: 0,
                max_index_bytes: 0,
                max_value_bytes: 0,
                max_total_value_bytes: 0,
            }),
        )
        .unwrap();
    assert_eq!(empty.stats().index_bytes, 0);
    assert_eq!(empty.stats().entries, 0);
    assert_eq!(empty.get(b"").unwrap(), None);
    assert!(hash_index_bytes(usize::MAX).is_err());
}

#[test]
fn old_file_lookup_survives_database_recreation_reopen_and_environment_drop() {
    let d = Dir::new();
    let env = Environment::create(&d.0).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"old").unwrap();
    w.commit().unwrap();
    let old = env.begin_read();
    let lookup = old.prepare_lookup(b"kv", hash()).unwrap();
    let old_identity = lookup.identity();
    let mut w = env.begin_write().unwrap();
    w.drop_database(b"kv").unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"new").unwrap();
    w.commit().unwrap();
    let new = env.begin_read();
    let new_lookup = new.prepare_lookup(b"kv", hash()).unwrap();
    assert_ne!(new_lookup.identity().database_id, old_identity.database_id);
    assert_ne!(new_lookup.identity().commit, old_identity.commit);
    assert_eq!(lookup.get(b"key").unwrap(), Some(b"old".as_slice()));
    assert_eq!(new_lookup.get(b"key").unwrap(), Some(b"new".as_slice()));
    drop(env);
    let reopened = Environment::open(&d.0).unwrap();
    let reopened_read = reopened.begin_read();
    let reopened_lookup = reopened_read.prepare_lookup(b"kv", hash()).unwrap();
    assert_eq!(
        reopened_lookup.identity().commit,
        new_lookup.identity().commit
    );
    assert_eq!(
        reopened_lookup.get(b"key").unwrap(),
        Some(b"new".as_slice())
    );
    drop(reopened);
    assert_eq!(lookup.get(b"key").unwrap(), Some(b"old".as_slice()));
    assert_eq!(new_lookup.get(b"key").unwrap(), Some(b"new".as_slice()));
}

#[test]
fn prepared_file_lookup_shares_across_reader_threads_without_blocking_writes() {
    let d = Dir::new();
    let env = Environment::create(&d.0).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"old").unwrap();
    w.commit().unwrap();
    let read = env.begin_read();
    let lookup = read.prepare_lookup(b"kv", hash()).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let lookup = &lookup;
            scope.spawn(move || {
                for _ in 0..10_000 {
                    assert_eq!(lookup.get(b"key").unwrap(), Some(b"old".as_slice()));
                }
            });
        }
        for i in 0..8u64 {
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"key", &i.to_le_bytes()).unwrap();
            w.commit().unwrap();
        }
    });
    assert_eq!(lookup.get(b"key").unwrap(), Some(b"old".as_slice()));
    assert_eq!(
        env.begin_read().get(b"kv", b"key").unwrap(),
        Some(7u64.to_le_bytes().as_slice())
    );
}

#[test]
fn page_geometry_key_limit_is_taken_from_pinned_source() {
    let d = Dir::new();
    let page = ulmdb::PageSize::new(16_384).unwrap();
    let env =
        Environment::create_with_page_size(&d.0, ulmdb::PublicationMode::Legacy, page).unwrap();
    let key = vec![7; 5000];
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", &key, b"value").unwrap();
    w.commit().unwrap();
    let r = env.begin_read();
    let lookup = r.prepare_lookup(b"kv", hash()).unwrap();
    assert_eq!(lookup.get(&key).unwrap(), Some(b"value".as_slice()));
    let bad = vec![0; page.native_key_limit() + 1];
    assert_eq!(
        error_key(lookup.get(&bad)),
        error_key(r.kv(b"kv").unwrap().get(&bad))
    );
}
