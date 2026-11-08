#![cfg(feature = "resident-private-index")]
use std::{
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use ulmdb::{
    DatabaseKind, Error,
    resident::{Environment, Options, RedoEncoding, RedoFormat},
};
struct Directory(PathBuf);
impl Directory {
    fn new(base: &Path) -> Self {
        Self(base.join(format!(
            "ulmdb-private-{:032x}",
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
        pool: ulmdb_storage::resident::Config {
            kv_pages: 32,
            tiny_pages: 64,
            max_reserved_bytes: 8 << 20,
            max_metadata_bytes: 32 << 20,
            max_views: 32,
        },
        wal_capacity: 8 << 20,
        graph_read_budget: 2_000_000,
        auto_collect_below: 8,
    }
}
fn create(path: &Path, o: Options) -> Environment {
    Environment::create_with_encoding(path, o, RedoFormat::CompactV2, RedoEncoding::Borrowed)
        .unwrap()
}
#[test]
fn scratch_capacity_reused_across_real_commit_abort_collect_and_cold_owner() {
    for base in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        if !base.is_dir() {
            continue;
        }
        let dir = Directory::new(&base);
        let o = options();
        let env = create(&dir.0, o);
        assert!(env.private_workspace_enabled());
        let initial = env.private_workspace_stats().unwrap();
        assert_eq!(initial.configured_slots, 96);
        assert_eq!(initial.index_capacity_bytes, 192);
        assert_eq!(
            initial.aggregate_reserved_bytes,
            initial.charged_capacity_bytes as u64
        );
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"state", b"old").unwrap();
        w.commit().unwrap();
        let read = env.begin_read();
        let held = read.get(b"kv", b"state").unwrap().unwrap();
        let address = held.as_ptr();
        for round in 0..64u64 {
            {
                let mut w = env.begin_write().unwrap();
                w.put(b"kv", b"state", b"aborted").unwrap();
            }
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"state", &round.to_le_bytes()).unwrap();
            w.commit().unwrap();
            assert_eq!(held, b"old");
            assert_eq!(held.as_ptr(), address);
            let now = env.private_workspace_stats().unwrap();
            assert!(!now.leased);
            assert_eq!(now.charged_capacity_bytes, initial.charged_capacity_bytes);
            assert_eq!(
                now.aggregate_reserved_bytes,
                initial.charged_capacity_bytes as u64
            );
        }
        assert!(env.pool_stats().unwrap().reused_pages > 0);
        env.collect().unwrap();
        let leases = env.private_workspace_stats().unwrap().leases;
        assert!(leases >= 129);
        drop(read);
        drop(env);
        let env = Environment::open_with_encoding(
            &dir.0,
            o,
            RedoFormat::CompactV2,
            RedoEncoding::Borrowed,
        )
        .unwrap();
        assert_eq!(
            env.begin_read().get(b"kv", b"state").unwrap().unwrap(),
            63u64.to_le_bytes()
        );
        assert_eq!(env.private_workspace_stats().unwrap().leases, 0);
    }
}
#[test]
fn retained_budget_rejection_precedes_wal_creation_and_cold_heal() {
    for base in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        if !base.is_dir() {
            continue;
        }
        let dir = Directory::new(&base);
        let o = options();
        let env = create(&dir.0, o);
        let bound = env.pool_stats().unwrap().metadata_bound_bytes;
        let charge = env
            .private_workspace_stats()
            .unwrap()
            .charged_capacity_bytes as u64;
        let mut tight = o;
        tight.pool.max_metadata_bytes = bound + charge - 1;
        let reject = Directory::new(&base);
        assert!(matches!(
            Environment::create_with_encoding(
                &reject.0,
                tight,
                RedoFormat::CompactV2,
                RedoEncoding::Borrowed
            ),
            Err(Error::LimitExceeded)
        ));
        assert!(!reject.0.join("RESIDENT.wal").exists());
        for value in [b"one".as_slice(), b"two"] {
            let mut w = env.begin_write().unwrap();
            if env.sequence() == 0 {
                w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            }
            w.put(b"kv", b"state", value).unwrap();
            w.commit().unwrap();
        }
        drop(env);
        let path = dir.0.join("RESIDENT.wal");
        let bytes = std::fs::read(&path).unwrap();
        let heads = [65536usize, 131072];
        let latest = *heads
            .iter()
            .max_by_key(|&&h| u64::from_le_bytes(bytes[h + 40..h + 48].try_into().unwrap()))
            .unwrap();
        let mut f = OpenOptions::new().write(true).open(&path).unwrap();
        f.seek(SeekFrom::Start((latest + 224) as u64)).unwrap();
        f.write_all(&[bytes[latest + 224] ^ 1]).unwrap();
        f.sync_all().unwrap();
        drop(f);
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            Environment::open_with_encoding(
                &dir.0,
                tight,
                RedoFormat::CompactV2,
                RedoEncoding::Borrowed
            ),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "rejected cache admission must precede healing"
        );
        let env = Environment::open_with_encoding(
            &dir.0,
            o,
            RedoFormat::CompactV2,
            RedoEncoding::Borrowed,
        )
        .unwrap();
        assert_eq!(
            env.begin_read().get(b"kv", b"state").unwrap(),
            Some(b"two".as_slice())
        );
        drop(env);
        assert_ne!(
            std::fs::read(&path).unwrap(),
            before,
            "control must actually heal the witness"
        );
        File::open(&path).unwrap().sync_all().unwrap();
    }
}
