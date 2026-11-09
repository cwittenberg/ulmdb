use std::{sync::mpsc, thread};
use ulmdb::{
    Error,
    backup::{BackupControl, BackupJob, BackupProgress, JobStep, WorkBudget},
};

#[test]
fn helper_thread_cannot_spend_worker_permits_or_prematurely_pause_the_owner() {
    let control = BackupControl::default();
    let mut job = BackupJob::spawn(control.clone(), |operation| {
        let helper_control = operation.clone();
        let rejected = thread::spawn(move || {
            matches!(helper_control.checkpoint(1, 1), Err(Error::InvalidInput(_)))
        })
        .join()
        .unwrap();
        assert!(rejected);
        operation.checkpoint(1, 1)?;
        Ok(17)
    })
    .unwrap();
    assert!(matches!(
        job.advance(WorkBudget {
            max_units: 1,
            max_bytes: 1
        })
        .unwrap(),
        JobStep::Complete(17)
    ));
    assert_eq!(control.progress(), BackupProgress { units: 1, bytes: 1 });
}

#[test]
fn exposed_operation_control_is_affine_while_paused_and_shared_after_completion() {
    let control = BackupControl::default();
    let (sender, receiver) = mpsc::sync_channel(1);
    let mut job = BackupJob::spawn(control.clone(), move |operation| {
        sender.send(operation.clone()).unwrap();
        operation.checkpoint(1, 1)?;
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        job.advance(WorkBudget::default()).unwrap(),
        JobStep::Pending {
            required: WorkBudget {
                max_units: 1,
                max_bytes: 1
            },
            ..
        }
    ));
    let operation = receiver.recv().unwrap();
    assert!(matches!(
        operation.checkpoint(1, 1),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(control.progress(), BackupProgress::default());
    // The public base handle still supports independent cancellation/progress
    // work; it does not carry or consume the worker's private slice permits.
    job.control().checkpoint(0, 0).unwrap();
    assert!(matches!(
        job.advance(WorkBudget {
            max_units: 1,
            max_bytes: 1
        })
        .unwrap(),
        JobStep::Complete(())
    ));
    operation.checkpoint(1, 1).unwrap();
    assert_eq!(control.progress(), BackupProgress { units: 2, bytes: 2 });
    drop(job);
    assert!(!control.cancellation_token().is_cancelled());
}
