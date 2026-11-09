use std::{fs, path::PathBuf, sync::Arc};
use ulmdb::{
    DatabaseKind, Environment, Error, PublicationMode, ReadTxn,
    backup::{BackupControl, BackupJob, JobStep, WorkBudget},
};
use ulmdb_storage::coordination::Coordination;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-cold-backup-control-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn source() -> (Directory, Arc<Environment>, [u8; 16], u64) {
    let directory = Directory::new();
    let environment = Arc::new(
        Environment::create_with_publication(&directory.0, PublicationMode::Indexed).unwrap(),
    );
    let mut writer = environment.begin_write().unwrap();
    writer.create_database(b"data", DatabaseKind::Kv).unwrap();
    writer.put(b"data", b"version", b"old").unwrap();
    writer.put(b"data", b"large", &vec![9; 2 << 20]).unwrap();
    let sequence = writer.commit().unwrap();
    let uuid = environment.begin_read().file_manifest().unwrap().uuid;
    (directory, environment, uuid, sequence)
}

#[test]
fn cold_read_job_releases_capture_after_pinning_before_pausing_mapping_admission() {
    let (directory, environment, uuid, sequence) = source();
    let source_path = directory.0.clone();
    let mut job = BackupJob::spawn(BackupControl::default(), move |control| {
        control.scope(|| ReadTxn::open_coordinated_snapshot(source_path))
    })
    .unwrap();
    assert!(matches!(
        job.advance(WorkBudget {
            max_units: 128,
            max_bytes: 256 << 10
        })
        .unwrap(),
        JobStep::Pending { .. }
    ));
    // A shared Capture held by the paused worker would make this exclusive
    // lease Busy. Its completed physical pin alone must retain the old files.
    let coordinator = Coordination::open(&directory.0, uuid, Default::default()).unwrap();
    drop(coordinator.try_maintenance().unwrap());
    let mut writer = environment.begin_write().unwrap();
    writer.put(b"data", b"version", b"new").unwrap();
    assert_eq!(writer.commit().unwrap(), sequence + 1);
    environment.reclaim().unwrap();
    let JobStep::Complete(old) = job.advance(WorkBudget::UNLIMITED).unwrap() else {
        panic!("unlimited grant did not finish cold admission");
    };
    assert_eq!(old.sequence(), sequence);
    assert_eq!(
        old.get(b"data", b"version").unwrap(),
        Some(b"old".as_slice())
    );
    assert_eq!(old.get(b"data", b"large").unwrap().unwrap().len(), 2 << 20);
    assert_eq!(
        environment.begin_read().get(b"data", b"version").unwrap(),
        Some(b"new".as_slice())
    );
}

#[test]
fn insufficient_atomic_bootstrap_grant_stops_instead_of_parking_with_capture() {
    let (directory, _environment, uuid, _) = source();
    let source_path = directory.0.clone();
    let mut job = BackupJob::spawn(BackupControl::default(), move |control| {
        control.scope(|| ReadTxn::open_coordinated_snapshot(source_path))
    })
    .unwrap();
    assert!(matches!(
        job.advance(WorkBudget::default()),
        Err(ref error) if error.is_work_limit_exceeded()
    ));
    let coordinator = Coordination::open(&directory.0, uuid, Default::default()).unwrap();
    drop(coordinator.try_maintenance().unwrap());
}

#[test]
fn cold_environment_job_can_cancel_after_pin_without_retaining_capture_or_writer_lock() {
    let (directory, environment, uuid, _) = source();
    drop(environment);
    let source_path = directory.0.clone();
    let mut job = BackupJob::spawn(BackupControl::default(), move |control| {
        control.scope(|| Environment::open(source_path))
    })
    .unwrap();
    assert!(matches!(
        job.advance(WorkBudget {
            max_units: 128,
            max_bytes: 256 << 10
        })
        .unwrap(),
        JobStep::Pending { .. }
    ));
    let coordinator = Coordination::open(&directory.0, uuid, Default::default()).unwrap();
    drop(coordinator.try_maintenance().unwrap());
    assert!(matches!(job.cancel_and_wait(), Err(ref error) if error.is_cancelled()));
    let reopened = Environment::open(&directory.0).unwrap();
    assert_eq!(
        reopened.begin_read().get(b"data", b"version").unwrap(),
        Some(b"old".as_slice())
    );
}
