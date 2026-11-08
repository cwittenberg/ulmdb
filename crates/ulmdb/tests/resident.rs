//! Actual resident transactions and full retained-WAL recovery, not timing tests.
use std::{ops::Bound, path::PathBuf};
use ulmdb::{
    Address, DatabaseKind, Error, Prefix,
    resident::{Environment, Options},
};
use ulmdb_storage::resident::Config;
struct Directory(PathBuf);
impl Directory {
    fn new(base: &std::path::Path) -> Self {
        Self(base.join(format!(
            "ulmdb-resident-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn roots() -> Vec<PathBuf> {
    [std::env::temp_dir(), PathBuf::from("/dev/shm")]
        .into_iter()
        .filter(|p| p.is_dir())
        .collect()
}
fn options(pages: usize) -> Options {
    Options {
        pool: Config {
            kv_pages: pages,
            tiny_pages: 32,
            max_reserved_bytes: 64 << 20,
            max_metadata_bytes: 64 << 20,
            max_views: 64,
        },
        wal_capacity: 8 << 20,
        graph_read_budget: 2_000_000,
        auto_collect_below: pages.min(8),
    }
}
fn prefixes() -> (Prefix, Prefix) {
    (
        Prefix::new(Address::V4(0x0a00_0000), 8).unwrap(),
        Prefix::new(Address::V6(0x2001_0db8u128 << 96), 32).unwrap(),
    )
}
fn initialize(env: &Environment) {
    let mut write = env.begin_write().unwrap();
    for (name, kind) in [
        (b"kv".as_slice(), DatabaseKind::Kv),
        (b"v4", DatabaseKind::Ipv4),
        (b"v6", DatabaseKind::Ipv6),
    ] {
        write.create_database(name, kind).unwrap();
    }
    write.put(b"kv", b"state", &0u64.to_le_bytes()).unwrap();
    let (v4, v6) = prefixes();
    write.insert_prefix(b"v4", v4, &0u64.to_le_bytes()).unwrap();
    write.insert_prefix(b"v6", v6, &0u64.to_le_bytes()).unwrap();
    assert_eq!(write.commit().unwrap(), 1);
}
#[test]
fn durable_cross_index_updates_escaped_borrow_and_fresh_owner_reopen() {
    for base in roots() {
        let path = Directory::new(&base);
        let o = options(64);
        let env = Environment::create(&path.0, o).unwrap();
        initialize(&env);
        let old = env.begin_read();
        let escaped = old.get(b"kv", b"state").unwrap().unwrap();
        let old_owner = env.pool_stats().unwrap().physical_incarnation;
        let (v4, v6) = prefixes();
        for n in 1..=12u64 {
            let mut write = env.begin_write().unwrap();
            write.put(b"kv", b"state", &n.to_le_bytes()).unwrap();
            write.insert_prefix(b"v4", v4, &n.to_le_bytes()).unwrap();
            write.insert_prefix(b"v6", v6, &n.to_le_bytes()).unwrap();
            assert_eq!(write.commit().unwrap(), n + 1);
        }
        let selected_lsn = env.physical_lsn();
        assert_eq!(escaped, 0u64.to_le_bytes());
        drop(env);
        let reopened = Environment::open(&path.0, o).unwrap();
        assert_ne!(
            reopened.pool_stats().unwrap().physical_incarnation,
            old_owner
        );
        assert_eq!(
            (reopened.sequence(), reopened.physical_lsn()),
            (13, selected_lsn)
        );
        let fresh = reopened.begin_read();
        assert_eq!(
            fresh.get(b"kv", b"state").unwrap().unwrap(),
            12u64.to_le_bytes()
        );
        for (name, prefix) in [(b"v4".as_slice(), v4), (b"v6", v6)] {
            let reader = fresh.prefix_index(name).unwrap();
            assert_eq!(
                reader.exact(prefix).unwrap().unwrap().1,
                12u64.to_le_bytes()
            );
            assert_eq!(reader.covering(prefix).unwrap().unwrap().0, prefix);
            assert_eq!(reader.subnets(prefix).unwrap().count(), 1);
        }
        assert_eq!(
            old.longest_match(b"v4", v4.network()).unwrap().unwrap().1,
            0u64.to_le_bytes()
        );
        assert_eq!(escaped, 0u64.to_le_bytes());
        assert!(fresh.longest_match(b"v4", v6.network()).is_err());
    }
}
#[test]
fn actual_multileaf_updates_wrap_fixed_maps_with_charged_collection_and_old_reader() {
    let path = Directory::new(&std::env::temp_dir());
    let o = options(64);
    let env = Environment::create(&path.0, o).unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    for key in 0..256u32 {
        write
            .put(b"kv", &key.to_be_bytes(), &0u64.to_le_bytes())
            .unwrap();
    }
    write.commit().unwrap();
    let old = env.begin_read();
    let escaped = old.get(b"kv", &0u32.to_be_bytes()).unwrap().unwrap();
    let mut expected = [0u64; 256];
    for round in 1..=96u64 {
        let mut write = env.begin_write().unwrap();
        for step in 0..16u32 {
            let key = ((round as u32 * 17) + step) % 256;
            write
                .put(b"kv", &key.to_be_bytes(), &round.to_le_bytes())
                .unwrap();
            expected[key as usize] = round;
        }
        write.commit().unwrap();
    }
    let stats = env.pool_stats().unwrap();
    assert_eq!(stats.maps_created, o.pool.kv_pages + o.pool.tiny_pages);
    assert!(stats.reused_pages > 100);
    assert!(env.physical_lsn() > env.sequence());
    let read = env.begin_read();
    let rows = read
        .kv(b"kv")
        .unwrap()
        .cursor(Bound::Unbounded, Bound::Unbounded, false)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows.len(), 256);
    for (key, value) in rows {
        let key = u32::from_be_bytes(key.try_into().unwrap());
        assert_eq!(value, expected[key as usize].to_le_bytes());
    }
    assert_eq!(escaped, 0u64.to_le_bytes());
    drop(read);
    let sequence = env.sequence();
    drop(env);
    let reopened = Environment::open(&path.0, o).unwrap();
    assert_eq!(reopened.sequence(), sequence);
    for key in 0..256u32 {
        assert_eq!(
            reopened
                .begin_read()
                .get(b"kv", &key.to_be_bytes())
                .unwrap()
                .unwrap(),
            expected[key as usize].to_le_bytes()
        );
    }
    assert_eq!(escaped, 0u64.to_le_bytes());
}
#[test]
fn collection_preserves_business_identity_and_abort_noop_never_publish() {
    let path = Directory::new(&std::env::temp_dir());
    let mut o = options(32);
    o.auto_collect_below = 0;
    let env = Environment::create(&path.0, o).unwrap();
    initialize(&env);
    for n in 1..=4u64 {
        let mut write = env.begin_write().unwrap();
        write.put(b"kv", b"state", &n.to_le_bytes()).unwrap();
        write.commit().unwrap();
    }
    let read = env.begin_read();
    let identity = read.commit_identity();
    let sequence = read.sequence();
    let lsn = env.physical_lsn();
    drop(read);
    let report = env.collect().unwrap();
    assert!(report.retired_versions > 0);
    assert!(report.graph_reads > 0);
    assert_eq!((report.sequence, report.physical_lsn), (sequence, lsn + 1));
    assert_eq!(env.begin_read().commit_identity(), identity);
    let selected = (env.sequence(), env.physical_lsn(), env.wal_bytes());
    {
        let mut write = env.begin_write().unwrap();
        write.put(b"kv", b"state", b"aborted").unwrap();
    }
    assert_eq!(
        (env.sequence(), env.physical_lsn(), env.wal_bytes()),
        selected
    );
    assert_eq!(env.begin_write().unwrap().commit().unwrap(), sequence);
    assert_eq!(
        (env.sequence(), env.physical_lsn(), env.wal_bytes()),
        selected
    );
    let mut write = env.begin_write().unwrap();
    write.put(b"kv", b"state", &4u64.to_le_bytes()).unwrap();
    assert_eq!(write.commit().unwrap(), sequence);
    assert_eq!(
        (env.sequence(), env.physical_lsn(), env.wal_bytes()),
        selected
    );
}
#[test]
fn retained_snapshots_apply_bounded_backpressure_then_release_maps_for_retry() {
    let path = Directory::new(&std::env::temp_dir());
    let o = options(16);
    let env = Environment::create(&path.0, o).unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    write.put(b"kv", b"state", &0u64.to_le_bytes()).unwrap();
    write.commit().unwrap();
    let first = env.begin_read();
    let escaped = first.get(b"kv", b"state").unwrap().unwrap();
    let mut retained = Vec::new();
    let mut blocked = false;
    for n in 1..=64u64 {
        retained.push(env.begin_read());
        let result = (|| {
            let mut write = env.begin_write()?;
            write.put(b"kv", b"state", &n.to_le_bytes())?;
            write.commit()
        })();
        match result {
            Ok(_) => {}
            Err(Error::LimitExceeded | Error::Busy) => {
                blocked = true;
                break;
            }
            Err(error) => panic!("unexpected backpressure error: {error}"),
        }
    }
    assert!(blocked);
    assert_eq!(escaped, 0u64.to_le_bytes());
    assert!(!env.pool_stats().unwrap().poisoned);
    let selected = env.sequence();
    drop(retained);
    env.collect().unwrap();
    let mut write = env.begin_write().unwrap();
    write.put(b"kv", b"state", b"after release").unwrap();
    assert_eq!(write.commit().unwrap(), selected + 1);
    assert_eq!(escaped, 0u64.to_le_bytes());
}

#[test]
fn create_then_drop_commits_counter_only_state_without_allocating_fake_pages() {
    let path = Directory::new(&std::env::temp_dir());
    let o = options(16);
    let env = Environment::create(&path.0, o).unwrap();
    let before = env.pool_stats().unwrap().allocations;
    let mut write = env.begin_write().unwrap();
    write
        .create_database(b"temporary", DatabaseKind::Kv)
        .unwrap();
    write.drop_database(b"temporary").unwrap();
    assert_eq!(write.commit().unwrap(), 1);
    assert_eq!((env.sequence(), env.physical_lsn()), (1, 1));
    assert_eq!(env.begin_read().database_count(), 0);
    assert_eq!(env.pool_stats().unwrap().allocations, before);
    let committed_bytes = env.wal_bytes();
    assert!(committed_bytes > 196_608);
    drop(env);
    let env = Environment::open(&path.0, o).unwrap();
    assert_eq!(
        (env.sequence(), env.physical_lsn(), env.wal_bytes()),
        (1, 1, committed_bytes)
    );
    assert_eq!(env.begin_read().database_count(), 0);
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kept", DatabaseKind::Kv).unwrap();
    write.put(b"kept", b"key", b"value").unwrap();
    assert_eq!(write.commit().unwrap(), 2);
    assert_eq!(
        env.begin_read().get(b"kept", b"key").unwrap(),
        Some(b"value".as_slice())
    );
}
