#![cfg(all(feature = "range-lifecycle", target_os = "linux"))]
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix, PublicationMode, ReadTxn};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "range-retention-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn seed(directory: &Dir) -> Environment {
    let env = Environment::create_with_publication(&directory.0, PublicationMode::Range).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"dups", DatabaseKind::Duplicates { width: Some(2) })
        .unwrap();
    w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
    w.put(b"kv", b"k", b"A").unwrap();
    w.insert_duplicate(b"dups", b"k", b"aa").unwrap();
    w.insert_duplicate(b"dups", b"k", b"bb").unwrap();
    w.insert_prefix(
        b"ip",
        Prefix::new(Address::V4(0x0a000000), 8).unwrap(),
        b"old",
    )
    .unwrap();
    w.commit().unwrap();
    env
}
fn old_values(read: &ReadTxn) {
    assert_eq!(read.get(b"kv", b"k").unwrap(), Some(b"A".as_slice()));
    assert_eq!(read.duplicate_count(b"dups", b"k").unwrap(), 2);
    assert_eq!(
        read.duplicate_first(b"dups", b"k").unwrap(),
        Some(b"aa".as_slice())
    );
    assert_eq!(
        read.exact_prefix(b"ip", Prefix::new(Address::V4(0x0a000000), 8).unwrap())
            .unwrap()
            .unwrap()
            .1,
        b"old"
    );
}
#[test]
fn named_range_pin_preserves_kv_duplicates_prefix_and_reopens_after_owner_restart() {
    let d = Dir::new();
    let env = seed(&d);
    let old = env.begin_read();
    let borrowed = old.get(b"kv", b"k").unwrap().unwrap();
    let expected = (old.generation(), old.sequence(), old.commit_identity());
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"k", b"B").unwrap();
    w.commit().unwrap();
    let before = fs::read(d.0.join("RANGE.pack")).unwrap();
    let retained = env.retain_snapshot("stable", &old).unwrap();
    assert_eq!(
        (retained.generation, retained.sequence, retained.commit),
        expected
    );
    assert_eq!(env.retained_snapshots().unwrap(), vec![retained.clone()]);
    assert!(matches!(
        env.retain_snapshot("stable", &old),
        Err(Error::InvalidInput(_))
    ));
    assert!(env.retain_snapshot("../bad", &old).is_err());
    old_values(&env.open_retained("stable").unwrap());
    let held = env.begin_write().unwrap();
    assert!(matches!(env.retained_snapshots(), Err(Error::Busy)));
    assert!(matches!(env.compact_range(1 << 20), Err(Error::Busy)));
    assert!(matches!(env.reclaim_range(8), Err(Error::Busy)));
    drop(held);
    assert_eq!(fs::read(d.0.join("RANGE.pack")).unwrap(), before);
    let current = env.begin_read();
    let business = (current.sequence(), current.commit_identity());
    let previous_generation = current.generation();
    drop(current);
    let compacted = env.compact_range(1 << 20).unwrap();
    assert_eq!((compacted.sequence, compacted.commit), business);
    assert_eq!(compacted.previous_generation, previous_generation);
    assert_ne!(compacted.generation, previous_generation);
    assert_eq!(borrowed, b"A");
    old_values(&old);
    old_values(&env.open_retained("stable").unwrap());
    let protected = env.reclaim_range(8).unwrap();
    assert_eq!(protected.removed_assets, 0, "named pin keeps the old asset");
    assert_eq!(fs::read(d.0.join("RANGE.pack")).unwrap(), before);
    drop(old);
    drop(env);
    let env = Environment::open(&d.0).unwrap();
    assert_eq!(
        env.begin_read().get(b"kv", b"k").unwrap(),
        Some(b"B".as_slice())
    );
    let restored = env.open_retained("stable").unwrap();
    assert_eq!(
        (
            restored.generation(),
            restored.sequence(),
            restored.commit_identity()
        ),
        expected
    );
    old_values(&restored);
    env.release_snapshot("stable").unwrap();
    assert!(env.retained_snapshots().unwrap().is_empty());
    old_values(&restored);
    let protected = env.reclaim_range(8).unwrap();
    assert_eq!(protected.removed_assets, 0);
    assert_eq!(
        protected.busy_assets, 1,
        "released pin still has a live reader"
    );
    assert_eq!(fs::read(d.0.join("RANGE.pack")).unwrap(), before);
    drop(restored);
    let retired = env.reclaim_range(8).unwrap();
    assert_eq!(retired.removed_assets, 1);
    assert_eq!(retired.removed_bytes, before.len() as u64);
    assert!(!d.0.join("RANGE.pack").exists());
    let current = env.begin_read();
    assert_eq!((current.sequence(), current.commit_identity()), business);
    assert_eq!(current.get(b"kv", b"k").unwrap(), Some(b"B".as_slice()));
    drop(current);
    drop(env);
    let env = Environment::open(&d.0).unwrap();
    assert_eq!(
        env.begin_read().get(b"kv", b"k").unwrap(),
        Some(b"B".as_slice())
    );
}
#[test]
fn malformed_rehashed_or_foreign_range_pin_fails_without_pack_or_pin_removal() {
    let d = Dir::new();
    let env = seed(&d);
    let old = env.begin_read();
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"k", b"B").unwrap();
    w.commit().unwrap();
    env.retain_snapshot("stable", &old).unwrap();
    let path = d.0.join("retained-range/stable.pin");
    let valid = fs::read(&path).unwrap();
    assert_eq!(valid.len(), 256);
    let before = fs::read(d.0.join("RANGE.pack")).unwrap();
    for offset in [0, 10, 16, 32, 48, 56, 72, 104, 160, 176, 184, 192] {
        let mut bad = valid.clone();
        bad[offset] ^= 0x40;
        let hash = Sha256::digest(&bad[..224]);
        bad[224..].copy_from_slice(&hash);
        fs::write(&path, &bad).unwrap();
        assert!(env.open_retained("stable").is_err(), "offset={offset}");
        assert!(env.release_snapshot("stable").is_err());
        assert_eq!(fs::read(&path).unwrap(), bad);
        assert_eq!(fs::read(d.0.join("RANGE.pack")).unwrap(), before);
    }
    for bad in [&valid[..255], valid.as_slice()].into_iter().enumerate() {
        let mut bytes = bad.1.to_vec();
        if bad.0 == 1 {
            bytes.push(0);
        }
        fs::write(&path, &bytes).unwrap();
        assert!(env.open_retained("stable").is_err());
    }
    fs::write(&path, &valid).unwrap();
    old_values(&env.open_retained("stable").unwrap());
    let other = Dir::new();
    let other_env = seed(&other);
    assert!(matches!(
        other_env.retain_snapshot("foreign", &old),
        Err(Error::StaleHandle)
    ));
    assert!(!other.0.join("retained-range").exists());
}
#[test]
fn format5_retention_remains_refused_without_creating_control_records() {
    let d = Dir::new();
    let env = Environment::create_with_publication(&d.0, PublicationMode::RangeV5).unwrap();
    let read = env.begin_read();
    let before = fs::read(d.0.join("RANGE.pack")).unwrap();
    assert!(env.retain_snapshot("old", &read).is_err());
    assert!(env.retained_snapshots().is_err());
    assert!(env.open_retained("old").is_err());
    assert!(env.release_snapshot("old").is_err());
    assert!(env.compact_range(1 << 20).is_err());
    assert!(env.reclaim_range(8).is_err());
    assert!(!d.0.join("retained-range").exists());
    assert_eq!(fs::read(d.0.join("RANGE.pack")).unwrap(), before);
}
#[test]
#[ignore]
fn range_reader_worker() {
    let d = PathBuf::from(std::env::var_os("ULMDB_RANGE_LEASE_WORKER").unwrap());
    let read = ReadTxn::open_snapshot(&d).unwrap();
    let borrowed = read.get(b"kv", b"k").unwrap().unwrap();
    old_values(&read);
    fs::write(d.join("reader-ready"), b"ready").unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while !d.join("reader-release").exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(borrowed, b"A");
    old_values(&read);
}
#[test]
fn independent_reader_shared_lease_allows_append_and_blocks_destructive_readiness_until_drop() {
    use std::os::unix::process::CommandExt;
    let d = Dir::new();
    drop(seed(&d));
    let mut worker = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "range_reader_worker", "--nocapture"])
        .env("ULMDB_RANGE_LEASE_WORKER", &d.0)
        .process_group(0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let deadline = Instant::now() + Duration::from_secs(6);
        while !d.0.join("reader-ready").exists() {
            assert!(Instant::now() < deadline);
            assert!(worker.try_wait().unwrap().is_none());
            std::thread::sleep(Duration::from_millis(5));
        }
        let file = fs::File::open(d.0.join("RANGE.pack")).unwrap();
        assert!(matches!(
            ulmdb_storage::hybrid::retirement::ExclusiveLease::try_acquire(
                &d.0.join("RANGE.pack"),
                &file
            ),
            Err(Error::Busy)
        ));
        let env = Environment::open(&d.0).unwrap();
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"k", b"B").unwrap();
        w.commit().unwrap();
        let business = (
            env.begin_read().sequence(),
            env.begin_read().commit_identity(),
        );
        let compacted = env.compact_range(1 << 20).unwrap();
        assert_eq!((compacted.sequence, compacted.commit), business);
        let blocked = env.reclaim_range(8).unwrap();
        assert_eq!(blocked.removed_assets, 0);
        assert_eq!(
            blocked.busy_assets, 1,
            "independent reader owns the old pack"
        );
        assert!(d.0.join("RANGE.pack").is_file());
        assert!(matches!(
            ulmdb_storage::hybrid::retirement::ExclusiveLease::try_acquire(
                &d.0.join("RANGE.pack"),
                &file
            ),
            Err(Error::Busy)
        ));
        fs::write(d.0.join("reader-release"), b"release").unwrap();
        while worker.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let ready = ulmdb_storage::hybrid::retirement::ExclusiveLease::try_acquire(
            &d.0.join("RANGE.pack"),
            &file,
        )
        .unwrap();
        drop(ready);
        let retired = env.reclaim_range(8).unwrap();
        assert_eq!(retired.removed_assets, 1);
        assert!(retired.removed_bytes > 0);
        assert!(!d.0.join("RANGE.pack").exists());
        assert_eq!(
            env.begin_read().get(b"kv", b"k").unwrap(),
            Some(b"B".as_slice())
        );
    }));
    if result.is_err() {
        let _ = worker.kill();
        let _ = worker.wait();
        std::panic::resume_unwind(result.unwrap_err());
    }
    let output = worker.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let env = Environment::open(&d.0).unwrap();
    assert_eq!(
        env.begin_read().get(b"kv", b"k").unwrap(),
        Some(b"B".as_slice())
    );
}
