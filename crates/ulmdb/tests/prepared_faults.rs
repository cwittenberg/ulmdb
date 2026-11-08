#![cfg(feature = "fault-injection")]
//! Real subprocess faults and phase traces; process death is not power loss.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};
use ulmdb::{DatabaseKind, Environment, Error, PublicationMode};
#[path = "support/crash.rs"]
mod crash_support;

const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
const LEGACY_PREPARE: &[&str] = &[
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
const COHORT_PREPARE: &[&str] = &[
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
const HISTORY_ARCHIVE: &[&str] = &[
    "history_archive.before_file_sync",
    "history_archive.before_rename",
    "history_archive.before_directory_sync",
];
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-prepare-fault-{}-{}",
            std::process::id(),
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn seed(path: &Path, mode: PublicationMode) {
    let env = Environment::create_with_publication(path, mode).unwrap();
    let mut writer = env.begin_write().unwrap();
    writer.create_database(b"kv", DatabaseKind::Kv).unwrap();
    writer.put(b"kv", b"key", b"old").unwrap();
    writer.commit().unwrap();
}
fn worker(path: &Path, role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "prepared_process_worker", "--nocapture"])
        .env("ULMDB_PREPARED_DATABASE", path)
        .env("ULMDB_PREPARED_ROLE", role)
        .env_remove("ULMDB_FAIL_AT")
        .env_remove("ULMDB_CRASH_AT")
        .env_remove("ULMDB_TRACE_FILE");
    command
}
fn assert_success(output: std::process::Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn assert_selected(path: &Path, sequence: u64, value: &[u8]) {
    let env = Environment::open(path).unwrap();
    assert_eq!(env.begin_read().sequence(), sequence);
    assert_eq!(env.begin_read().get(b"kv", b"key").unwrap(), Some(value));
}

#[test]
fn prepared_process_worker() {
    let Some(path) = std::env::var_os("ULMDB_PREPARED_DATABASE") else {
        return;
    };
    let env = crash_support::open_worker(&path);
    let role = std::env::var("ULMDB_PREPARED_ROLE").unwrap();
    if role == "trace-manual" {
        env.set_maintenance_policy(ulmdb::MaintenancePolicy::Disabled)
            .unwrap();
    }
    let mut writer = env.begin_write().unwrap();
    writer.put(b"kv", b"key", b"new").unwrap();
    if role == "prepare-error" {
        assert!(matches!(writer.prepare(), Err(Error::Io(_))));
        assert_eq!(env.begin_read().sequence(), 1);
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"old".as_slice())
        );
        env.begin_write().unwrap().abort();
        return;
    }
    let prepared = writer.prepare().unwrap();
    if role == "die-prepared" {
        std::process::exit(86);
    }
    if role == "selection-error" {
        assert!(matches!(prepared.commit(), Err(Error::OutcomeUnknown)));
        assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
        assert_eq!(env.begin_read().sequence(), 1);
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"old".as_slice())
        );
        return;
    }
    let trace = PathBuf::from(std::env::var_os("ULMDB_TRACE_FILE").unwrap());
    writeln!(
        fs::OpenOptions::new().append(true).open(&trace).unwrap(),
        "test.after_prepare"
    )
    .unwrap();
    assert_eq!(prepared.commit().unwrap(), 2);
    writeln!(
        fs::OpenOptions::new().append(true).open(trace).unwrap(),
        "test.after_commit"
    )
    .unwrap();
}

#[test]
fn successful_phase_traces_separate_immutable_preparation_from_selection() {
    let root = Directory::new();
    for role in ["trace", "trace-manual"] {
        for (index, mode) in MODES.into_iter().enumerate() {
            let path = root.0.join(format!("{role}-{index}"));
            seed(&path, mode);
            let trace = root.0.join(format!("{role}-{index}.trace"));
            assert_success(
                worker(&path, role)
                    .env("ULMDB_TRACE_FILE", &trace)
                    .output()
                    .unwrap(),
            );
            let trace = fs::read_to_string(trace).unwrap();
            let (before, after) = trace.split_once("test.after_prepare\n").unwrap();
            assert_eq!(
                before.lines().collect::<Vec<_>>(),
                if mode == PublicationMode::Legacy {
                    LEGACY_PREPARE
                } else {
                    COHORT_PREPARE
                },
                "{mode:?} {role} preparation"
            );
            // Automatic maintenance archives prior cohort metadata only after
            // selecting CURRENT. It never repeats prepared business assets.
            let archive = if role == "trace" && mode != PublicationMode::Legacy {
                HISTORY_ARCHIVE
            } else {
                &[]
            };
            let expected: Vec<_> = (if mode == PublicationMode::Legacy {
                PREVIOUS
            } else {
                &[]
            })
            .iter()
            .copied()
            .chain(CURRENT.iter().copied())
            .chain(archive.iter().copied())
            .chain(["test.after_commit"])
            .collect();
            assert_eq!(
                after.lines().collect::<Vec<_>>(),
                expected,
                "{mode:?} {role}"
            );
            assert_selected(&path, 2, b"new");
        }
    }
}

#[test]
fn each_explicit_prepare_io_failure_preserves_old_head_and_unpoisoned_retry() {
    let root = Directory::new();
    for (index, mode) in MODES.into_iter().enumerate() {
        for (point_index, point) in (if mode == PublicationMode::Legacy {
            LEGACY_PREPARE
        } else {
            COHORT_PREPARE
        })
        .iter()
        .enumerate()
        {
            let path = root.0.join(format!("{index}-{point_index}"));
            seed(&path, mode);
            let current = fs::read(path.join("CURRENT")).unwrap();
            let previous = fs::read(path.join("PREVIOUS")).ok();
            assert_success(
                worker(&path, "prepare-error")
                    .env("ULMDB_FAIL_AT", point)
                    .output()
                    .unwrap(),
            );
            assert_eq!(
                fs::read(path.join("CURRENT")).unwrap(),
                current,
                "{mode:?} {point}"
            );
            assert_eq!(fs::read(path.join("PREVIOUS")).ok(), previous);
            let env = Environment::open(&path).unwrap();
            assert_eq!(env.begin_read().sequence(), 1);
            let mut retry = env.begin_write().unwrap();
            retry.put(b"kv", b"key", b"retry").unwrap();
            assert_eq!(retry.commit().unwrap(), 2);
        }
    }
}

#[test]
fn each_final_selection_io_failure_preserves_uncertainty_and_poisoning() {
    let root = Directory::new();
    for (index, mode) in MODES.into_iter().enumerate() {
        let points = (if mode == PublicationMode::Legacy {
            PREVIOUS
        } else {
            &[]
        })
        .iter()
        .chain(CURRENT);
        for (point_index, point) in points.enumerate() {
            let path = root.0.join(format!("{index}-{point_index}"));
            seed(&path, mode);
            assert_success(
                worker(&path, "selection-error")
                    .env("ULMDB_FAIL_AT", point)
                    .output()
                    .unwrap(),
            );
            let selected = matches!(
                *point,
                "pointer.CURRENT.after_rename"
                    | "pointer.CURRENT.before_directory_sync"
                    | "pointer.CURRENT.after_directory_sync"
            );
            assert_selected(
                &path,
                if selected { 2 } else { 1 },
                if selected { b"new" } else { b"old" },
            );
        }
    }
}

#[test]
fn process_death_after_successful_prepare_leaves_old_head_and_reclaimable_orphans() {
    let root = Directory::new();
    for (index, mode) in MODES.into_iter().enumerate() {
        let path = root.0.join(index.to_string());
        seed(&path, mode);
        let current = fs::read(path.join("CURRENT")).unwrap();
        let output = worker(&path, "die-prepared").output().unwrap();
        assert_eq!(output.status.code(), Some(86));
        assert_eq!(fs::read(path.join("CURRENT")).unwrap(), current);
        let env = Environment::open(&path).unwrap();
        assert_eq!(env.begin_read().sequence(), 1);
        let reclaimed = env.reclaim().unwrap();
        assert!(
            reclaimed.removed_segments + reclaimed.removed_descriptors + reclaimed.removed_cohorts
                > 0
        );
        let mut retry = env.begin_write().unwrap();
        retry.put(b"kv", b"key", b"retry").unwrap();
        assert_eq!(retry.commit().unwrap(), 2);
    }
}
