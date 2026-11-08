#![cfg(feature = "fault-injection")]
//! Process interruption retains the OS cache. Power-loss testing is separate.
use std::{path::Path, process::Command};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix};
#[path = "support/crash.rs"]
mod crash_support;
use crash_support::open_worker;

const POINTS: &[&str] = &[
    "segment.before_write",
    "segment.after_write",
    "segment.before_file_sync",
    "segment.after_file_sync",
    "segment.before_rename",
    "segment.after_rename",
    "segment.before_segments_dir_sync",
    "segment.after_segments_dir_sync",
    "descriptor.before_write",
    "descriptor.after_write",
    "descriptor.before_file_sync",
    "descriptor.after_file_sync",
    "descriptor.before_directory_sync",
    "descriptor.after_directory_sync",
    "pointer.PREVIOUS.before_write",
    "pointer.PREVIOUS.after_write",
    "pointer.PREVIOUS.before_file_sync",
    "pointer.PREVIOUS.after_file_sync",
    "pointer.PREVIOUS.before_rename",
    "pointer.PREVIOUS.after_rename",
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
    if point.starts_with("segment.") {
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

fn initialize(path: &Path) {
    let env = Environment::create(path).unwrap();
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

fn matrix(parent: &Path) {
    let root = parent.join(format!(
        "ulmdb-crash-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    for (index, point) in POINTS.iter().enumerate() {
        let database = root.join(index.to_string());
        initialize(&database);
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
    initialize(&database);
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
    matrix(&std::env::temp_dir());
}

#[test]
fn every_publication_boundary_reopens_a_complete_snapshot_on_ramdrive() {
    // This Linux qualification target requires the RAM-backed mount; absence
    // must fail rather than silently claiming a skipped platform passed.
    matrix(Path::new("/dev/shm"));
}

#[test]
fn native_publication_trace_matches_the_model_schedule() {
    let root = std::env::temp_dir().join(format!(
        "ulmdb-trace-{}",
        ulmdb_storage::new_identity().unwrap()
    ));
    std::fs::create_dir(&root).unwrap();
    let database = root.join("database");
    initialize(&database);
    let trace = root.join("trace");
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_worker", "--nocapture"])
        .env("ULMDB_CRASH_DATABASE", &database)
        .env("ULMDB_TRACE_FILE", &trace)
        .env_remove("ULMDB_CRASH_AT")
        .env_remove("ULMDB_FAIL_AT")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = std::fs::read_to_string(trace).unwrap();
    assert_eq!(
        events.lines().collect::<Vec<_>>(),
        &POINTS[..POINTS.len() - 1]
    );
    check_reopened(&database, true);
    std::fs::remove_dir_all(root).unwrap();
}

fn error_matrix(parent: &Path) {
    let root = parent.join(format!(
        "ulmdb-io-errors-{}",
        ulmdb_storage::new_identity().unwrap()
    ));
    std::fs::create_dir(&root).unwrap();
    for (index, point) in POINTS
        .iter()
        .filter(|point| **point != "transaction.after_commit")
        .enumerate()
    {
        let database = root.join(index.to_string());
        initialize(&database);
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
    error_matrix(&std::env::temp_dir());
}

#[test]
fn every_injected_io_error_preserves_atomicity_and_writer_poisoning_on_ramdrive() {
    error_matrix(Path::new("/dev/shm"));
}
