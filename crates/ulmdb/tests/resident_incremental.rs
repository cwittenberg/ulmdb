//! Actual opt-in RL2 transactions: bounded retirement, old readers and reopen.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use ulmdb::{
    Address, DatabaseKind, Error, Prefix,
    resident::{Environment, IncrementalLimits, Options, RedoEncoding, RedoFormat},
};
use ulmdb_storage::resident::{Config, versioned_redo::Body, wal_codec_v2 as codec};

struct Directory(PathBuf);
impl Directory {
    fn new(base: &Path) -> Self {
        Self(base.join(format!(
            "ulmdb-incremental-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn options() -> Options {
    Options {
        pool: Config {
            kv_pages: 32,
            tiny_pages: 64,
            max_reserved_bytes: 64 << 20,
            max_metadata_bytes: 64 << 20,
            max_views: 64,
        },
        wal_capacity: 8 << 20,
        graph_read_budget: 2_000_000,
        auto_collect_below: 0,
    }
}
fn bases() -> Vec<PathBuf> {
    [std::env::temp_dir(), PathBuf::from("/dev/shm")]
        .into_iter()
        .filter(|p| p.is_dir())
        .collect()
}
fn modes() -> [(RedoFormat, RedoEncoding); 4] {
    [
        (RedoFormat::CompactV2, RedoEncoding::Owned),
        (RedoFormat::CompactV2, RedoEncoding::Borrowed),
        (RedoFormat::FullPostimageV2, RedoEncoding::Owned),
        (RedoFormat::FullPostimageV2, RedoEncoding::Borrowed),
    ]
}
fn prefix() -> Prefix {
    Prefix::new(Address::V4(0), 0).unwrap()
}
fn initialize(env: &Environment) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let mut txn = env.begin_write().unwrap();
    txn.create_database(b"kv", DatabaseKind::Kv).unwrap();
    txn.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
    txn.create_database(b"v6", DatabaseKind::Ipv6).unwrap();
    let mut expected = BTreeMap::new();
    for n in 0..128u64 {
        let key = n.to_be_bytes().to_vec();
        let value = n.to_le_bytes().to_vec();
        txn.put(b"kv", &key, &value).unwrap();
        expected.insert(key, value);
    }
    txn.insert_prefix(b"v4", prefix(), b"four").unwrap();
    txn.insert_prefix(b"v6", Prefix::new(Address::V6(0), 0).unwrap(), b"six")
        .unwrap();
    assert_eq!(txn.commit().unwrap(), 1);
    expected
}
fn check(env: &Environment, expected: &BTreeMap<Vec<u8>, Vec<u8>>) {
    let read = env.begin_read();
    for (key, value) in expected {
        assert_eq!(read.get(b"kv", key).unwrap(), Some(value.as_slice()));
    }
    assert_eq!(
        read.longest_match(b"v4", Address::V4(17))
            .unwrap()
            .unwrap()
            .1,
        b"four"
    );
    assert_eq!(
        read.longest_match(b"v6", Address::V6(17))
            .unwrap()
            .unwrap()
            .1,
        b"six"
    );
}
fn retired_writes(env: &Environment) -> usize {
    let bytes = std::fs::read(env.directory().join("RESIDENT.wal")).unwrap();
    let boot = codec::Boot {
        uuid: bytes[16..32].try_into().unwrap(),
        epoch: u64::from_le_bytes(bytes[32..40].try_into().unwrap()),
        commit: u128::from_le_bytes(bytes[40..56].try_into().unwrap()),
        kv_pages: u32::from_le_bytes(bytes[56..60].try_into().unwrap()),
        tiny_pages: u32::from_le_bytes(bytes[60..64].try_into().unwrap()),
    };
    let mut state = codec::ReplayState::new(boot).unwrap();
    let mut at = codec::DATA_START as usize;
    let mut count = 0;
    while at < env.wal_bytes() as usize {
        let len = codec::frame_len(&bytes[at..at + codec::HEADER]).unwrap();
        let decoded = codec::decode(boot, &bytes[at..at + len], at as u64).unwrap();
        let record = decoded.record();
        assert_eq!(record.kind, codec::Kind::Write);
        assert!(record.retired.len() <= 64);
        for allocation in &record.allocations {
            if let Body::Copy { source, .. } = &allocation.body {
                assert!(!record.retired.contains(source));
            }
        }
        count += usize::from(!record.retired.is_empty());
        state.apply(&decoded).unwrap();
        at += len;
    }
    assert_eq!(at, env.wal_bytes() as usize);
    count
}

#[test]
fn ordinary_writes_retire_and_reuse_without_collect_while_old_readers_stay_valid() {
    for base in bases() {
        for (format, encoding) in modes() {
            let directory = Directory::new(&base);
            let env = Environment::create_with_encoding(&directory.0, options(), format, encoding)
                .unwrap();
            let mut expected = initialize(&env);
            let old = env.begin_read();
            let old_value = old.get(b"kv", &0u64.to_be_bytes()).unwrap().unwrap();
            env.enable_incremental(IncrementalLimits::default())
                .unwrap();
            for n in 0..96u64 {
                let key = (n % 128).to_be_bytes();
                let value = (1000 + n).to_le_bytes();
                let mut txn = env.begin_write().unwrap();
                txn.put(b"kv", &key, &value).unwrap();
                txn.commit().unwrap();
                expected.insert(key.to_vec(), value.to_vec());
                assert_eq!(old_value, 0u64.to_le_bytes());
            }
            assert_eq!(env.sequence(), 97);
            assert_eq!(env.physical_lsn(), 97);
            assert!(env.pool_stats().unwrap().reused_pages > 0);
            assert!(retired_writes(&env) > 80);
            let metrics = env.incremental_metrics().unwrap().unwrap();
            assert!(metrics.expanded_objects < 32, "{metrics:?}");
            check(&env, &expected);
            drop(env);
            assert_eq!(old_value, 0u64.to_le_bytes());
            let env =
                Environment::open_with_encoding(&directory.0, options(), format, encoding).unwrap();
            assert!(env.incremental_metrics().unwrap().is_none());
            check(&env, &expected);
            env.enable_incremental(IncrementalLimits::default())
                .unwrap();
            assert_eq!(old_value, 0u64.to_le_bytes());
        }
    }
}

#[test]
fn explicit_collect_abort_noop_and_drop_recreate_keep_certificate_in_sync() {
    for base in bases() {
        for (format, encoding) in modes() {
            let directory = Directory::new(&base);
            let env = Environment::create_with_encoding(&directory.0, options(), format, encoding)
                .unwrap();
            let mut expected = initialize(&env);
            env.enable_incremental(IncrementalLimits::default())
                .unwrap();
            let sequence = env.sequence();
            {
                let mut txn = env.begin_write().unwrap();
                txn.put(b"kv", &0u64.to_be_bytes(), b"aborted").unwrap();
            }
            assert_eq!(env.sequence(), sequence);
            assert_eq!(env.begin_write().unwrap().commit().unwrap(), sequence);
            let mut txn = env.begin_write().unwrap();
            txn.put(b"kv", &0u64.to_be_bytes(), b"committed").unwrap();
            txn.commit().unwrap();
            expected.insert(0u64.to_be_bytes().to_vec(), b"committed".to_vec());
            let collection = env.collect().unwrap();
            assert!(collection.retired_versions > 0);
            let mut txn = env.begin_write().unwrap();
            txn.drop_database(b"v4").unwrap();
            txn.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
            txn.insert_prefix(b"v4", prefix(), b"four").unwrap();
            txn.commit().unwrap();
            check(&env, &expected);
            let mut txn = env.begin_write().unwrap();
            txn.create_database(b"transient", DatabaseKind::Kv).unwrap();
            txn.drop_database(b"transient").unwrap();
            txn.commit().unwrap();
            check(&env, &expected);
            drop(env);
            let env =
                Environment::open_with_encoding(&directory.0, options(), format, encoding).unwrap();
            check(&env, &expected);
        }
    }
}

#[test]
fn preparation_budget_failure_has_no_publication_and_can_be_reconfigured() {
    let base = std::env::temp_dir();
    let directory = Directory::new(&base);
    let env = Environment::create_with_encoding(
        &directory.0,
        options(),
        RedoFormat::CompactV2,
        RedoEncoding::Borrowed,
    )
    .unwrap();
    env.enable_incremental(IncrementalLimits {
        max_work: 1,
        ..Default::default()
    })
    .unwrap();
    let mut txn = env.begin_write().unwrap();
    txn.create_database(b"kv", DatabaseKind::Kv).unwrap();
    txn.put(b"kv", b"k", b"v").unwrap();
    assert!(matches!(txn.commit(), Err(Error::LimitExceeded)));
    assert_eq!(env.sequence(), 0);
    assert_eq!(env.begin_read().database_count(), 0);
    assert!(!env.pool_stats().unwrap().poisoned);
    env.enable_incremental(IncrementalLimits::default())
        .unwrap();
    let mut txn = env.begin_write().unwrap();
    txn.create_database(b"kv", DatabaseKind::Kv).unwrap();
    txn.put(b"kv", b"k", b"v").unwrap();
    assert_eq!(txn.commit().unwrap(), 1);
    assert_eq!(
        env.begin_read().get(b"kv", b"k").unwrap(),
        Some(b"v".as_slice())
    );
}
