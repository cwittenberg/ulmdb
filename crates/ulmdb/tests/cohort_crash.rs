#![cfg(feature = "fault-injection")]
//! Cohort/Indexed: 15 abrupt cuts and 14 returned I/O failures on disk/tmpfs.
//! Process interruption retains the OS cache. Physical power loss is separate.
use std::{path::Path, process::Command};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix};
#[path = "support/crash.rs"]
mod crash_support;
use crash_support::open_worker;

const POINTS: &[&str] = &[
    "cohort.before_write",
    "cohort.after_write",
    "cohort.before_file_sync",
    "cohort.after_file_sync",
    "cohort.before_directory_sync",
    "cohort.after_directory_sync",
    "pointer.CURRENT.before_write",
    "pointer.CURRENT.after_write",
    "pointer.CURRENT.before_file_sync",
    "pointer.CURRENT.after_file_sync",
    "pointer.CURRENT.before_rename",
    "pointer.CURRENT.after_rename",
    "pointer.CURRENT.before_directory_sync",
    "pointer.CURRENT.after_directory_sync",
    "transaction.after_commit",
];
const HISTORY_ARCHIVE: &[&str] = &[
    "history_archive.before_file_sync",
    "history_archive.before_rename",
    "history_archive.before_directory_sync",
];

fn network() -> Prefix {
    Prefix::new(Address::V4(0xc000_0200), 24).unwrap()
}

/// Re-entered as a separate process by the matrix below; exits without running
/// transaction/environment destructors at the requested storage hook.
#[test]
fn crash_worker() {
    let Some(path) = std::env::var_os("ULMDB_CRASH_DATABASE") else {
        return;
    };
    let env = open_worker(path);
    // Only trace children request this override. Crash/error matrices retain
    // the actual default automatic-maintenance behavior.
    if std::env::var("ULMDB_COHORT_TRACE_MANUAL").is_ok_and(|value| value == "1") {
        assert!(std::env::var_os("ULMDB_TRACE_FILE").is_some());
        env.set_maintenance_policy(ulmdb::MaintenancePolicy::Disabled)
            .unwrap();
    }
    let mut write = env.begin_write().unwrap();
    write.put(b"kv", b"state", b"new").unwrap();
    write.insert_prefix(b"ip", network(), b"new").unwrap();
    write.delete_duplicates(b"dups", b"state").unwrap();
    write.insert_duplicate(b"dups", b"state", b"new").unwrap();
    assert_eq!(write.commit().unwrap(), 2);
    if std::env::var("ULMDB_CRASH_AT").is_ok_and(|p| p == "transaction.after_commit") {
        std::process::exit(86);
    }
}

#[test]
fn io_error_worker() {
    let Some(path) = std::env::var_os("ULMDB_IO_DATABASE") else {
        return;
    };
    let point = std::env::var("ULMDB_FAIL_AT").unwrap();
    let env = open_worker(path);
    let mut write = env.begin_write().unwrap();
    write.put(b"kv", b"state", b"new").unwrap();
    write.insert_prefix(b"ip", network(), b"new").unwrap();
    write.delete_duplicates(b"dups", b"state").unwrap();
    write.insert_duplicate(b"dups", b"state", b"new").unwrap();
    let outcome = write.commit();
    if point.starts_with("cohort.") {
        assert!(matches!(outcome, Err(Error::Io(_))), "{point}: {outcome:?}");
        // Failure before root publication has a known nonpublication outcome.
        // Acquiring/aborting another writer must remain possible.
        env.begin_write().unwrap().abort();
    } else {
        assert!(
            matches!(outcome, Err(Error::OutcomeUnknown)),
            "{point}: {outcome:?}"
        );
        assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
    }
    // Activation never occurred on these error paths. A reader still acquires
    // one complete old in-process snapshot, even if CURRENT reached the new one.
    assert_eq!(env.begin_read().sequence(), 1);
    assert_eq!(
        env.begin_read().get(b"kv", b"state").unwrap(),
        Some(b"old".as_slice())
    );
}

fn was_published(point: &str) -> bool {
    matches!(
        point,
        "pointer.CURRENT.after_rename"
            | "pointer.CURRENT.before_directory_sync"
            | "pointer.CURRENT.after_directory_sync"
            | "transaction.after_commit"
    )
}

fn initialize(path: &Path, mode: ulmdb::PublicationMode) {
    let env = Environment::create_with_publication(path, mode).unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    write.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
    write
        .create_database(b"dups", DatabaseKind::Duplicates { width: None })
        .unwrap();
    write.put(b"kv", b"state", b"old").unwrap();
    write.insert_prefix(b"ip", network(), b"old").unwrap();
    write.insert_duplicate(b"dups", b"state", b"old").unwrap();
    assert_eq!(write.commit().unwrap(), 1);
}

fn check_reopened(path: &Path, new: bool) {
    let env = Environment::open(path).unwrap();
    let read = env.begin_read();
    let expected: &[u8] = if new { b"new" } else { b"old" };
    assert_eq!(read.sequence(), if new { 2 } else { 1 });
    assert_eq!(read.get(b"kv", b"state").unwrap(), Some(expected));
    assert_eq!(
        read.exact_prefix(b"ip", network()).unwrap(),
        Some((network(), expected))
    );
    let duplicates = read.duplicates(b"dups").unwrap();
    assert_eq!(duplicates.count(b"state").unwrap(), 1);
    assert!(duplicates.contains(b"state", expected).unwrap());
    assert!(
        !duplicates
            .contains(b"state", if new { b"old" } else { b"new" })
            .unwrap()
    );
    // Orphan prepared segments/descriptors must not prevent a later commit.
    let mut write = env.begin_write().unwrap();
    write.put(b"kv", b"continued", b"yes").unwrap();
    assert_eq!(write.commit().unwrap(), read.sequence() + 1);
    assert_eq!(read.get(b"kv", b"continued").unwrap(), None);
    assert_eq!(
        env.begin_read().get(b"kv", b"continued").unwrap(),
        Some(b"yes".as_slice())
    );
}

fn matrix(parent: &Path, mode: ulmdb::PublicationMode) {
    let root = parent.join(format!(
        "ulmdb-cohort-crash-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    for (index, point) in POINTS.iter().enumerate() {
        let database = root.join(index.to_string());
        initialize(&database, mode);
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_worker", "--nocapture"])
            .env("ULMDB_CRASH_DATABASE", &database)
            .env("ULMDB_CRASH_AT", point)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(86),
            "unreached crash hook {point}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let published = was_published(point);
        check_reopened(&database, published);
    }
    // A fault-free child must acknowledge the same complete cross-index commit.
    let database = root.join("acknowledged");
    initialize(&database, mode);
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_worker", "--nocapture"])
        .env("ULMDB_CRASH_DATABASE", &database)
        .env_remove("ULMDB_CRASH_AT")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    check_reopened(&database, true);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn every_publication_boundary_reopens_a_complete_snapshot_on_disk() {
    matrix(&std::env::temp_dir(), ulmdb::PublicationMode::Cohort);
}

#[test]
fn every_publication_boundary_reopens_a_complete_snapshot_on_ramdrive() {
    // This Linux qualification target requires the RAM-backed mount; absence
    // must fail rather than silently claiming a skipped platform passed.
    matrix(Path::new("/dev/shm"), ulmdb::PublicationMode::Cohort);
}

#[test]
fn native_publication_trace_matches_the_model_schedule() {
    for automatic in [true, false] {
        for mode in [
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            native_trace(mode, automatic);
        }
    }
}
fn native_trace(mode: ulmdb::PublicationMode, automatic: bool) {
    let root = std::env::temp_dir().join(format!(
        "ulmdb-trace-{}",
        ulmdb_storage::new_identity().unwrap()
    ));
    std::fs::create_dir(&root).unwrap();
    let database = root.join("database");
    initialize(&database, mode);
    let trace = root.join("trace");
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_worker", "--nocapture"])
        .env("ULMDB_CRASH_DATABASE", &database)
        .env("ULMDB_TRACE_FILE", &trace)
        .env("ULMDB_COHORT_TRACE_MANUAL", if automatic { "0" } else { "1" })
        .env_remove("ULMDB_CRASH_AT")
        .env_remove("ULMDB_FAIL_AT")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(trace).unwrap();
    let events: Vec<_> = text.lines().collect();
    let business = &POINTS[..POINTS.len() - 1];
    let mut expected = business.to_vec();
    if automatic {
        expected.extend_from_slice(HISTORY_ARCHIVE);
    }
    assert_eq!(
        events, expected,
        "{mode:?} automatic={automatic}: exact business selection then optional archive"
    );
    assert_eq!(
        events[..business.len()]
            .iter()
            .filter(|event| event.ends_with("before_file_sync")
                || event.ends_with("before_directory_sync"))
            .count(),
        4,
        "{mode:?} automatic={automatic}: original business fences"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.ends_with("before_file_sync")
                || event.ends_with("before_directory_sync"))
            .count(),
        if automatic { 6 } else { 4 },
        "{mode:?} automatic={automatic}: all business and maintenance fences"
    );
    check_reopened(&database, true);
    std::fs::remove_dir_all(root).unwrap();
}

fn error_matrix(parent: &Path, mode: ulmdb::PublicationMode) {
    let root = parent.join(format!(
        "ulmdb-cohort-io-errors-{}",
        ulmdb_storage::new_identity().unwrap()
    ));
    std::fs::create_dir(&root).unwrap();
    for (index, point) in POINTS
        .iter()
        .filter(|point| **point != "transaction.after_commit")
        .enumerate()
    {
        let database = root.join(index.to_string());
        initialize(&database, mode);
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "io_error_worker", "--nocapture"])
            .env("ULMDB_IO_DATABASE", &database)
            .env("ULMDB_FAIL_AT", point)
            .env_remove("ULMDB_CRASH_AT")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{point}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        check_reopened(&database, was_published(point));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn every_injected_io_error_preserves_atomicity_and_writer_poisoning_on_disk() {
    error_matrix(&std::env::temp_dir(), ulmdb::PublicationMode::Cohort);
}

#[test]
fn every_injected_io_error_preserves_atomicity_and_writer_poisoning_on_ramdrive() {
    error_matrix(Path::new("/dev/shm"), ulmdb::PublicationMode::Cohort);
}

#[test]
fn indexed_every_publication_boundary_reopens_a_complete_snapshot_on_disk() {
    matrix(&std::env::temp_dir(), ulmdb::PublicationMode::Indexed);
}
#[test]
fn indexed_every_publication_boundary_reopens_a_complete_snapshot_on_ramdrive() {
    matrix(Path::new("/dev/shm"), ulmdb::PublicationMode::Indexed);
}
#[test]
fn indexed_injected_io_errors_preserve_atomicity_and_writer_poisoning_on_disk() {
    error_matrix(&std::env::temp_dir(), ulmdb::PublicationMode::Indexed);
}
#[test]
fn indexed_injected_io_errors_preserve_atomicity_and_writer_poisoning_on_ramdrive() {
    error_matrix(Path::new("/dev/shm"), ulmdb::PublicationMode::Indexed);
}
