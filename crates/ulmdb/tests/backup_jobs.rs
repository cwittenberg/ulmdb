use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use ulmdb::{
    DatabaseKind, Environment, Error, PublicationMode,
    backup::{self, BackupControl, BackupJob, BackupLimits, BackupStopReason, JobStep, WorkBudget},
};

#[test]
fn slices_preserve_the_running_stack_without_replaying_work_or_callbacks() {
    let visits = Arc::new(AtomicU64::new(0));
    let worker_visits = visits.clone();
    let control = BackupControl::default();
    let mut job = BackupJob::spawn(control.clone(), move |control| {
        let mut sum = 0;
        for number in 1..=10 {
            control.checkpoint(1, 4)?;
            worker_visits.fetch_add(1, Ordering::Relaxed);
            sum += number;
        }
        Ok(sum)
    })
    .unwrap();
    assert_eq!(visits.load(Ordering::Relaxed), 0);
    assert!(
        matches!(job.advance(WorkBudget { max_units: 3, max_bytes: 12 }).unwrap(), JobStep::Pending { progress, .. } if progress.units == 3 && progress.bytes == 12)
    );
    assert_eq!(visits.load(Ordering::Relaxed), 3);
    assert!(
        matches!(job.advance(WorkBudget { max_units: 2, max_bytes: 8 }).unwrap(), JobStep::Pending { progress, .. } if progress.units == 5 && progress.bytes == 20)
    );
    assert_eq!(visits.load(Ordering::Relaxed), 5);
    assert!(matches!(
        job.advance(WorkBudget::UNLIMITED).unwrap(),
        JobStep::Complete(55)
    ));
    assert_eq!(visits.load(Ordering::Relaxed), 10);
    drop(job);
    assert!(
        !control.cancellation_token().is_cancelled(),
        "dropping a completed job cancelled its shared control"
    );
}

#[test]
fn insufficient_slice_reports_the_next_request_and_total_limits_stop_before_work() {
    let visits = Arc::new(AtomicU64::new(0));
    let worker_visits = visits.clone();
    let mut job = BackupJob::spawn(BackupControl::default(), move |control| {
        control.checkpoint(1, 4)?;
        worker_visits.fetch_add(1, Ordering::Relaxed);
        Ok(())
    })
    .unwrap();
    assert!(
        matches!(job.advance(WorkBudget { max_units: 1, max_bytes: 3 }).unwrap(), JobStep::Pending { progress, required } if progress.units == 0 && progress.bytes == 0 && required == WorkBudget { max_units: 1, max_bytes: 4 })
    );
    assert_eq!(visits.load(Ordering::Relaxed), 0);
    assert!(matches!(
        job.advance(WorkBudget {
            max_units: 1,
            max_bytes: 4
        })
        .unwrap(),
        JobStep::Complete(())
    ));
    assert_eq!(visits.load(Ordering::Relaxed), 1);

    let control = BackupControl::new(BackupLimits {
        max_units: 3,
        ..BackupLimits::UNLIMITED
    });
    let counter = Arc::new(AtomicU64::new(0));
    let worker_counter = counter.clone();
    let mut job = BackupJob::spawn(control.clone(), move |control| {
        for _ in 0..10 {
            control.checkpoint(1, 1)?;
            worker_counter.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        job.advance(WorkBudget::UNLIMITED),
        Err(ref error) if error.is_work_limit_exceeded()
    ));
    assert_eq!(counter.load(Ordering::Relaxed), 3);
    assert_eq!(control.stop_reason(), Some(BackupStopReason::UnitLimit));
}

#[test]
fn token_cancellation_and_drop_release_a_paused_worker() {
    let control = BackupControl::default();
    let token = control.cancellation_token();
    let mut job = BackupJob::spawn(control, |control| {
        control.checkpoint(1, 1)?;
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        job.advance(WorkBudget::default()).unwrap(),
        JobStep::Pending { .. }
    ));
    token.cancel();
    assert!(matches!(
        job.advance(WorkBudget::UNLIMITED),
        Err(ref error) if error.is_cancelled()
    ));

    let done = Arc::new(AtomicU64::new(0));
    let worker_done = done.clone();
    let mut job = BackupJob::spawn(BackupControl::default(), move |control| {
        struct Guard(Arc<AtomicU64>);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let _guard = Guard(worker_done);
        control.checkpoint(1, 1)?;
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        job.advance(WorkBudget::default()).unwrap(),
        JobStep::Pending { .. }
    ));
    drop(job);
    assert_eq!(done.load(Ordering::Relaxed), 1);
}

#[test]
fn abandoned_result_can_release_resources_through_its_control_without_locking_the_gate() {
    struct ResultOwner(BackupControl, Arc<AtomicU64>);
    impl Drop for ResultOwner {
        fn drop(&mut self) {
            let _ = self.0.checkpoint(0, 0);
            self.1.fetch_add(1, Ordering::Relaxed);
        }
    }
    let released = Arc::new(AtomicU64::new(0));
    let worker_released = released.clone();
    let mut job = BackupJob::spawn(BackupControl::default(), move |control| {
        // Simulate a caller-owned result constructed while unwinding a stop.
        let _ = control.checkpoint(1, 1);
        Ok(ResultOwner(control, worker_released))
    })
    .unwrap();
    assert!(matches!(
        job.advance(WorkBudget::default()).unwrap(),
        JobStep::Pending { .. }
    ));
    let (done, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(job);
        let _ = done.send(());
    });
    received
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("result destructor deadlocked its completed job");
    assert_eq!(released.load(Ordering::Relaxed), 1);
}

struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-backup-job-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn paused_native_verification_keeps_old_snapshot_without_freezing_new_writers() {
    paused_native_verification_case(false);
    paused_native_verification_case(true);
}
fn paused_native_verification_case(independent: bool) {
    let directory = Directory::new();
    let environment = Arc::new(
        Environment::create_with_publication(&directory.0, PublicationMode::Indexed).unwrap(),
    );
    let mut write = environment.begin_write().unwrap();
    write.create_database(b"data", DatabaseKind::Kv).unwrap();
    write
        .load_sorted_kv(
            b"data",
            (0..2_000u32).map(|n| Ok((n.to_be_bytes(), n.to_le_bytes()))),
        )
        .unwrap();
    let sequence = write.commit().unwrap();
    let read = if independent {
        ulmdb::ReadTxn::open_coordinated_snapshot(&directory.0).unwrap()
    } else {
        environment.begin_read()
    };
    let control = BackupControl::default();
    let mut job = if independent {
        backup::verify_independent_snapshot_job(&directory.0, read, control).unwrap()
    } else {
        backup::verify_snapshot_job(environment.clone(), read, control).unwrap()
    };
    assert!(matches!(
        job.advance(WorkBudget {
            max_units: 1,
            max_bytes: 256 * 1024
        })
        .unwrap(),
        JobStep::Pending { .. }
    ));
    let mut write = environment.begin_write().unwrap();
    write.put(b"data", &7u32.to_be_bytes(), b"new").unwrap();
    assert_eq!(write.commit().unwrap(), sequence + 1);
    environment.reclaim().unwrap();
    let mut budget = WorkBudget {
        max_units: 97,
        max_bytes: 256 * 1024,
    };
    let mut previous = job.control().progress().units;
    loop {
        match job.advance(budget).unwrap() {
            JobStep::Pending { progress, required } => {
                assert!(progress.units - previous <= budget.max_units);
                previous = progress.units;
                budget.max_units = budget.max_units.max(required.max_units);
                budget.max_bytes = budget.max_bytes.max(required.max_bytes);
            }
            JobStep::Complete(summary) => {
                assert_eq!(summary.sequence, sequence);
                break;
            }
        }
    }
}
