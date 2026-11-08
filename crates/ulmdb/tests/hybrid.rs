//! Real facade transactions/recovery, separate from standalone storage tests.
use std::{collections::BTreeMap, ops::Bound, path::PathBuf, sync::Arc};
use ulmdb::{
    Address, DatabaseKind, Prefix,
    hybrid::{Environment, Options, RedoMode},
};
fn paths() -> Vec<PathBuf> {
    [std::env::temp_dir(), PathBuf::from("/dev/shm")]
        .into_iter()
        .filter(|p| p.is_dir())
        .map(|p| {
            p.join(format!(
                "ulmdb-hybrid-env-{:032x}",
                ulmdb_storage::new_identity().unwrap()
            ))
        })
        .collect()
}
fn options(mode: RedoMode) -> Options {
    let mut o = Options::default();
    o.chunk.capacity = 8 * 1024 * 1024;
    o.chunk.reserve_quantum = 65536;
    o.chunk.max_burned_bytes = 2 * 1024 * 1024;
    o.max_image_bytes = 32 * 1024 * 1024;
    o.redo = mode;
    o
}
fn prefixes() -> (Prefix, Prefix) {
    (
        Prefix::new(Address::V4(0x0a00_0000), 8).unwrap(),
        Prefix::new(Address::V6(0x20010db8000000000000000000000000), 32).unwrap(),
    )
}
fn initialize(env: &Environment) {
    let mut w = env.begin_write().unwrap();
    for (n, k) in [
        (b"kv".as_slice(), DatabaseKind::Kv),
        (b"v4", DatabaseKind::Ipv4),
        (b"v6", DatabaseKind::Ipv6),
    ] {
        w.create_database(n, k).unwrap();
    }
    let (v4, v6) = prefixes();
    w.put(b"kv", b"state", &0u64.to_le_bytes()).unwrap();
    w.insert_prefix(b"v4", v4, &0u64.to_le_bytes()).unwrap();
    w.insert_prefix(b"v6", v6, &0u64.to_le_bytes()).unwrap();
    assert_eq!(w.commit().unwrap(), 1);
}
#[test]
fn atomic_cross_index_escaped_reader_reopen_and_continue() {
    for path in paths() {
        for mode in [RedoMode::Compact, RedoMode::FullPostimage] {
            let path = path.with_extension(format!("{mode:?}"));
            let env = Environment::create(&path, options(mode)).unwrap();
            initialize(&env);
            let old = env.begin_read();
            let held = old.get(b"kv", b"state").unwrap().unwrap();
            let (v4, v6) = prefixes();
            for n in 1..=12u64 {
                let mut w = env.begin_write().unwrap();
                w.put(b"kv", b"state", &n.to_le_bytes()).unwrap();
                w.insert_prefix(b"v4", v4, &n.to_le_bytes()).unwrap();
                w.insert_prefix(b"v6", v6, &n.to_le_bytes()).unwrap();
                assert_eq!(w.commit().unwrap(), n + 1);
            }
            assert_eq!(held, 0u64.to_le_bytes());
            assert!(Environment::open(&path, options(mode)).is_err());
            drop(env);
            let env = Environment::open(&path, options(mode)).unwrap();
            assert_eq!(held, 0u64.to_le_bytes());
            assert_eq!(
                old.prefix_index(b"v4")
                    .unwrap()
                    .exact(v4)
                    .unwrap()
                    .unwrap()
                    .1,
                held
            );
            let r = env.begin_read();
            let now = r.get(b"kv", b"state").unwrap().unwrap();
            assert_eq!(now, 12u64.to_le_bytes());
            assert_eq!(
                r.prefix_index(b"v4").unwrap().exact(v4).unwrap().unwrap().1,
                now
            );
            assert_eq!(
                r.prefix_index(b"v6").unwrap().exact(v6).unwrap().unwrap().1,
                now
            );
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"state", b"continued").unwrap();
            w.commit().unwrap();
            drop(env);
            let env = Environment::open(&path, options(mode)).unwrap();
            assert_eq!(
                env.begin_read().get(b"kv", b"state").unwrap(),
                Some(b"continued".as_slice())
            );
            drop(env);
            drop(r);
            drop(old);
            std::fs::remove_dir_all(&path).unwrap();
        }
    }
}
#[test]
fn randomized_updates_savepoints_noop_rotation_and_ordered_reads() {
    for path in paths() {
        let env = Environment::create(&path, options(RedoMode::Compact)).unwrap();
        initialize(&env);
        let mut expected = BTreeMap::new();
        expected.insert(b"state".to_vec(), 0u64.to_le_bytes().to_vec());
        let mut seed = 7u64;
        for batch in 0..24 {
            if batch == 12 {
                env.rotate_chunk().unwrap();
                env.rotate_chunk().unwrap();
            }
            let mut w = env.begin_write().unwrap();
            for _ in 0..32 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let key = (seed % 256).to_be_bytes().to_vec();
                if seed & 7 == 0 {
                    assert_eq!(
                        w.delete(b"kv", &key).unwrap(),
                        expected.remove(&key).is_some()
                    );
                } else {
                    let value = seed.to_le_bytes().to_vec();
                    w.put(b"kv", &key, &value).unwrap();
                    expected.insert(key, value);
                }
            }
            {
                let mut save = w.savepoint().unwrap();
                save.put(b"kv", b"SHOULD_ABORT", b"bad").unwrap();
                {
                    let mut inner = save.savepoint().unwrap();
                    inner.drop_database(b"v6").unwrap();
                    inner.release().unwrap();
                }
                save.rollback().unwrap();
                save.put(b"kv", b"SHOULD_ABORT", b"bad2").unwrap();
            }
            assert_eq!(w.get(b"kv", b"SHOULD_ABORT").unwrap(), None);
            w.commit().unwrap();
        }
        let sequence = env.sequence();
        env.begin_write().unwrap().commit().unwrap();
        assert_eq!(sequence, env.sequence());
        let old = env.begin_read();
        let kv = old.kv(b"kv").unwrap();
        let rows: BTreeMap<_, _> = kv
            .cursor(Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()
            .map(|r| {
                let (k, v) = r.unwrap();
                (k.to_vec(), v.to_vec())
            })
            .collect();
        assert_eq!(rows, expected);
        drop(env);
        let env = Environment::open(&path, options(RedoMode::Compact)).unwrap();
        for (key, value) in expected {
            assert_eq!(
                env.begin_read().get(b"kv", &key).unwrap(),
                Some(value.as_slice())
            );
        }
        drop(env);
        drop(old);
        std::fs::remove_dir_all(path).unwrap();
    }
}
#[test]
fn racing_readers_see_complete_three_index_snapshots() {
    for path in paths() {
        let env = Arc::new(Environment::create(&path, options(RedoMode::Compact)).unwrap());
        initialize(&env);
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let (v4, v6) = prefixes();
        std::thread::scope(|scope| {
            for _ in 0..2 {
                let env = Arc::clone(&env);
                let running = Arc::clone(&running);
                scope.spawn(move || {
                    while running.load(std::sync::atomic::Ordering::Acquire) {
                        let r = env.begin_read();
                        let kv = r.get(b"kv", b"state").unwrap().unwrap();
                        assert_eq!(
                            r.prefix_index(b"v4").unwrap().exact(v4).unwrap().unwrap().1,
                            kv
                        );
                        assert_eq!(
                            r.prefix_index(b"v6").unwrap().exact(v6).unwrap().unwrap().1,
                            kv
                        );
                    }
                });
            }
            for n in 1..=16u64 {
                let mut w = env.begin_write().unwrap();
                w.put(b"kv", b"state", &n.to_le_bytes()).unwrap();
                w.insert_prefix(b"v4", v4, &n.to_le_bytes()).unwrap();
                w.insert_prefix(b"v6", v6, &n.to_le_bytes()).unwrap();
                w.commit().unwrap();
            }
            running.store(false, std::sync::atomic::Ordering::Release);
        });
        drop(env);
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[test]
fn transient_catalog_noop_and_empty_values_remain_distinct() {
    for path in paths() {
        let env = Environment::create(&path, options(RedoMode::Compact)).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"transient", DatabaseKind::Kv).unwrap();
        w.drop_database(b"transient").unwrap();
        assert_eq!(w.commit().unwrap(), 0);
        initialize(&env);
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"empty", b"").unwrap();
        assert_eq!(w.get(b"kv", b"empty").unwrap(), Some(b"".as_slice()));
        assert!(w.delete(b"kv", b"empty").unwrap());
        assert!(!w.delete(b"kv", b"empty").unwrap());
        w.commit().unwrap();
        drop(env);
        let env = Environment::open(&path, options(RedoMode::Compact)).unwrap();
        assert_eq!(env.begin_read().database_count(), 3);
        assert_eq!(env.begin_read().get(b"kv", b"empty").unwrap(), None);
        drop(env);
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[test]
fn compact_overflow_replay_and_refused_commit_preserve_source_authority() {
    for path in paths() {
        let env = Environment::create(&path, options(RedoMode::Compact)).unwrap();
        initialize(&env);
        let mut value = vec![17u8; 262144];
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"large", &value).unwrap();
        w.commit().unwrap();
        let old = env.begin_read();
        let held = old.get(b"kv", b"large").unwrap().unwrap();
        let before = env.wal_bytes().unwrap();
        for n in 0..4usize {
            value[n * 8192..n * 8192 + 8].copy_from_slice(&(n as u64).to_le_bytes());
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"large", &value).unwrap();
            w.commit().unwrap();
        }
        // Mechanism bound: four sparse replacements must not log four literals.
        assert!(env.wal_bytes().unwrap() - before < 65536);
        assert!(held.iter().all(|b| *b == 17));
        drop(env);
        let env = Environment::open(&path, options(RedoMode::Compact)).unwrap();
        assert_eq!(
            env.begin_read().get(b"kv", b"large").unwrap(),
            Some(value.as_slice())
        );
        drop(env);
        drop(old);
        std::fs::remove_dir_all(&path).unwrap();
        let mut limited = options(RedoMode::Compact);
        limited.max_wal_bytes = 220000;
        let env = Environment::create(&path, limited).unwrap();
        initialize(&env);
        let sequence = env.sequence();
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"state", &vec![9; 65536]).unwrap();
        assert!(matches!(w.commit(), Err(ulmdb::Error::LimitExceeded)));
        assert_eq!(env.sequence(), sequence);
        assert_eq!(
            env.begin_read().get(b"kv", b"state").unwrap(),
            Some(0u64.to_le_bytes().as_slice())
        );
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"state", b"retry").unwrap();
        w.commit().unwrap();
        drop(env);
        let env = Environment::open(&path, limited).unwrap();
        assert_eq!(
            env.begin_read().get(b"kv", b"state").unwrap(),
            Some(b"retry".as_slice())
        );
        drop(env);
        std::fs::remove_dir_all(path).unwrap();
    }
}
