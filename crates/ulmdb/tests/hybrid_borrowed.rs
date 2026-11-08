//! Same-executable owned/public and borrowed/reusable real Environment controls.
use ulmdb::{
    Address, DatabaseKind, Prefix,
    hybrid::{Environment, ImageMode, LiveRedoMode, Options, RedoMode, WalLayout},
};
#[test]
fn both_live_strategies_preserve_kv_ip_snapshots_abort_noop_reopen_and_observation() {
    assert_eq!(Options::default().live_redo, LiveRedoMode::Owned);
    for fs in ["/tmp", "/dev/shm"] {
        for image in [ImageMode::File, ImageMode::Anonymous] {
            for layout in [
                WalLayout::Dense,
                WalLayout::PreSized {
                    capacity: 1024 * 1024,
                },
            ] {
                for redo in [RedoMode::Compact, RedoMode::FullPostimage] {
                    for mode in [LiveRedoMode::Owned, LiveRedoMode::Borrowed] {
                        let path = std::path::Path::new(fs).join(format!(
                            "ulmdb-live-recipe-{:032x}",
                            ulmdb_storage::new_identity().unwrap()
                        ));
                        let mut options = Options {
                            image,
                            wal_layout: layout,
                            redo,
                            live_redo: mode,
                            ..Options::default()
                        };
                        options.chunk.capacity = 2 * 1024 * 1024;
                        options.chunk.reserve_quantum = 65536;
                        options.chunk.max_burned_bytes = 1024 * 1024;
                        options.max_image_bytes = 8 * 1024 * 1024;
                        let env = Environment::create(&path, options).unwrap();
                        assert_eq!(env.live_redo_usage().unwrap().last_committed, None);
                        let p4 = Prefix::new(Address::V4(0x0a00_0000), 8).unwrap();
                        let p6 = Prefix::new(Address::V6(0x2001_0db8u128 << 96), 32).unwrap();
                        let mut w = env.begin_write().unwrap();
                        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
                        w.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
                        w.create_database(b"v6", DatabaseKind::Ipv6).unwrap();
                        for i in 0..128u64 {
                            w.put(b"kv", &i.to_be_bytes(), &[0; 32]).unwrap();
                        }
                        w.insert_prefix(b"v4", p4, b"old").unwrap();
                        w.insert_prefix(b"v6", p6, b"old").unwrap();
                        w.commit().unwrap();
                        let old = env.begin_read();
                        let held = old.get(b"kv", &0u64.to_be_bytes()).unwrap().unwrap();
                        assert_eq!(env.begin_write().unwrap().commit().unwrap(), 1);
                        let mut aborted = env.begin_write().unwrap();
                        aborted.put(b"kv", &0u64.to_be_bytes(), b"abort").unwrap();
                        drop(aborted);
                        for round in 1..4u8 {
                            let mut w = env.begin_write().unwrap();
                            for i in 0..128u64 {
                                w.put(b"kv", &i.to_be_bytes(), &[round; 32]).unwrap();
                            }
                            w.insert_prefix(b"v4", p4, b"new").unwrap();
                            w.insert_prefix(b"v6", p6, b"new").unwrap();
                            w.commit().unwrap();
                        }
                        let used = env.live_redo_usage().unwrap();
                        assert_eq!(used.configured, mode);
                        assert_eq!(used.last_committed, Some(mode));
                        if mode == LiveRedoMode::Owned {
                            assert_eq!(used.encoding_capacity_bytes, 0);
                            assert_eq!(used.patch_metadata_capacity_bytes, 0);
                        } else {
                            assert!(used.encoding_capacity_bytes > 0);
                            assert!(used.encoding_capacity_bytes <= options.max_wal_bytes as usize);
                            assert!(
                                used.patch_metadata_capacity_bytes
                                    <= options.max_wal_bytes as usize
                            );
                        }
                        assert_eq!(held, [0; 32]);
                        drop(env);
                        assert_eq!(held, [0; 32]);
                        // Strategy is not persisted: either may write/recover the same exact format.
                        options.live_redo = if mode == LiveRedoMode::Owned {
                            LiveRedoMode::Borrowed
                        } else {
                            LiveRedoMode::Owned
                        };
                        let reopened = Environment::open(&path, options).unwrap();
                        let r = reopened.begin_read();
                        for i in 0..128u64 {
                            assert_eq!(
                                r.get(b"kv", &i.to_be_bytes()).unwrap(),
                                Some([3; 32].as_slice())
                            );
                        }
                        assert_eq!(
                            r.prefix_index(b"v4").unwrap().exact(p4).unwrap().unwrap().1,
                            b"new"
                        );
                        assert_eq!(
                            r.prefix_index(b"v6").unwrap().exact(p6).unwrap().unwrap().1,
                            b"new"
                        );
                        assert_eq!(held, [0; 32]);
                        drop(r);
                        drop(reopened);
                        drop(old);
                        std::fs::remove_dir_all(path).unwrap();
                    }
                }
            }
        }
    }
}
