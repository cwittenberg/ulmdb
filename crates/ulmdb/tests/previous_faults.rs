#![cfg(feature = "fault-injection")]
//! Real subprocess IO failures/process exits; no power-loss durability claim.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
use ulmdb::{DatabaseKind, Environment, Error, PreviousError, PreviousSession, PublicationMode};
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
const LEGACY: &[&str] = &[
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
];
const COHORT: &[&str] = &[
    "cohort.before_write",
    "cohort.after_write",
    "cohort.before_file_sync",
    "cohort.after_file_sync",
    "cohort.before_directory_sync",
    "cohort.after_directory_sync",
];
const CURRENT: &[&str] = &[
    "pointer.CURRENT.before_write",
    "pointer.CURRENT.after_write",
    "pointer.CURRENT.before_file_sync",
    "pointer.CURRENT.after_file_sync",
    "pointer.CURRENT.before_rename",
    "pointer.CURRENT.after_rename",
    "pointer.CURRENT.before_directory_sync",
    "pointer.CURRENT.after_directory_sync",
];
const PREVIOUS: &[&str] = &[
    "pointer.PREVIOUS.before_write",
    "pointer.PREVIOUS.after_write",
    "pointer.PREVIOUS.before_file_sync",
    "pointer.PREVIOUS.after_file_sync",
    "pointer.PREVIOUS.before_rename",
    "pointer.PREVIOUS.after_rename",
];
const AFTER: &[&str] = &[
    "previous.before_publish",
    "previous.after_publish",
    "previous.before_activate",
    "previous.after_activate",
];
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-previous-fault-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn seed(path: &Path, mode: PublicationMode) {
    let env = Environment::create_with_publication(path, mode).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"A").unwrap();
    assert_eq!(w.commit().unwrap(), 1);
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"key", b"B").unwrap();
    assert_eq!(w.commit().unwrap(), 2);
}
fn worker(path: &Path, role: &str) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "previous_process_worker", "--nocapture"])
        .env("ULMDB_PREVIOUS_PATH", path)
        .env("ULMDB_PREVIOUS_ROLE", role)
        .env_remove("ULMDB_FAIL_AT")
        .env_remove("ULMDB_CRASH_AT")
        .env_remove("ULMDB_TRACE_FILE");
    c
}
fn output(mut c: Command) -> Output {
    c.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            let out = child.wait_with_output().unwrap();
            panic!(
                "previous worker deadline\n{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn success(out: Output) {
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
fn inspect(path: &Path, allowed: &[&[u8]]) {
    let env = Environment::open(path).unwrap();
    let read = env.begin_read();
    assert_eq!(read.sequence(), 2);
    let value = read.get(b"kv", b"key").unwrap().unwrap();
    assert!(allowed.contains(&value), "selected {value:?}");
    let previous = ulmdb_storage::read_previous(path).unwrap().unwrap();
    assert_eq!(previous.sequence, 1);
    ulmdb::backup::verify(&env).unwrap();
    env.reclaim().unwrap();
    assert!(allowed.contains(&read.get(b"kv", b"key").unwrap().unwrap()));
}
#[test]
fn previous_process_worker() {
    let Some(path) = std::env::var_os("ULMDB_PREVIOUS_PATH") else {
        return;
    };
    let path = PathBuf::from(path);
    let deadline = Instant::now() + Duration::from_secs(1);
    let session = loop {
        match PreviousSession::open(&path) {
            Ok(s) => break s,
            Err(PreviousError::Engine(Error::Busy)) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(1))
            }
            Err(e) => panic!("previous open: {e:?}"),
        }
    };
    let head = fs::read(path.join("CURRENT")).unwrap();
    let a = session.begin_read();
    let borrowed = a.get(b"kv", b"key").unwrap().unwrap();
    let mut w = session.begin_write().unwrap();
    w.put(b"kv", b"key", b"C").unwrap();
    let role = std::env::var("ULMDB_PREVIOUS_ROLE").unwrap();
    if role == "prepare-error" {
        assert!(matches!(w.prepare(), Err(Error::Io(_))));
        assert!(session.is_previous());
        assert_eq!(fs::read(path.join("CURRENT")).unwrap(), head);
        assert_eq!(borrowed, b"A");
        session.begin_write().unwrap().abort();
        return;
    }
    let prepared = w.prepare().unwrap();
    assert_eq!(fs::read(path.join("CURRENT")).unwrap(), head);
    assert!(session.is_previous());
    if role == "die-prepared" {
        std::process::exit(86)
    }
    if role == "selection-error" {
        assert!(matches!(prepared.commit(), Err(Error::OutcomeUnknown)));
        assert!(session.is_previous());
        assert!(matches!(session.begin_write(), Err(Error::OutcomeUnknown)));
        assert_eq!(borrowed, b"A");
        return;
    }
    if role == "trace" {
        use std::io::Write;
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(std::env::var_os("ULMDB_TRACE_FILE").unwrap())
            .unwrap();
        writeln!(f, "test.after_prepare").unwrap();
        assert_eq!(prepared.commit().unwrap(), 2);
        assert!(!session.is_previous());
        writeln!(f, "test.after_commit").unwrap();
        return;
    }
    assert_eq!(prepared.commit().unwrap(), 2);
    assert!(!session.is_previous());
}
#[test]
fn each_preparation_io_failure_keeps_b_and_session_a_unpoisoned() {
    for mode in MODES {
        for point in if mode == PublicationMode::Legacy {
            LEGACY
        } else {
            COHORT
        } {
            let d = Directory::new();
            seed(&d.0, mode);
            let head = fs::read(d.0.join("CURRENT")).unwrap();
            let mut c = worker(&d.0, "prepare-error");
            c.env("ULMDB_FAIL_AT", point);
            success(output(c));
            assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), head);
            inspect(&d.0, &[b"B"]);
        }
    }
}
#[test]
fn every_selection_and_activation_io_failure_is_uncertain_and_keeps_mode_pending() {
    for mode in MODES {
        let points = CURRENT
            .iter()
            .chain(
                (mode == PublicationMode::Legacy)
                    .then_some(PREVIOUS)
                    .into_iter()
                    .flatten(),
            )
            .chain(AFTER);
        for point in points {
            let d = Directory::new();
            seed(&d.0, mode);
            let mut c = worker(&d.0, "selection-error");
            c.env("ULMDB_FAIL_AT", point);
            success(output(c));
            inspect(&d.0, &[b"B", b"C"]);
        }
    }
}
#[test]
fn process_exit_at_all_prepare_select_and_activate_boundaries_reopens_complete_b_or_c() {
    for mode in MODES {
        let prep = if mode == PublicationMode::Legacy {
            LEGACY
        } else {
            COHORT
        };
        let points = prep
            .iter()
            .chain(CURRENT)
            .chain(
                (mode == PublicationMode::Legacy)
                    .then_some(PREVIOUS)
                    .into_iter()
                    .flatten(),
            )
            .chain(AFTER);
        for point in points {
            let d = Directory::new();
            seed(&d.0, mode);
            let mut c = worker(&d.0, "commit");
            c.env("ULMDB_CRASH_AT", point);
            assert_eq!(output(c).status.code(), Some(86), "{mode:?}/{point}");
            inspect(
                &d.0,
                if prep.contains(point) {
                    &[b"B"]
                } else {
                    &[b"B", b"C"]
                },
            );
        }
    }
}
#[test]
fn dirty_prepared_process_death_keeps_b_and_releases_writer_and_pin_ownership() {
    for mode in MODES {
        let d = Directory::new();
        seed(&d.0, mode);
        assert_eq!(output(worker(&d.0, "die-prepared")).status.code(), Some(86));
        inspect(&d.0, &[b"B"]);
        let session = PreviousSession::open(&d.0).unwrap();
        session.begin_write().unwrap().abort();
    }
}
#[test]
fn trace_prepares_without_head_writes_then_selects_c_with_a_predecessor() {
    for mode in MODES {
        let d = Directory::new();
        seed(&d.0, mode);
        let trace = std::env::temp_dir().join(format!(
            "previous-trace-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        let mut c = worker(&d.0, "trace");
        c.env("ULMDB_TRACE_FILE", &trace);
        success(output(c));
        let text = fs::read_to_string(&trace).unwrap();
        fs::remove_file(trace).unwrap();
        let (before, after) = text.split_once("test.after_prepare\n").unwrap();
        assert!(!before.contains("pointer.CURRENT"));
        assert!(!before.contains("pointer.PREVIOUS"));
        assert!(after.contains("pointer.CURRENT.after_directory_sync"));
        assert_eq!(
            after.contains("pointer.PREVIOUS.before_write"),
            mode == PublicationMode::Legacy
        );
        assert!(after.contains("previous.after_activate"));
        inspect(&d.0, &[b"C"]);
    }
}
