//! Real resident API abrupt process/returned-IO boundaries, not hardware power loss.
#![cfg(feature = "fault-injection")]
use std::{
    path::{Path, PathBuf},
    process::Command,
};
use ulmdb::{
    Address, DatabaseKind, Error, Prefix,
    resident::{Environment, Options},
};
use ulmdb_storage::resident::Config;
const POINTS: [&str; 11] = [
    "resident.commit.before_append",
    "resident.commit.half_record",
    "resident.commit.after_append",
    "resident.commit.before_data_sync",
    "resident.commit.after_data_sync",
    "resident.commit.before_witness",
    "resident.commit.half_witness",
    "resident.commit.after_witness",
    "resident.commit.before_witness_sync",
    "resident.commit.after_witness_sync",
    "resident.commit.before_ack",
];
fn options() -> Options {
    Options {
        pool: Config {
            kv_pages: 16,
            tiny_pages: 16,
            max_reserved_bytes: 64 << 20,
            max_metadata_bytes: 64 << 20,
            max_views: 32,
        },
        wal_capacity: 4 << 20,
        graph_read_budget: 1_000_000,
        auto_collect_below: 0,
    }
}
fn update(e: &Environment, value: &[u8], create: bool) -> ulmdb::Result<u64> {
    let mut w = e.begin_write()?;
    if create {
        w.create_database(b"kv", DatabaseKind::Kv)?;
        w.create_database(b"v4", DatabaseKind::Ipv4)?;
        w.create_database(b"v6", DatabaseKind::Ipv6)?;
    }
    w.put(b"kv", b"state", value)?;
    w.insert_prefix(b"v4", Prefix::new(Address::V4(0), 0).unwrap(), value)?;
    w.insert_prefix(b"v6", Prefix::new(Address::V6(0), 0).unwrap(), value)?;
    w.commit()
}
fn worker() -> bool {
    let Some(path) = std::env::var_os("ULMDB_RESIDENT_CHILD") else {
        return false;
    };
    let os_case = std::env::var("ULMDB_RESIDENT_OS_CASE").ok();
    let opened = Environment::open(&path, options());
    if os_case.as_deref() == Some("open_clear") {
        assert!(matches!(opened, Err(Error::Io(_))));
        println!("RESIDENT_CHILD_DONE");
        return true;
    }
    let env = opened.unwrap();
    let old = env.begin_read();
    let held = old.get(b"kv", b"state").unwrap().unwrap();
    let result = update(&env, b"new", false);
    let unknown = if let Some(case) = os_case {
        case != "data"
    } else if let Ok(p) = std::env::var("ULMDB_RESIDENT_FAIL_AT") {
        POINTS.iter().position(|x| *x == p).unwrap() >= 5
    } else {
        false
    };
    if unknown {
        assert!(matches!(result, Err(Error::OutcomeUnknown)));
        assert!(env.pool_stats().unwrap().poisoned);
        assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
    } else if std::env::var_os("ULMDB_RESIDENT_FAIL_AT").is_some()
        || std::env::var_os("ULMDB_RESIDENT_OS_CASE").is_some()
    {
        assert!(matches!(result, Err(Error::Io(_))));
        assert!(!env.pool_stats().unwrap().poisoned);
        assert!(env.begin_write().is_ok());
    } else {
        result.unwrap();
    }
    assert_eq!(held, b"old");
    assert_eq!(old.get(b"kv", b"state").unwrap(), Some(b"old".as_slice()));
    println!("RESIDENT_CHILD_DONE");
    true
}
fn basepath(base: &Path) -> PathBuf {
    base.join(format!(
        "ulmdb-resident-fault-{:032x}",
        ulmdb_storage::new_identity().unwrap()
    ))
}
fn check_reopened(path: &Path, old: &ulmdb::resident::ReadTxn) {
    let e = Environment::open(path, options()).unwrap();
    let r = e.begin_read();
    let value = r.get(b"kv", b"state").unwrap().unwrap();
    assert!(value == b"old" || value == b"new");
    assert_eq!(
        r.longest_match(b"v4", Address::V4(1)).unwrap().unwrap().1,
        value
    );
    assert_eq!(
        r.longest_match(b"v6", Address::V6(1)).unwrap().unwrap().1,
        value
    );
    assert_eq!(old.get(b"kv", b"state").unwrap(), Some(b"old".as_slice()));
}
#[test]
fn every_real_commit_cut_and_returned_error_preserves_complete_kv_ip_snapshots() {
    if worker() {
        return;
    }
    for base in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        for exit in [false, true] {
            for point in POINTS {
                let path = basepath(&base);
                let e = Environment::create(&path, options()).unwrap();
                update(&e, b"old", true).unwrap();
                let old = e.begin_read();
                drop(e);
                let output=Command::new("timeout").args(["--signal=KILL","30s"]).arg(std::env::current_exe().unwrap()).args(["--exact","every_real_commit_cut_and_returned_error_preserves_complete_kv_ip_snapshots","--nocapture"]).env("ULMDB_RESIDENT_CHILD",&path).env(if exit{"ULMDB_RESIDENT_EXIT_AT"}else{"ULMDB_RESIDENT_FAIL_AT"},point).output().unwrap();
                assert_eq!(
                    output.status.code(),
                    Some(if exit { 86 } else { 0 }),
                    "{point}: {} {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                if !exit {
                    assert!(
                        String::from_utf8_lossy(&output.stdout).contains("RESIDENT_CHILD_DONE")
                    );
                }
                check_reopened(&path, &old);
                drop(old);
                std::fs::remove_dir_all(path).unwrap();
            }
        }
    }
}
#[test]
fn actual_fdatasync_errors_cover_clear_data_witness_and_failed_reset() {
    if worker() {
        return;
    }
    for base in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        for (case, when) in [
            ("open_clear", "1"),
            ("data", "2"),
            ("witness", "3"),
            ("reset", "2+"),
        ] {
            let path = basepath(&base);
            let e = Environment::create(&path, options()).unwrap();
            update(&e, b"old", true).unwrap();
            let old = e.begin_read();
            drop(e);
            let trace = path.join("fdatasync.strace");
            let output = Command::new("timeout")
                .args([
                    "--signal=KILL",
                    "30s",
                    "strace",
                    "-f",
                    "-e",
                    "trace=fdatasync",
                    "-e",
                    &format!("inject=fdatasync:error=EIO:when={when}"),
                    "-o",
                ])
                .arg(&trace)
                .arg(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "actual_fdatasync_errors_cover_clear_data_witness_and_failed_reset",
                    "--nocapture",
                ])
                .env("ULMDB_RESIDENT_CHILD", &path)
                .env("ULMDB_RESIDENT_OS_CASE", case)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{case}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("RESIDENT_CHILD_DONE"));
            let raw = std::fs::read_to_string(&trace).unwrap();
            assert!(raw.contains("EIO") && raw.contains("INJECTED"));
            check_reopened(&path, &old);
            drop(old);
            std::fs::remove_dir_all(path).unwrap();
        }
    }
}
