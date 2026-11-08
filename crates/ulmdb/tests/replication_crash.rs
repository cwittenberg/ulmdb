#![cfg(feature = "fault-injection")]
//! Process-crash and returned-I/O checks; physical power loss and second hosts
//! remain separate qualification requirements.
use std::{
    fs,
    io::Cursor as Input,
    path::{Path, PathBuf},
    process::Command,
};
use ulmdb::replication::{
    self, ApplyOutcome, Cursor, PublisherKey, ReplicationError as RError, TransferLimits,
};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix, PublicationMode};
#[path = "support/crash.rs"]
mod crash_support;
use crash_support::open_worker;
const POINTS: &[&str] = &[
    "replica.before_validate",
    "replica.after_validate",
    "replica.before_import",
    "replica.before_file_import",
    "replica.after_file_import",
    "replica.before_directory_sync",
    "replica.after_directory_sync",
    "replica.after_import",
    "replica.before_prepare",
    "replica.after_prepare",
    "replica.before_publish",
    "pointer.CURRENT.before_write",
    "pointer.CURRENT.after_write",
    "pointer.CURRENT.before_file_sync",
    "pointer.CURRENT.after_file_sync",
    "pointer.CURRENT.before_rename",
    "pointer.CURRENT.after_rename",
    "pointer.CURRENT.before_directory_sync",
    "pointer.CURRENT.after_directory_sync",
    "replica.after_publish",
    "replica.before_activate",
    "replica.after_activate",
    "replica.ack",
];
const ROLE_POINTS: &[&str] = &[
    "replica.role.before_write",
    "replica.role.after_write",
    "replica.role.before_file_sync",
    "replica.role.after_file_sync",
    "replica.role.before_directory_sync",
    "replica.role.after_directory_sync",
];
fn prefix() -> Prefix {
    Prefix::new(Address::V4(0xc000_0200), 24).unwrap()
}
fn inspect(env: &Environment, new: bool) {
    assert!(env.is_replica());
    let read = env.begin_read();
    let value: &[u8] = if new { b"new" } else { b"old" };
    assert_eq!(read.sequence(), if new { 2 } else { 1 });
    assert_eq!(read.get(b"kv", b"state").unwrap(), Some(value));
    assert_eq!(
        read.exact_prefix(b"ip", prefix()).unwrap(),
        Some((prefix(), value))
    );
    assert_eq!(read.duplicate_count(b"dups", b"state").unwrap(), 1);
    assert!(
        read.duplicates(b"dups")
            .unwrap()
            .contains(b"state", value)
            .unwrap()
    );
    assert!(matches!(env.begin_write(), Err(Error::ReadOnly)));
}
fn published(point: &str) -> bool {
    matches!(
        point,
        "pointer.CURRENT.after_rename"
            | "pointer.CURRENT.before_directory_sync"
            | "pointer.CURRENT.after_directory_sync"
            | "replica.after_publish"
            | "replica.before_activate"
            | "replica.after_activate"
            | "replica.ack"
    )
}
fn uncertain(point: &str) -> bool {
    point.starts_with("pointer.CURRENT.")
        || matches!(
            point,
            "replica.after_publish" | "replica.before_activate" | "replica.after_activate"
        )
}
#[test]
fn replication_worker() {
    let Some(path) = std::env::var_os("ULMDB_REPLICA_WORKER") else {
        return;
    };
    let c = ulmdb_storage::read_current(std::path::Path::new(
        &std::env::var_os("ULMDB_PRIMARY").unwrap(),
    ))
    .unwrap();
    let key = PublisherKey::new(7, c.uuid, c.epoch, [0x37; 32]).unwrap();
    let bytes = fs::read(std::env::var_os("ULMDB_FRAME").unwrap()).unwrap();
    let stage = std::env::var_os("ULMDB_STAGE").unwrap();
    let frame = replication::receive(
        &mut Input::new(bytes),
        &key,
        TransferLimits::default(),
        stage,
    )
    .unwrap();
    let bootstrap = std::env::var_os("ULMDB_BOOTSTRAP").is_some();
    let point = std::env::var("ULMDB_FAIL_AT").ok();
    if bootstrap {
        let result = replication::bootstrap(&frame, path);
        if point.is_some() {
            assert!(result.is_err());
        } else {
            result.unwrap();
        }
    } else {
        let replica = open_worker(&path);
        let pinned = replica.begin_read();
        let result = replica.apply_replication(&frame);
        if let Some(point) = point {
            if uncertain(&point) {
                assert!(
                    matches!(result, Err(RError::Engine(Error::OutcomeUnknown))),
                    "{point}: {result:?}"
                );
                assert!(matches!(
                    replica.apply_replication(&frame),
                    Err(RError::Engine(Error::OutcomeUnknown))
                ));
            } else {
                assert!(result.is_err(), "{point}: {result:?}");
            }
            // Activation hooks after the swap may return Unknown with a complete
            // new in-process root; the retained reader always stays old.
            inspect(&replica, point == "replica.after_activate");
            assert_eq!(
                pinned.get(b"kv", b"state").unwrap(),
                Some(b"old".as_slice())
            );
        } else {
            assert_eq!(result.unwrap().outcome, ApplyOutcome::Activated);
        }
    }
    if std::env::var("ULMDB_CRASH_AT").is_ok_and(|p| p == "replica.ack") {
        std::process::exit(86);
    }
}
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn matrix(parent: &Path, mode: PublicationMode) {
    let d = Directory(parent.join(format!(
        "ulmdb-replication-crash-{}",
        ulmdb_storage::new_identity().unwrap()
    )));
    fs::create_dir(&d.0).unwrap();
    let primary_path = d.0.join("primary");
    let primary = Environment::create_with_publication(&primary_path, mode).unwrap();
    let mut w = primary.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
    w.create_database(b"dups", DatabaseKind::Duplicates { width: None })
        .unwrap();
    w.put(b"kv", b"state", b"old").unwrap();
    w.insert_prefix(b"ip", prefix(), b"old").unwrap();
    w.insert_duplicate(b"dups", b"state", b"old").unwrap();
    w.commit().unwrap();
    let first = primary.begin_read();
    let c = Cursor::from_read(&first).unwrap();
    let key = PublisherKey::new(7, c.uuid, c.epoch, [0x37; 32]).unwrap();
    let mut full = Vec::new();
    replication::send_snapshot(&primary, &first, None, &key, &mut full).unwrap();
    let full_verified = replication::receive(
        &mut Input::new(&full),
        &key,
        TransferLimits::default(),
        &d.0,
    )
    .unwrap();
    let mut w = primary.begin_write().unwrap();
    w.put(b"kv", b"state", b"new").unwrap();
    w.insert_prefix(b"ip", prefix(), b"new").unwrap();
    w.delete_duplicates(b"dups", b"state").unwrap();
    w.insert_duplicate(b"dups", b"state", b"new").unwrap();
    w.commit().unwrap();
    let mut delta = Vec::new();
    replication::send_snapshot(
        &primary,
        &primary.begin_read(),
        Some(&first),
        &key,
        &mut delta,
    )
    .unwrap();
    let frame_path = d.0.join("delta.frame");
    fs::write(&frame_path, &delta).unwrap();
    for (index, point) in POINTS.iter().enumerate() {
        for fail in [false, true] {
            if fail && *point == "replica.ack" {
                continue;
            }
            let path = d.0.join(format!("{index}-{fail}"));
            drop(replication::bootstrap(&full_verified, &path).unwrap());
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "replication_worker", "--nocapture"])
                .env("ULMDB_REPLICA_WORKER", &path)
                .env("ULMDB_PRIMARY", &primary_path)
                .env("ULMDB_FRAME", &frame_path)
                .env("ULMDB_STAGE", &d.0)
                .env(
                    if fail {
                        "ULMDB_FAIL_AT"
                    } else {
                        "ULMDB_CRASH_AT"
                    },
                    point,
                )
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(if fail { 0 } else { 86 }),
                "{point} fail={fail}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let replica = Environment::open(&path).unwrap();
            inspect(&replica, published(point));
            let verified = replication::receive(
                &mut Input::new(&delta),
                &key,
                TransferLimits::default(),
                &d.0,
            )
            .unwrap();
            let report = replica.apply_replication(&verified).unwrap();
            assert_eq!(
                report.outcome,
                if published(point) {
                    ApplyOutcome::AlreadyCurrent
                } else {
                    ApplyOutcome::Activated
                }
            );
            inspect(&replica, true);
        }
    }
    let full_path = d.0.join("full.frame");
    fs::write(&full_path, &full).unwrap();
    for (index, point) in POINTS.iter().chain(ROLE_POINTS).enumerate() {
        let path = d.0.join(format!("bootstrap-{index}"));
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "replication_worker", "--nocapture"])
            .env("ULMDB_REPLICA_WORKER", &path)
            .env("ULMDB_PRIMARY", &primary_path)
            .env("ULMDB_FRAME", &full_path)
            .env("ULMDB_STAGE", &d.0)
            .env("ULMDB_BOOTSTRAP", "1")
            .env("ULMDB_CRASH_AT", point)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(86),
            "bootstrap {point}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if path.join("CURRENT").exists() {
            let replica = Environment::open(&path).unwrap();
            inspect(&replica, false);
            assert_eq!(fs::metadata(path.join("REPLICA")).unwrap().len(), 64);
        } else {
            assert!(!published(point));
        }
    }
}
#[test]
fn crash_and_io_matrix_disk() {
    matrix(&std::env::temp_dir(), PublicationMode::Cohort);
}
#[test]
fn crash_and_io_matrix_ramdrive() {
    if Path::new("/dev/shm").is_dir() {
        matrix(Path::new("/dev/shm"), PublicationMode::Cohort);
    }
}

#[test]
fn indexed_crash_and_io_matrix_disk() {
    matrix(&std::env::temp_dir(), PublicationMode::Indexed);
}
#[test]
fn indexed_crash_and_io_matrix_ramdrive() {
    if Path::new("/dev/shm").is_dir() {
        matrix(Path::new("/dev/shm"), PublicationMode::Indexed);
    }
}
