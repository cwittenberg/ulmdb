use std::{
    fs,
    ops::ControlFlow,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use ulmdb::{
    DatabaseKind, Environment, Error, PublicationMode, ReadTxn,
    backup::{self, BackupControl, BackupLimits, BackupStopReason, CancellationToken},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-controlled-export-{}",
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

fn source(mode: PublicationMode) -> (Directory, Arc<Environment>) {
    let dir = Directory::new();
    let env = Arc::new(Environment::create_with_publication(dir.0.join("source"), mode).unwrap());
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    write.put(b"kv", b"version", b"old").unwrap();
    write.put(b"kv", b"large", &vec![0x5a; 2 << 20]).unwrap();
    write.commit().unwrap();
    (dir, env)
}

fn largest_asset(target: &Path) -> u64 {
    ["segments", "cohorts", "commits"]
        .into_iter()
        .flat_map(|name| fs::read_dir(target.join(name)).into_iter().flatten())
        .flatten()
        .filter_map(|entry| entry.metadata().ok())
        .map(|metadata| metadata.len())
        .max()
        .unwrap_or(0)
}

#[test]
fn controlled_exports_preserve_the_exact_old_snapshot_while_source_writers_run() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        for independent in [false, true] {
            let (dir, env) = source(mode);
            let old = if independent {
                ReadTxn::open_coordinated_snapshot(dir.0.join("source")).unwrap()
            } else {
                env.begin_read()
            };
            let expected_sequence = old.sequence();
            let target = dir.0.join("export");
            let did_write = Arc::new(AtomicBool::new(false));
            let callback_env = env.clone();
            let callback_write = did_write.clone();
            let callback_target = target.clone();
            let control = BackupControl::default().with_progress_callback(Arc::new(move |_| {
                if callback_target.exists() && !callback_write.swap(true, Ordering::AcqRel) {
                    let mut writer = callback_env.begin_write().unwrap();
                    writer.put(b"kv", b"version", b"new").unwrap();
                    writer.commit().unwrap();
                }
                ControlFlow::Continue(())
            }));
            let summary = if independent {
                backup::export_independent_snapshot_with_control(
                    dir.0.join("source"),
                    &old,
                    &target,
                    &control,
                )
            } else {
                backup::export_snapshot_with_control(&env, &old, &target, &control)
            }
            .unwrap();
            assert!(did_write.load(Ordering::Acquire));
            assert_eq!(summary.sequence, expected_sequence);
            assert_eq!(
                env.begin_read().get(b"kv", b"version").unwrap(),
                Some(b"new".as_slice())
            );
            assert_eq!(old.get(b"kv", b"version").unwrap(), Some(b"old".as_slice()));
            let copy = Environment::open(&target).unwrap();
            assert_eq!(
                copy.begin_read().get(b"kv", b"version").unwrap(),
                Some(b"old".as_slice())
            );
            assert_eq!(
                copy.begin_read()
                    .get(b"kv", b"large")
                    .unwrap()
                    .unwrap()
                    .len(),
                2 << 20
            );
            assert_eq!(backup::verify(&copy).unwrap(), summary);
            assert!(control.progress().bytes >= 2 << 20);
        }
    }
}

#[test]
fn cancellation_inside_large_file_copy_never_selects_partial_output() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let (dir, env) = source(mode);
        let target = dir.0.join("cancelled");
        let copied = Arc::new(AtomicBool::new(false));
        let callback_copied = copied.clone();
        let callback_target = target.clone();
        let control = BackupControl::default().with_progress_callback(Arc::new(move |_| {
            if largest_asset(&callback_target) >= 65_536 {
                callback_copied.store(true, Ordering::Release);
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }));
        assert!(matches!(
            backup::export_with_control(&env, &target, &control),
            Err(ref error) if error.is_cancelled()
        ));
        assert!(
            copied.load(Ordering::Acquire),
            "cancellation must occur after a bounded copy chunk"
        );
        assert!(!target.join("CURRENT").exists());
        assert!(Environment::open(&target).is_err());
        assert_eq!(
            env.begin_read().get(b"kv", b"version").unwrap(),
            Some(b"old".as_slice())
        );
        assert_eq!(control.stop_reason(), Some(BackupStopReason::Cancelled));
        let resumed_target = dir.0.join("fresh-export");
        backup::export_with_control(&env, &resumed_target, &BackupControl::default()).unwrap();
        assert!(Environment::open(resumed_target).is_ok());
    }
}

#[test]
fn total_work_limits_and_deadlines_stop_before_creating_a_destination() {
    let (dir, env) = source(PublicationMode::Indexed);
    for (name, limits, expected) in [
        (
            "units",
            BackupLimits {
                max_units: 0,
                ..Default::default()
            },
            BackupStopReason::UnitLimit,
        ),
        (
            "bytes",
            BackupLimits {
                max_bytes: 1,
                ..Default::default()
            },
            BackupStopReason::ByteLimit,
        ),
        (
            "deadline",
            BackupLimits {
                deadline: Some(Instant::now()),
                ..Default::default()
            },
            BackupStopReason::Deadline,
        ),
    ] {
        let target = dir.0.join(name);
        let control = BackupControl::new(limits);
        assert!(matches!(
            backup::export_with_control(&env, &target, &control),
            Err(ref error) if error.is_work_limit_exceeded()
        ));
        assert_eq!(control.stop_reason(), Some(expected));
        assert!(!target.exists());
        assert!(control.progress().units <= limits.max_units);
        assert!(control.progress().bytes <= limits.max_bytes);
    }
}

#[test]
fn existing_targets_and_foreign_snapshots_are_never_modified() {
    let (dir, env) = source(PublicationMode::Indexed);
    let target = dir.0.join("existing");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep"), b"unrelated output").unwrap();
    let token = CancellationToken::new();
    token.cancel();
    let cancelled = BackupControl::default().with_cancel_token(token);
    assert!(matches!(
        backup::export_with_control(&env, &target, &cancelled),
        Err(ref error) if error.is_cancelled()
    ));
    assert!(backup::export_with_control(&env, &target, &BackupControl::default()).is_err());
    assert_eq!(fs::read(target.join("keep")).unwrap(), b"unrelated output");
    assert_eq!(fs::read_dir(&target).unwrap().count(), 1);
    let (_foreign_directory, foreign) = source(PublicationMode::Indexed);
    let rejected = dir.0.join("foreign");
    assert!(
        backup::export_snapshot_with_control(
            &env,
            &foreign.begin_read(),
            &rejected,
            &BackupControl::default()
        )
        .is_err()
    );
    assert!(!rejected.exists());
}
