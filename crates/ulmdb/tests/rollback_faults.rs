#![cfg(feature = "fault-injection")]
//! Real subprocess IO faults/process exits. This is not a power-loss claim.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use ulmdb::{DatabaseKind, Environment, Error, PublicationMode, RollbackError};
#[path = "support/crash.rs"]
mod crash_support;
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
const LEGACY: &[&str] = &[
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
const AFTER: &[&str] = &[
    "rollback.after_publish",
    "rollback.before_activate",
    "rollback.after_activate",
];
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let root = std::env::var_os("ULMDB_ROLLBACK_EVIDENCE")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = root.join(format!(
            "ulmdb-rollback-fault-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        if std::env::var_os("ULMDB_ROLLBACK_EVIDENCE").is_none() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
fn seed(path: &Path, mode: PublicationMode) {
    let env = Environment::create_with_publication(path, mode).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"old").unwrap();
    assert_eq!(w.commit().unwrap(), 1);
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"key", b"new").unwrap();
    assert_eq!(w.commit().unwrap(), 2);
}
fn worker(path: &Path, role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "rollback_process_worker", "--nocapture"])
        .env("ULMDB_ROLLBACK_DATABASE", path)
        .env("ULMDB_ROLLBACK_ROLE", role)
        .env_remove("ULMDB_FAIL_AT")
        .env_remove("ULMDB_CRASH_AT")
        .env_remove("ULMDB_TRACE_FILE");
    if std::env::var_os("ULMDB_ROLLBACK_EVIDENCE").is_some() {
        command.env("ULMDB_TRACE_FILE", path.with_extension("trace"));
    }
    command
}
fn success(output: std::process::Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn inspect(path: &Path, allowed: &[u64]) {
    let env = Environment::open(path).unwrap();
    let read = env.begin_read();
    assert!(
        allowed.contains(&read.sequence()),
        "sequence={}",
        read.sequence()
    );
    assert_eq!(
        read.get(b"kv", b"key").unwrap(),
        Some(if read.sequence() == 1 {
            b"old".as_slice()
        } else {
            b"new".as_slice()
        })
    );
    ulmdb::backup::verify(&env).unwrap();
    if read.sequence() == 1 {
        assert_eq!(ulmdb_storage::read_current(path).unwrap().previous, 0);
        assert!(ulmdb_storage::read_roots(path).unwrap().1.is_none());
        assert!(matches!(
            env.rollback_last_commit(1),
            Err(RollbackError::CannotRollback)
        ));
    }
    env.reclaim().unwrap();
    assert_eq!(
        read.get(b"kv", b"key").unwrap().unwrap(),
        if read.sequence() == 1 { b"old" } else { b"new" }
    );
}

#[test]
fn rollback_process_worker() {
    let Some(path) = std::env::var_os("ULMDB_ROLLBACK_DATABASE") else {
        return;
    };
    let env = crash_support::open_worker(&path);
    let role = std::env::var("ULMDB_ROLLBACK_ROLE").unwrap();
    let old = env.begin_read();
    let borrowed = old.get(b"kv", b"key").unwrap().unwrap();
    let result = env.rollback_last_commit(2);
    if role == "preparation-error" {
        assert!(
            matches!(result, Err(RollbackError::Engine(Error::Io(_)))),
            "{result:?}"
        );
        assert_eq!(env.begin_read().sequence(), 2);
        env.begin_write().unwrap().abort();
    } else if role == "selection-error" {
        assert!(
            matches!(result, Err(RollbackError::Engine(Error::OutcomeUnknown))),
            "{result:?}"
        );
        assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
    } else {
        assert_eq!(result.unwrap().sequence, 1);
        assert_eq!(env.begin_read().sequence(), 1);
    }
    assert_eq!(old.sequence(), 2);
    assert_eq!(borrowed, b"new");
}

#[test]
fn successful_rollback_trace_prepares_descriptor_and_selects_current_without_previous_rotation() {
    let root = Directory::new();
    for (index, mode) in MODES.into_iter().enumerate() {
        let path = root.0.join(index.to_string());
        seed(&path, mode);
        let trace = root.0.join(format!("trace-{index}"));
        success(
            worker(&path, "trace")
                .env("ULMDB_TRACE_FILE", &trace)
                .output()
                .unwrap(),
        );
        let expected: Vec<_> = (if mode == PublicationMode::Legacy {
            LEGACY
        } else {
            COHORT
        })
        .iter()
        .copied()
        .chain(["rollback.before_publish"])
        .chain(CURRENT.iter().copied())
        .chain(AFTER.iter().copied())
        .collect();
        assert_eq!(
            fs::read_to_string(trace)
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            expected
        );
        inspect(&path, &[1]);
    }
}

#[test]
fn each_prepublication_failure_keeps_exact_old_head_and_unpoisoned_writer() {
    let root = Directory::new();
    for (index, mode) in MODES.into_iter().enumerate() {
        for (cut, point) in (if mode == PublicationMode::Legacy {
            LEGACY
        } else {
            COHORT
        })
        .iter()
        .copied()
        .chain(["rollback.before_publish"])
        .enumerate()
        {
            let path = root.0.join(format!("{index}-{cut}"));
            seed(&path, mode);
            let current = fs::read(path.join("CURRENT")).unwrap();
            let previous = fs::read(path.join("PREVIOUS")).ok();
            success(
                worker(&path, "preparation-error")
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
            inspect(&path, &[2]);
        }
    }
}

#[test]
fn each_selection_and_activation_failure_is_uncertain_and_poisoned_with_complete_reopen() {
    let root = Directory::new();
    for (index, mode) in MODES.into_iter().enumerate() {
        for (cut, point) in CURRENT.iter().chain(AFTER).enumerate() {
            let path = root.0.join(format!("{index}-{cut}"));
            seed(&path, mode);
            success(
                worker(&path, "selection-error")
                    .env("ULMDB_FAIL_AT", point)
                    .output()
                    .unwrap(),
            );
            inspect(
                &path,
                if CURRENT[..5].contains(point) {
                    &[2]
                } else {
                    &[1]
                },
            );
        }
    }
}

#[test]
fn process_exit_at_every_preparation_selection_and_activation_boundary_reopens_complete_state() {
    let root = Directory::new();
    for (index, mode) in MODES.into_iter().enumerate() {
        for (cut, point) in (if mode == PublicationMode::Legacy {
            LEGACY
        } else {
            COHORT
        })
        .iter()
        .copied()
        .chain(["rollback.before_publish"])
        .chain(CURRENT.iter().copied())
        .chain(AFTER.iter().copied())
        .enumerate()
        {
            let path = root.0.join(format!("{index}-{cut}"));
            seed(&path, mode);
            let output = worker(&path, "exit")
                .env("ULMDB_CRASH_AT", point)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(86),
                "{mode:?} {point}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            // Real process exits after rename see the new complete root. This
            // does not model kernel/storage-cache loss on a power failure.
            inspect(&path, if cut < 12 { &[2] } else { &[1] });
        }
    }
}
