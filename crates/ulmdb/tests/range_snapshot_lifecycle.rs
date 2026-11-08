#![cfg(feature = "range-lifecycle")]
//! Independent Range6 reads and bounded physical export. No benchmark clocks.
use std::{
    fs,
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use ulmdb::{Address, DatabaseKind, Environment, Prefix, PublicationMode, ReadTxn};
struct Dir(PathBuf);
impl Dir {
    fn new(base: &Path) -> Self {
        Self(base.join(format!("ulmdb-range-life-{}", unique())))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn unique() -> u128 {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    ((std::process::id() as u128) << 64)
        | N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128
}
fn hash_files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut v = Vec::new();
    for item in fs::read_dir(dir).unwrap() {
        let p = item.unwrap().path();
        if p.is_file() {
            v.push((p.file_name().unwrap().into(), fs::read(p).unwrap()));
        }
    }
    v.sort();
    v
}
fn seed(env: &Environment) {
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
    w.create_database(b"v6", DatabaseKind::Ipv6).unwrap();
    for n in 0..1024u64 {
        w.put(b"kv", &n.to_be_bytes(), b"original").unwrap();
    }
    w.insert_prefix(b"v4", Prefix::new(Address::V4(0), 0).unwrap(), b"old4")
        .unwrap();
    w.insert_prefix(b"v6", Prefix::new(Address::V6(0), 0).unwrap(), b"old6")
        .unwrap();
    w.commit().unwrap();
}
fn check(read: &ReadTxn, updated: bool) {
    for n in 0..1024u64 {
        assert_eq!(
            read.get(b"kv", &n.to_be_bytes()).unwrap(),
            Some(if updated {
                &b"updated!"[..]
            } else {
                &b"original"[..]
            })
        );
    }
    assert_eq!(
        read.longest_match(b"v4", Address::V4(12))
            .unwrap()
            .unwrap()
            .1,
        if updated { b"new4" } else { b"old4" }
    );
    assert_eq!(
        read.longest_match(b"v6", Address::V6(12))
            .unwrap()
            .unwrap()
            .1,
        if updated { b"new6" } else { b"old6" }
    );
}
#[test]
fn independent_range_snapshot_does_not_lock_or_mutate_and_old_export_normalizes_heads() {
    for base in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        let source = Dir::new(&base);
        let target = Dir::new(&base);
        let current_target = Dir::new(&base);
        let warm_target = Dir::new(&base);
        let env = Environment::create_with_publication(&source.0, PublicationMode::Range).unwrap();
        seed(&env);
        let warm_old = env.begin_read();
        let before = hash_files(&source.0);
        let old = ReadTxn::open_snapshot(&source.0).unwrap();
        check(&old, false);
        assert_eq!(hash_files(&source.0), before);
        let borrowed = old.get(b"kv", &0u64.to_be_bytes()).unwrap().unwrap();
        let mut write = env.begin_write().unwrap();
        for n in 0..1024u64 {
            write.put(b"kv", &n.to_be_bytes(), b"updated!").unwrap();
        }
        write
            .insert_prefix(b"v4", Prefix::new(Address::V4(0), 0).unwrap(), b"new4")
            .unwrap();
        write
            .insert_prefix(b"v6", Prefix::new(Address::V6(0), 0).unwrap(), b"new6")
            .unwrap();
        write.commit().unwrap();
        check(&old, false);
        check(&env.begin_read(), true);
        assert_eq!(borrowed, b"original");
        let before_export = hash_files(&source.0);
        let summary =
            ulmdb::backup::export_independent_snapshot(&source.0, &old, &target.0).unwrap();
        assert_eq!(hash_files(&source.0), before_export);
        assert_eq!(
            fs::metadata(target.0.join("RANGE.pack")).unwrap().len(),
            summary.physical_bytes
        );
        assert!(summary.physical_bytes < fs::metadata(source.0.join("RANGE.pack")).unwrap().len());
        let restored = Environment::open(&target.0).unwrap();
        assert_eq!(restored.stats().format_minor, 6);
        check(&restored.begin_read(), false);
        assert_eq!(restored.stats().sequence, old.stats().sequence);
        drop(restored);
        // The old warm authority was minted privately before its ACK and only
        // exposed with the acknowledged Snapshot. It must not follow new heads.
        ulmdb::backup::export_snapshot(&env, &warm_old, &warm_target.0).unwrap();
        check(
            &Environment::open(&warm_target.0).unwrap().begin_read(),
            false,
        );
        ulmdb::backup::export(&env, &current_target.0).unwrap();
        check(
            &Environment::open(&current_target.0).unwrap().begin_read(),
            true,
        );
        drop(env);
        assert_eq!(borrowed, b"original");
        let writer = Environment::open(&source.0).unwrap();
        let mut w = writer.begin_write().unwrap();
        w.put(b"kv", &0u64.to_be_bytes(), b"newagain").unwrap();
        w.commit().unwrap();
        drop(writer);
        check(&old, false);
    }
}
#[test]
fn source_mismatch_corruption_and_lifecycle_guards_fail_before_destination_publication() {
    let base = std::env::temp_dir();
    let source = Dir::new(&base);
    let foreign = Dir::new(&base);
    let target = Dir::new(&base);
    let env = Environment::create_with_publication(&source.0, PublicationMode::Range).unwrap();
    seed(&env);
    let old = env.begin_read();
    let other = Environment::create_with_publication(&foreign.0, PublicationMode::Range).unwrap();
    seed(&other);
    assert!(ulmdb::backup::export_independent_snapshot(&foreign.0, &old, &target.0).is_err());
    assert!(!target.0.exists());
    assert!(ulmdb::backup::export_snapshot(&other, &old, &target.0).is_err());
    assert!(!target.0.exists());
    assert!(env.compact().is_err());
    assert!(env.reclaim().is_err());
    assert!(env.prune_inventory().is_err());
    assert!(env.retain_snapshot("old", &old).is_err());
    assert!(ulmdb::replication::Cursor::from_read(&old).is_err());
    drop(other);
    drop(env);
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(source.0.join("RANGE.pack"))
        .unwrap();
    // Corrupt a selected fixed head, preserving every byte through failed open.
    file.seek(SeekFrom::Start(65536)).unwrap();
    file.write_all(b"broken!!").unwrap();
    file.sync_all().unwrap();
    let before = hash_files(&source.0);
    assert!(ReadTxn::open_snapshot(&source.0).is_err());
    assert_eq!(hash_files(&source.0), before);
    assert!(!target.0.exists());
}
#[test]
fn format5_independent_path_stays_explicitly_unsupported() {
    let base = std::env::temp_dir();
    let source = Dir::new(&base);
    let target = Dir::new(&base);
    let env = Environment::create_with_publication(&source.0, PublicationMode::RangeV5).unwrap();
    seed(&env);
    let before = hash_files(&source.0);
    assert!(ReadTxn::open_snapshot(&source.0).is_err());
    assert!(ulmdb::backup::export(&env, &target.0).is_err());
    assert!(!target.0.exists());
    assert_eq!(hash_files(&source.0), before);
}
