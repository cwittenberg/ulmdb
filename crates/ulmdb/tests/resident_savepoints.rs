//! Resident child commits publish only with the outer writer. Real catalog,
//! private KV/radix, allocation exhaustion, checkpoint and reopen journeys.
use std::{fs, path::PathBuf};
use ulmdb::{
    Address, DatabaseKind, Error, Prefix,
    resident::{Environment, Options, RedoEncoding, RedoFormat},
};
use ulmdb_storage::resident::Config;
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "resident-savepoints-{:032x}",
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
            kv_pages: 48,
            tiny_pages: 24,
            max_reserved_bytes: 8 << 20,
            max_metadata_bytes: 16 << 20,
            max_views: 64,
        },
        wal_capacity: 256 << 10,
        graph_read_budget: 1_000_000,
        auto_collect_below: 8,
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
fn children_release_drop_abort_rollback_restore_catalog_kv_and_prefix_through_reopen() {
    for (format, encoding) in modes() {
        let dir = Dir::new();
        let o = options();
        let mut env = Environment::create_with_encoding(&dir.0, o, format, encoding).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
        w.put(b"kv", b"key", b"old").unwrap();
        let prefix = Prefix::new(Address::V4(0x0a000000), 8).unwrap();
        let address = Address::V4(0x0a010203);
        w.insert_prefix(b"ip", prefix, b"old-ip").unwrap();
        w.commit().unwrap();
        let old = env.begin_read();
        let borrowed = old.get(b"kv", b"key").unwrap().unwrap();
        let before = env.wal_bytes();
        let mut w = env.begin_write().unwrap();
        // Force an already-private parent page, not only committed-page CoW.
        w.put(b"kv", b"key", b"parent").unwrap();
        w.insert_prefix(b"ip", prefix, b"parent-ip").unwrap();
        {
            let mut child = w.savepoint().unwrap();
            child.put(b"kv", b"key", b"child").unwrap();
            child.insert_prefix(b"ip", prefix, b"child-ip").unwrap();
            child.drop_database(b"kv").unwrap();
            child.create_database(b"ghost", DatabaseKind::Kv).unwrap();
            child.put(b"ghost", b"g", b"drop").unwrap();
            // Dropping the scope restores parent roots and complete topology.
        }
        assert_eq!(w.get(b"kv", b"key").unwrap(), Some(b"parent".as_slice()));
        assert!(matches!(w.get(b"ghost", b"g"), Err(Error::MissingDatabase)));
        assert_eq!(
            w.longest_match(b"ip", address).unwrap().unwrap().1,
            b"parent-ip"
        );
        {
            let mut child = w.savepoint().unwrap();
            child.put(b"kv", b"key", b"released").unwrap();
            {
                let mut grandchild = child.savepoint().unwrap();
                grandchild.put(b"kv", b"key", b"grandchild").unwrap();
                grandchild
                    .create_database(b"born", DatabaseKind::Kv)
                    .unwrap();
                grandchild.commit().unwrap();
            }
            assert_eq!(
                child.get(b"kv", b"key").unwrap(),
                Some(b"grandchild".as_slice())
            );
            child.rollback().unwrap();
            assert_eq!(
                child.get(b"kv", b"key").unwrap(),
                Some(b"parent".as_slice())
            );
            assert!(matches!(
                child.get(b"born", b"k"),
                Err(Error::MissingDatabase)
            ));
            child.put(b"kv", b"key", b"final").unwrap();
            child.insert_prefix(b"ip", prefix, b"final-ip").unwrap();
            child.create_database(b"kept", DatabaseKind::Kv).unwrap();
            child.put(b"kept", b"k", b"yes").unwrap();
            child.release().unwrap();
        }
        assert_eq!(before, env.wal_bytes());
        assert_eq!(borrowed, b"old");
        assert_eq!(
            old.longest_match(b"ip", address).unwrap().unwrap().1,
            b"old-ip"
        );
        assert_eq!(w.commit().unwrap(), 2);
        assert_eq!(borrowed, b"old");
        // Old readers survive an authenticated checkpoint and tail replacement.
        env.checkpoint().unwrap();
        assert_eq!(borrowed, b"old");
        drop(old);
        drop(env);
        env = Environment::open_with_encoding(&dir.0, o, format, encoding).unwrap();
        let r = env.begin_read();
        assert_eq!(r.sequence(), 2);
        assert_eq!(r.get(b"kv", b"key").unwrap(), Some(b"final".as_slice()));
        assert_eq!(r.get(b"kept", b"k").unwrap(), Some(b"yes".as_slice()));
        assert_eq!(
            r.longest_match(b"ip", address).unwrap().unwrap().1,
            b"final-ip"
        );
        assert!(matches!(r.get(b"ghost", b"g"), Err(Error::MissingDatabase)));
    }
}
#[test]
fn failed_child_exhaustion_rolls_back_and_parent_can_commit() {
    for (format, encoding) in modes() {
        let dir = Dir::new();
        let mut o = options();
        o.pool.kv_pages = 8;
        o.pool.tiny_pages = 8;
        o.auto_collect_below = 0;
        let env = Environment::create_with_encoding(&dir.0, o, format, encoding).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"key", b"parent").unwrap();
        let mut child = w.savepoint().unwrap();
        let mut failed = false;
        for i in 0u64..1000 {
            if child.put(b"kv", &i.to_be_bytes(), &[7; 1000]).is_err() {
                failed = true;
                break;
            }
        }
        assert!(failed);
        assert!(child.release().is_err());
        assert_eq!(w.get(b"kv", b"key").unwrap(), Some(b"parent".as_slice()));
        assert_eq!(w.get(b"kv", &0u64.to_be_bytes()).unwrap(), None);
        w.put(b"kv", b"key", b"recovered").unwrap();
        assert_eq!(w.commit().unwrap(), 1);
        drop(env);
        let env = Environment::open_with_encoding(&dir.0, o, format, encoding).unwrap();
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"recovered".as_slice())
        );
    }
}

#[test]
fn released_child_remains_private_until_outer_commit_and_outer_drop_aborts() {
    for (format, encoding) in modes() {
        let dir = Dir::new();
        let o = options();
        let env = Environment::create_with_encoding(&dir.0, o, format, encoding).unwrap();
        let before = env.wal_bytes();
        {
            let mut parent = env.begin_write().unwrap();
            parent.create_database(b"kv", DatabaseKind::Kv).unwrap();
            let mut child = parent.savepoint().unwrap();
            child.put(b"kv", b"k", b"unpublished").unwrap();
            child.commit().unwrap();
            assert_eq!(env.begin_read().sequence(), 0);
            assert_eq!(env.begin_read().database_count(), 0);
            assert_eq!(env.wal_bytes(), before);
        }
        assert_eq!(env.sequence(), 0);
        assert_eq!(env.wal_bytes(), before);
        if env.private_workspace_enabled() {
            assert!(!env.private_workspace_stats().unwrap().leased);
        }
        drop(env);
        let env = Environment::open_with_encoding(&dir.0, o, format, encoding).unwrap();
        assert_eq!(env.begin_read().database_count(), 0);
        let mut parent = env.begin_write().unwrap();
        parent.create_database(b"kv", DatabaseKind::Kv).unwrap();
        parent.put(b"kv", b"k", b"durable").unwrap();
        assert_eq!(parent.commit().unwrap(), 1);
    }
}
