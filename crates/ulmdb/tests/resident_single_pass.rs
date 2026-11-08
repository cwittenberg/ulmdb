//! Actual mapped KV/prefix collection and cold replay under either audit policy.
use std::path::PathBuf;
use ulmdb::{
    Address, DatabaseKind, Prefix,
    resident::{Environment, Options, RedoEncoding, RedoFormat},
};
use ulmdb_storage::resident::Config;

#[test]
fn selected_audit_strategy_keeps_collect_cold_graphs_and_old_borrowed_values() {
    for base in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        for encoding in [RedoEncoding::Owned, RedoEncoding::Borrowed] {
            let path = base.join(format!(
                "ulmdb-single-pass-{:032x}",
                ulmdb_storage::new_identity().unwrap()
            ));
            let options = Options {
                pool: Config {
                    kv_pages: 64,
                    tiny_pages: 64,
                    max_reserved_bytes: 64 << 20,
                    max_metadata_bytes: 64 << 20,
                    max_views: 64,
                },
                wal_capacity: 8 << 20,
                graph_read_budget: 1_000_000,
                auto_collect_below: 0,
            };
            let env =
                Environment::create_with_encoding(&path, options, RedoFormat::CompactV2, encoding)
                    .unwrap();
            assert_eq!(
                env.single_pass_audit_enabled(),
                cfg!(feature = "resident-single-pass-audit")
            );
            let mut writer = env.begin_write().unwrap();
            for (name, kind) in [
                (b"kv".as_slice(), DatabaseKind::Kv),
                (b"v4", DatabaseKind::Ipv4),
                (b"v6", DatabaseKind::Ipv6),
            ] {
                writer.create_database(name, kind).unwrap();
            }
            for n in 0..512u32 {
                writer
                    .put(b"kv", &n.to_be_bytes(), &0u64.to_le_bytes())
                    .unwrap();
            }
            writer
                .insert_prefix(b"v4", Prefix::new(Address::V4(0), 0).unwrap(), b"network4")
                .unwrap();
            writer
                .insert_prefix(b"v6", Prefix::new(Address::V6(0), 0).unwrap(), b"network6")
                .unwrap();
            writer.commit().unwrap();
            let old = env.begin_read();
            let held = old.get(b"kv", &17u32.to_be_bytes()).unwrap().unwrap();
            let incarnation = env.pool_stats().unwrap().physical_incarnation;
            let mut expected = [0u64; 512];
            let mut retired = 0;
            for round in 1..=12u64 {
                let mut writer = env.begin_write().unwrap();
                for n in 0..16u32 {
                    let key = (round as u32 * 17 + n) % 512;
                    expected[key as usize] = round;
                    writer
                        .put(b"kv", &key.to_be_bytes(), &round.to_le_bytes())
                        .unwrap();
                }
                writer.commit().unwrap();
                let before = (env.sequence(), env.physical_lsn());
                let report = env.collect().unwrap();
                retired += report.retired_versions;
                assert_eq!(env.sequence(), before.0);
                if report.retired_versions > 0 {
                    assert_eq!(env.physical_lsn(), before.1 + 1);
                }
                assert_eq!(held, 0u64.to_le_bytes());
            }
            assert!(retired > 0);
            assert!(env.pool_stats().unwrap().reused_pages > 0);
            let sequence = env.sequence();
            drop(env);
            let reopened =
                Environment::open_with_encoding(&path, options, RedoFormat::CompactV2, encoding)
                    .unwrap();
            assert_eq!(reopened.sequence(), sequence);
            assert_ne!(
                reopened.pool_stats().unwrap().physical_incarnation,
                incarnation
            );
            let read = reopened.begin_read();
            for n in 0..512u32 {
                assert_eq!(
                    read.get(b"kv", &n.to_be_bytes()).unwrap().unwrap(),
                    expected[n as usize].to_le_bytes()
                );
            }
            assert_eq!(
                read.longest_match(b"v4", Address::V4(123))
                    .unwrap()
                    .unwrap()
                    .1,
                b"network4"
            );
            assert_eq!(
                read.longest_match(b"v6", Address::V6(123))
                    .unwrap()
                    .unwrap()
                    .1,
                b"network6"
            );
            assert_eq!(held, 0u64.to_le_bytes());
            drop(read);
            drop(reopened);
            drop(old);
            std::fs::remove_dir_all(path).unwrap();
        }
    }
}
