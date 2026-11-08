use std::{
    fs,
    ops::Bound,
    path::{Path, PathBuf},
};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix, PublicationMode, ResidencyPolicy};
struct Dir(PathBuf);
impl Dir {
    fn new(parent: &Path) -> Self {
        let p = parent.join(format!(
            "ulmdb-range-env-{}-{}",
            std::process::id(),
            ulmdb_storage::new_identity().unwrap()
        ));
        Self(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn prefix4() -> Prefix {
    Prefix::new(Address::V4(0x0a000000), 8).unwrap()
}
fn prefix6() -> Prefix {
    Prefix::new(Address::V6(0x20010db8000000000000000000000000), 32).unwrap()
}
fn fill(env: &Environment) {
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
    w.create_database(b"v6", DatabaseKind::Ipv6).unwrap();
    for n in 0..512u64 {
        w.put(b"kv", &n.to_be_bytes(), &n.to_le_bytes()).unwrap();
    }
    w.insert_prefix(b"v4", prefix4(), b"initial4").unwrap();
    w.insert_prefix(b"v6", prefix6(), b"initial6").unwrap();
    assert_eq!(
        w.get(b"kv", &7u64.to_be_bytes()).unwrap(),
        Some(7u64.to_le_bytes().as_slice())
    );
    assert_eq!(w.commit().unwrap(), 1);
}
fn real(parent: &Path) {
    let d = Dir::new(parent);
    let env = Environment::create_with_publication(&d.0, PublicationMode::RangeV5).unwrap();
    assert!(env.is_range_backed());
    assert_eq!(env.publication_mode(), PublicationMode::RangeV5);
    assert_eq!(env.stats().format_minor, 5);
    fill(&env);
    let old = env.begin_read();
    let old_value = old.get(b"kv", &7u64.to_be_bytes()).unwrap().unwrap();
    let old_ip = old
        .longest_match(b"v4", Address::V4(0x0a010203))
        .unwrap()
        .unwrap()
        .1;
    let initial_len = fs::metadata(d.0.join("RANGE.pack")).unwrap().len();
    assert_eq!(env.begin_write().unwrap().commit().unwrap(), 1);
    assert_eq!(
        fs::metadata(d.0.join("RANGE.pack")).unwrap().len(),
        initial_len
    );
    {
        let mut abort = env.begin_write().unwrap();
        abort.put(b"kv", &7u64.to_be_bytes(), b"abort").unwrap();
        abort.insert_prefix(b"v6", prefix6(), b"abort").unwrap();
        abort.abort();
    }
    assert_eq!(
        fs::metadata(d.0.join("RANGE.pack")).unwrap().len(),
        initial_len
    );
    for turn in 2..=20u64 {
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", &7u64.to_be_bytes(), &turn.to_le_bytes())
            .unwrap();
        w.insert_prefix(b"v4", prefix4(), &turn.to_le_bytes())
            .unwrap();
        w.insert_prefix(b"v6", prefix6(), &turn.to_le_bytes())
            .unwrap();
        assert_eq!(w.commit().unwrap(), turn);
        let r = env.begin_read();
        assert_eq!(
            r.get(b"kv", &7u64.to_be_bytes()).unwrap(),
            Some(turn.to_le_bytes().as_slice())
        );
        assert_eq!(
            r.longest_match(b"v6", Address::V6(0x20010db8000000000000000000000042))
                .unwrap()
                .unwrap()
                .1,
            turn.to_le_bytes()
        );
        assert_eq!(old_value, 7u64.to_le_bytes());
        assert_eq!(old_ip, b"initial4");
    }
    let current = env.begin_read();
    assert_eq!(
        current
            .range(b"kv", Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()
            .count(),
        512
    );
    for n in 0..512u64 {
        let expected = if n == 7 { 20u64 } else { n };
        assert_eq!(
            current.get(b"kv", &n.to_be_bytes()).unwrap(),
            Some(expected.to_le_bytes().as_slice())
        );
        assert_eq!(
            old.get(b"kv", &n.to_be_bytes()).unwrap(),
            Some(n.to_le_bytes().as_slice())
        );
    }
    ulmdb::backup::verify(&env).unwrap();
    assert!(env.reclaim().is_err());
    assert!(env.compact().is_err());
    assert!(env.prune_inventory().is_err());
    assert!(env.upgrade_inventory().is_err());
    assert!(env.retain_snapshot("old", &old).is_err());
    assert!(ulmdb::replication::Cursor::from_read(&current).is_err());
    // The old borrow owns only an independent readonly OFD, never the writer flock.
    drop(env);
    let reopened = Environment::open(&d.0).unwrap();
    assert_eq!(reopened.stats().sequence, 20);
    assert_eq!(old_value, 7u64.to_le_bytes());
    assert_eq!(old_ip, b"initial4");
    assert_eq!(
        reopened
            .begin_read()
            .get(b"kv", &7u64.to_be_bytes())
            .unwrap(),
        Some(20u64.to_le_bytes().as_slice())
    );
    assert!(!d.0.join("CURRENT").exists());
    assert_eq!(fs::read_dir(d.0.join("cohorts")).unwrap().count(), 0);
    assert_eq!(fs::read_dir(d.0.join("segments")).unwrap().count(), 0);
}
#[test]
fn real_kv_ip_disk() {
    real(&std::env::temp_dir());
}
#[test]
fn real_kv_ip_tmpfs() {
    real(Path::new("/dev/shm"));
}
#[test]
fn compatibility_all_file_backends() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir::new(&std::env::temp_dir());
        let env = Environment::create_with_publication(&d.0, mode).unwrap();
        fill(&env);
        drop(env);
        let env = Environment::open(&d.0).unwrap();
        assert_eq!(env.publication_mode(), mode);
        assert_eq!(env.stats().sequence, 1);
    }
}
#[test]
fn denied_mapping_precedes_publication_and_is_reusable() {
    let d = Dir::new(Path::new("/dev/shm"));
    let env = Environment::create_with_residency(
        &d.0,
        PublicationMode::RangeV5,
        ResidencyPolicy::Prefault { max_bytes: 0 },
    )
    .unwrap();
    let length = fs::metadata(d.0.join("RANGE.pack")).unwrap().len();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"x", b"y").unwrap();
    assert!(matches!(w.commit(), Err(Error::LimitExceeded)));
    assert_eq!(env.stats().sequence, 0);
    assert_eq!(fs::metadata(d.0.join("RANGE.pack")).unwrap().len(), length);
    env.begin_write().unwrap().abort();
    drop(env);
    assert_eq!(Environment::open(&d.0).unwrap().stats().sequence, 0);
}
#[test]
fn selected_payload_corruption_has_no_fallback_or_mutation() {
    use ulmdb_platform::FileExt;
    let d = Dir::new(&std::env::temp_dir());
    let env = Environment::create_with_publication(&d.0, PublicationMode::RangeV5).unwrap();
    fill(&env);
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"extra", b"latest").unwrap();
    w.commit().unwrap();
    drop(env);
    let path = d.0.join("RANGE.pack");
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let mut latest = (0, 0);
    for offset in [65536, 131072] {
        let mut b = [0; 4096];
        file.read_exact_at(&mut b, offset).unwrap();
        let ordinal = u64::from_le_bytes(b[40..48].try_into().unwrap());
        if ordinal > latest.0 {
            latest = (ordinal, u64::from_le_bytes(b[32..40].try_into().unwrap()));
        }
    }
    file.write_all_at(&[0xaa], latest.1 + 8192 + 100).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let before = fs::read(&path).unwrap();
    assert!(matches!(Environment::open(&d.0), Err(Error::Corrupt(_))));
    assert_eq!(fs::read(path).unwrap(), before);
}
#[test]
fn unsupported_lifecycle_fails_before_any_storage_mutation() {
    let d = Dir::new(&std::env::temp_dir());
    let env = Environment::create_with_publication(&d.0, PublicationMode::RangeV5).unwrap();
    fill(&env);
    let read = env.begin_read();
    let before = fs::read(d.0.join("RANGE.pack")).unwrap();
    let marker = fs::read(d.0.join("BACKEND")).unwrap();
    let target = Dir::new(&std::env::temp_dir());
    for error in [
        env.reclaim().map(|_| ()),
        env.compact().map(|_| ()),
        env.prune_inventory().map(|_| ()),
        env.upgrade_inventory().map(|_| ()),
        env.upgrade_publication().map(|_| ()),
        env.retain_snapshot("pin", &read).map(|_| ()),
        env.open_retained("pin").map(|_| ()),
        env.retained_snapshots().map(|_| ()),
        env.release_snapshot("pin"),
        ulmdb::backup::export(&env, &target.0).map(|_| ()),
        ulmdb::backup::restore(&d.0, &target.0).map(|_| ()),
    ] {
        assert!(matches!(error, Err(Error::InvalidInput(_))), "{error:?}");
    }
    assert!(!target.0.exists());
    assert!(matches!(
        ulmdb::replication::Cursor::from_read(&read),
        Err(ulmdb::replication::ReplicationError::InvalidInput(_))
    ));
    assert_eq!(fs::read(d.0.join("RANGE.pack")).unwrap(), before);
    assert_eq!(fs::read(d.0.join("BACKEND")).unwrap(), marker);
    assert!(!d.0.join("CURRENT").exists());
}
#[test]
fn corrupt_active_head_blocks_commit_before_append() {
    use ulmdb_platform::FileExt;
    let d = Dir::new(&std::env::temp_dir());
    let env = Environment::create_with_publication(&d.0, PublicationMode::RangeV5).unwrap();
    fill(&env);
    let path = d.0.join("RANGE.pack");
    let f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let mut latest = (0, 0);
    for offset in [65536, 131072] {
        let mut b = [0; 4096];
        f.read_exact_at(&mut b, offset).unwrap();
        let ordinal = u64::from_le_bytes(b[40..48].try_into().unwrap());
        if ordinal > latest.0 {
            latest = (ordinal, offset);
        }
    }
    f.write_all_at(&[0xff], latest.1).unwrap();
    let before = fs::read(&path).unwrap();
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"x", b"y").unwrap();
    assert!(matches!(w.commit(), Err(Error::Corrupt(_))));
    assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
    assert_eq!(fs::read(path).unwrap(), before);
    assert_eq!(env.begin_read().sequence(), 1);
}

#[test]
fn cli_range_is_explicit_and_existing_commands_reopen_it() {
    use std::process::Command;
    let d = Dir::new(&std::env::temp_dir());
    for args in [
        vec!["demo-range-v5", d.0.to_str().unwrap()],
        vec!["check", d.0.to_str().unwrap()],
        vec!["get", d.0.to_str().unwrap(), "hostnames", "example.test"],
    ] {
        let o = Command::new(env!("CARGO_BIN_EXE_ulmdb"))
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }
    assert!(Environment::open(&d.0).unwrap().is_range_backed());
}
#[test]
fn overflow_duplicates_savepoint_handles_and_managed_mapping_share_range_contract() {
    let d = Dir::new(Path::new("/dev/shm"));
    let env = Environment::create_with_residency(
        &d.0,
        PublicationMode::RangeV5,
        ResidencyPolicy::Prefault {
            max_bytes: 16 * 1024 * 1024,
        },
    )
    .unwrap();
    let large = vec![0x5a; 131072];
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"dups", DatabaseKind::Duplicates { width: Some(8) })
        .unwrap();
    w.put(b"kv", b"large", &large).unwrap();
    for n in 0..64u64 {
        w.insert_duplicate(b"dups", b"key", &n.to_be_bytes())
            .unwrap();
    }
    w.commit().unwrap();
    let old = env.begin_read();
    let handle = old.open_database(b"kv").unwrap();
    let borrowed = old.get_handle(&handle, b"large").unwrap().unwrap();
    let mut w = env.begin_write().unwrap();
    {
        let mut s = w.savepoint().unwrap();
        s.put_handle(&handle, b"large", b"aborted").unwrap();
        s.delete_duplicates(b"dups", b"key").unwrap();
        s.abort().unwrap();
    }
    assert_eq!(
        w.get_handle(&handle, b"large").unwrap(),
        Some(large.as_slice())
    );
    w.put_handle(&handle, b"large", &vec![0x7b; 131072])
        .unwrap();
    w.delete_duplicate(b"dups", b"key", &7u64.to_be_bytes())
        .unwrap();
    w.commit().unwrap();
    assert_eq!(borrowed, large);
    assert_eq!(
        env.begin_read()
            .duplicates(b"dups")
            .unwrap()
            .count(b"key")
            .unwrap(),
        63
    );
    ulmdb::backup::verify(&env).unwrap();
    drop(env);
    let reopened = Environment::open_with_residency(
        &d.0,
        ResidencyPolicy::Prefault {
            max_bytes: 16 * 1024 * 1024,
        },
    )
    .unwrap();
    assert_eq!(borrowed, large);
    assert!(matches!(
        reopened.begin_read().get_handle(&handle, b"large"),
        Err(Error::StaleHandle)
    ));
    assert_eq!(
        reopened.begin_read().get(b"kv", b"large").unwrap().unwrap(),
        vec![0x7b; 131072]
    );
    assert_eq!(
        reopened
            .begin_read()
            .duplicates(b"dups")
            .unwrap()
            .count(b"key")
            .unwrap(),
        63
    );
}
#[test]
fn required_locked_range_maps_use_actual_os_locks_and_old_reader_lifetimes() {
    fn vm() -> u64 {
        fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .find_map(|l| l.strip_prefix("VmLck:"))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            * 1024
    }
    let before = vm();
    let d = Dir::new(Path::new("/dev/shm"));
    let env = Environment::create_with_residency(
        &d.0,
        PublicationMode::RangeV5,
        ResidencyPolicy::RequiredLocked { max_bytes: 1048576 },
    )
    .unwrap();
    fill(&env);
    let old = env.begin_read();
    let borrowed = old.get(b"kv", &7u64.to_be_bytes()).unwrap().unwrap();
    let report = old.residency_report();
    assert!(report.locked_bytes > 0);
    assert_eq!(report.locked_bytes, report.page_rounded_bytes.unwrap());
    assert!(vm() >= before + report.locked_bytes);
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", &7u64.to_be_bytes(), b"changed").unwrap();
    w.commit().unwrap();
    assert!(env.residency_status().live_locked_bytes >= report.locked_bytes);
    drop(env);
    assert_eq!(borrowed, 7u64.to_le_bytes());
    assert!(vm() >= before + report.locked_bytes);
    drop(old);
    assert_eq!(vm(), before);
}
