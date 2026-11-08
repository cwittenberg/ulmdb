//! Actual RL2 transactions, compact/full controls and strict family boundaries.
use std::{
    ops::Bound,
    path::{Path, PathBuf},
};
use ulmdb::{
    Address, DatabaseKind, Prefix,
    resident::{Environment, Options, RedoFormat},
};
use ulmdb_storage::resident::{Config, versioned_redo::Body, wal_codec_v2 as codec};
struct Directory(PathBuf);
impl Directory {
    fn new(base: &Path) -> Self {
        Self(base.join(format!(
            "ulmdb-r2-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn options(pages: usize) -> Options {
    Options {
        pool: Config {
            kv_pages: pages,
            tiny_pages: 64,
            max_reserved_bytes: 64 << 20,
            max_metadata_bytes: 64 << 20,
            max_views: 64,
        },
        wal_capacity: 8 << 20,
        graph_read_budget: 2_000_000,
        auto_collect_below: pages.min(8),
    }
}
fn modes() -> [RedoFormat; 2] {
    [RedoFormat::CompactV2, RedoFormat::FullPostimageV2]
}
fn roots() -> Vec<PathBuf> {
    [std::env::temp_dir(), PathBuf::from("/dev/shm")]
        .into_iter()
        .filter(|p| p.is_dir())
        .collect()
}
fn prefixes() -> (Prefix, Prefix) {
    (
        Prefix::new(Address::V4(0x0a00_0000), 8).unwrap(),
        Prefix::new(Address::V6(0x2001_0db8u128 << 96), 32).unwrap(),
    )
}
fn initialize(env: &Environment) {
    let mut w = env.begin_write().unwrap();
    for (name, kind) in [
        (b"kv".as_slice(), DatabaseKind::Kv),
        (b"v4", DatabaseKind::Ipv4),
        (b"v6", DatabaseKind::Ipv6),
    ] {
        w.create_database(name, kind).unwrap();
    }
    let (v4, v6) = prefixes();
    w.put(b"kv", b"state", &0u64.to_le_bytes()).unwrap();
    w.insert_prefix(b"v4", v4, &0u64.to_le_bytes()).unwrap();
    w.insert_prefix(b"v6", v6, &0u64.to_le_bytes()).unwrap();
    assert_eq!(w.commit().unwrap(), 1);
}
#[derive(Default)]
struct Frames {
    copies: usize,
    literals: usize,
    collections: usize,
    sequence: u64,
    lsn: u64,
}
fn frames(env: &Environment) -> Frames {
    // Read-only observation of actual canonical outer records, including logical
    // parent/source/retirement validation. Complete typed graph validation is also
    // exercised by the separate real Environment reopen below.
    let bytes = std::fs::read(env.directory().join("RESIDENT.wal")).unwrap();
    assert_eq!(&bytes[..8], b"ULMDBRW2");
    let boot = codec::Boot {
        uuid: bytes[16..32].try_into().unwrap(),
        epoch: u64::from_le_bytes(bytes[32..40].try_into().unwrap()),
        commit: u128::from_le_bytes(bytes[40..56].try_into().unwrap()),
        kv_pages: u32::from_le_bytes(bytes[56..60].try_into().unwrap()),
        tiny_pages: u32::from_le_bytes(bytes[60..64].try_into().unwrap()),
    };
    let mut state = codec::ReplayState::new(boot).unwrap();
    let mut out = Frames::default();
    let mut at = codec::DATA_START as usize;
    let end = env.wal_bytes() as usize;
    while at < end {
        let len = codec::frame_len(&bytes[at..at + codec::HEADER]).unwrap();
        let decoded = codec::decode(boot, &bytes[at..at + len], at as u64).unwrap();
        state.apply(&decoded).unwrap();
        for a in &decoded.record().allocations {
            match a.body {
                Body::Copy { .. } => out.copies += 1,
                Body::Literal(_) => out.literals += 1,
            }
        }
        if decoded.record().kind == codec::Kind::Collect {
            out.collections += 1;
        }
        at += len;
    }
    assert_eq!(at, end);
    out.sequence = state.selected().sequence;
    out.lsn = state.selected().lsn;
    assert_eq!(
        (out.sequence, out.lsn),
        (env.sequence(), env.physical_lsn())
    );
    out
}
#[test]
fn real_cross_index_atomic_writes_escaped_borrow_and_fresh_owner_cold_replay() {
    for base in roots() {
        for mode in modes() {
            let path = Directory::new(&base);
            let o = options(64);
            let env = Environment::create_with_redo(&path.0, o, mode).unwrap();
            assert_eq!(env.redo_format(), mode);
            initialize(&env);
            let old = env.begin_read();
            let held = old.get(b"kv", b"state").unwrap().unwrap();
            let old_owner = env.pool_stats().unwrap().physical_incarnation;
            let (v4, v6) = prefixes();
            for n in 1..=12u64 {
                let mut w = env.begin_write().unwrap();
                w.put(b"kv", b"state", &n.to_le_bytes()).unwrap();
                w.insert_prefix(b"v4", v4, &n.to_le_bytes()).unwrap();
                w.insert_prefix(b"v6", v6, &n.to_le_bytes()).unwrap();
                assert_eq!(w.commit().unwrap(), n + 1);
            }
            let observed = frames(&env);
            assert!(observed.literals > 0);
            match mode {
                RedoFormat::CompactV2 => assert!(observed.copies > 0),
                RedoFormat::FullPostimageV2 => assert_eq!(observed.copies, 0),
                _ => unreachable!(),
            }
            let lsn = env.physical_lsn();
            assert_eq!(held, 0u64.to_le_bytes());
            drop(env);
            let reopened = Environment::open(&path.0, o).unwrap();
            assert_eq!(reopened.redo_format(), RedoFormat::CompactV2);
            assert_ne!(
                reopened.pool_stats().unwrap().physical_incarnation,
                old_owner
            );
            assert_eq!((reopened.sequence(), reopened.physical_lsn()), (13, lsn));
            let read = reopened.begin_read();
            assert_eq!(
                read.get(b"kv", b"state").unwrap().unwrap(),
                12u64.to_le_bytes()
            );
            for (name, p) in [(b"v4".as_slice(), v4), (b"v6", v6)] {
                let reader = read.prefix_index(name).unwrap();
                assert_eq!(reader.exact(p).unwrap().unwrap().1, 12u64.to_le_bytes());
                assert_eq!(reader.covering(p).unwrap().unwrap().0, p);
                assert_eq!(reader.subnets(p).unwrap().count(), 1);
            }
            assert_eq!(
                old.longest_match(b"v4", v4.network()).unwrap().unwrap().1,
                0u64.to_le_bytes()
            );
            assert_eq!(held, 0u64.to_le_bytes());
        }
    }
}
#[test]
fn compact_and_full_multi_leaf_churn_cross_retirement_reuse_and_restore_all_values() {
    for mode in modes() {
        let path = Directory::new(&std::env::temp_dir());
        let o = options(64);
        let env = Environment::create_with_redo(&path.0, o, mode).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        for key in 0..256u32 {
            w.put(b"kv", &key.to_be_bytes(), &0u64.to_le_bytes())
                .unwrap();
        }
        w.commit().unwrap();
        let old = env.begin_read();
        let held = old.get(b"kv", &0u32.to_be_bytes()).unwrap().unwrap();
        let mut expected = [0u64; 256];
        for round in 1..=96u64 {
            let mut w = env.begin_write().unwrap();
            for step in 0..16u32 {
                let key = (round as u32 * 17 + step) % 256;
                w.put(b"kv", &key.to_be_bytes(), &round.to_le_bytes())
                    .unwrap();
                expected[key as usize] = round;
            }
            w.commit().unwrap();
        }
        let stats = env.pool_stats().unwrap();
        assert_eq!(stats.maps_created, 128);
        assert!(stats.reused_pages > 100);
        let observed = frames(&env);
        assert!(observed.collections > 0);
        assert_eq!(observed.sequence, 97);
        assert!(observed.lsn > observed.sequence);
        if mode == RedoFormat::CompactV2 {
            assert!(observed.copies > 100);
        } else {
            assert_eq!(observed.copies, 0);
        }
        let read = env.begin_read();
        let rows = read
            .kv(b"kv")
            .unwrap()
            .cursor(Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(rows.len(), 256);
        for (k, v) in rows {
            let key = u32::from_be_bytes(k.try_into().unwrap());
            assert_eq!(v, expected[key as usize].to_le_bytes());
        }
        drop(read);
        drop(env);
        let reopened = Environment::open_with_redo(&path.0, o, mode).unwrap();
        assert_eq!(reopened.redo_format(), mode);
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
        assert_eq!(held, 0u64.to_le_bytes());
    }
}
#[test]
fn metadata_only_counter_noop_abort_and_same_family_writer_policy_switch() {
    for mode in modes() {
        let path = Directory::new(&std::env::temp_dir());
        let o = options(16);
        let env = Environment::create_with_redo(&path.0, o, mode).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"temporary", DatabaseKind::Kv).unwrap();
        w.drop_database(b"temporary").unwrap();
        assert_eq!(w.commit().unwrap(), 1);
        assert_eq!(env.pool_stats().unwrap().allocations, 0);
        assert_eq!(frames(&env).literals, 0);
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"key", b"old").unwrap();
        assert_eq!(w.commit().unwrap(), 2);
        let selected = (env.sequence(), env.physical_lsn(), env.wal_bytes());
        {
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"key", b"aborted").unwrap();
        }
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"key", b"old").unwrap();
        assert_eq!(w.commit().unwrap(), 2);
        assert_eq!(
            (env.sequence(), env.physical_lsn(), env.wal_bytes()),
            selected
        );
        drop(env);
        let other = if mode == RedoFormat::CompactV2 {
            RedoFormat::FullPostimageV2
        } else {
            RedoFormat::CompactV2
        };
        let env = Environment::open_with_redo(&path.0, o, other).unwrap();
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"key", b"new").unwrap();
        assert_eq!(w.commit().unwrap(), 3);
        drop(env);
        let env = Environment::open(&path.0, o).unwrap();
        assert_eq!(env.sequence(), 3);
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"new".as_slice())
        );
    }
}
#[test]
fn strict_family_mismatch_refuses_before_any_wal_heal_or_private_tail_mutation() {
    for mode in [RedoFormat::LiteralV1, RedoFormat::CompactV2] {
        let path = Directory::new(&std::env::temp_dir());
        let o = options(16);
        let env = Environment::create_with_redo(&path.0, o, mode).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"key", b"kept").unwrap();
        w.commit().unwrap();
        drop(env);
        let wal = path.0.join("RESIDENT.wal");
        let before = std::fs::read(&wal).unwrap();
        let wrong = if mode == RedoFormat::LiteralV1 {
            RedoFormat::CompactV2
        } else {
            RedoFormat::LiteralV1
        };
        assert!(Environment::open_with_redo(&path.0, o, wrong).is_err());
        assert_eq!(std::fs::read(&wal).unwrap(), before);
        let env = Environment::open_with_redo(&path.0, o, mode).unwrap();
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"kept".as_slice())
        );
    }
}
#[test]
fn compact_control_reduces_actual_logical_journal_bytes_without_changing_values() {
    let mut bytes = Vec::new();
    for mode in modes() {
        let path = Directory::new(&std::env::temp_dir());
        let mut o = options(64);
        o.auto_collect_below = 0;
        let env = Environment::create_with_redo(&path.0, o, mode).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        for key in 0..32u32 {
            w.put(b"kv", &key.to_be_bytes(), &0u64.to_le_bytes())
                .unwrap();
        }
        w.commit().unwrap();
        for n in 1..=16u64 {
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", &((n as u32) % 32).to_be_bytes(), &n.to_le_bytes())
                .unwrap();
            w.commit().unwrap();
        }
        let observed = frames(&env);
        if mode == RedoFormat::CompactV2 {
            assert!(observed.copies >= 16);
        } else {
            assert_eq!(observed.copies, 0);
        }
        bytes.push(env.wal_bytes());
        let read = env.begin_read();
        for key in 0..32u32 {
            let expected = if (1..=16).contains(&key) {
                u64::from(key)
            } else {
                0
            };
            assert_eq!(
                read.get(b"kv", &key.to_be_bytes()).unwrap().unwrap(),
                expected.to_le_bytes()
            );
        }
    }
    // Retain/check complete logical bytes, including the fixed192KiB prefix.
    // Separately test record-region savings (also including initial data). A
    // fixed prefix dominates this small fixture; neither number is throughput,
    // physical capacity or an LMDB comparison.
    assert!(
        bytes[0] < bytes[1],
        "total compact{} full{}",
        bytes[0],
        bytes[1]
    );
    let compact_records = bytes[0].checked_sub(codec::DATA_START).unwrap();
    let full_records = bytes[1].checked_sub(codec::DATA_START).unwrap();
    assert!(
        compact_records * 4 < full_records,
        "record bytes compact{compact_records} full{full_records}; total bytes compact{} full{}",
        bytes[0],
        bytes[1]
    );
}
