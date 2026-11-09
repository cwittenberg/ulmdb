//! Stack-preserving cooperative jobs. A paused worker performs no native work.
use super::{BackupControl, BackupProgress, CancellationToken, Summary};
use crate::{Environment, Error, ReadTxn, Result};
use std::path::Path;
use std::{
    cell::Cell,
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use ulmdb_core::work::WorkObserver;

/// Work allowed by one `advance`, independently of total `BackupLimits`.
/// An indivisible mapped read can require a larger byte grant; Pending reports
/// that exact request without starting it or replaying earlier validation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkBudget {
    pub max_units: u64,
    pub max_bytes: u64,
}
impl WorkBudget {
    pub const UNLIMITED: Self = Self {
        max_units: u64::MAX,
        max_bytes: u64::MAX,
    };
}

// A grant belongs exclusively to the one worker executing this job's stack.
// Keeping its remaining slice here avoids a mutex on every checked tree read.
// No references or unchecked pointers are stored in TLS, and no grant is also
// available in the shared state after it has been transferred to the worker.
#[derive(Clone, Copy)]
struct LocalSlice {
    job: u64,
    remaining: WorkBudget,
    granted: bool,
}
thread_local! {
    static WORKER_SLICE: Cell<Option<LocalSlice>> = const { Cell::new(None) };
}
struct WorkerScope {
    previous: Option<LocalSlice>,
    _same_thread: PhantomData<Rc<()>>,
}
impl WorkerScope {
    fn enter(job: u64) -> Self {
        Self {
            previous: WORKER_SLICE.with(|slot| {
                slot.replace(Some(LocalSlice {
                    job,
                    remaining: WorkBudget::default(),
                    granted: false,
                }))
            }),
            _same_thread: PhantomData,
        }
    }
}
impl Drop for WorkerScope {
    fn drop(&mut self) {
        WORKER_SLICE.with(|slot| slot.set(self.previous));
    }
}
enum SliceCheck {
    Allowed,
    Exhausted,
    OtherThread,
}
fn take_local(job: u64, units: u64, bytes: u64) -> SliceCheck {
    WORKER_SLICE.with(|slot| {
        let Some(mut local) = slot.get().filter(|local| local.job == job) else {
            return SliceCheck::OtherThread;
        };
        if local.granted && units <= local.remaining.max_units && bytes <= local.remaining.max_bytes
        {
            local.remaining.max_units -= units;
            local.remaining.max_bytes -= bytes;
            slot.set(Some(local));
            SliceCheck::Allowed
        } else {
            SliceCheck::Exhausted
        }
    })
}
fn grant_local(job: u64, remaining: WorkBudget) {
    WORKER_SLICE.with(|slot| {
        if slot.get().is_some_and(|local| local.job == job) {
            slot.set(Some(LocalSlice {
                job,
                remaining,
                granted: true,
            }));
        }
    });
}
fn clear_local(job: u64) {
    WORKER_SLICE.with(|slot| {
        if let Some(mut local) = slot.get().filter(|local| local.job == job) {
            local.granted = false;
            local.remaining = WorkBudget::default();
            slot.set(Some(local));
        }
    });
}

#[derive(Debug)]
pub enum JobStep<T> {
    Pending {
        progress: BackupProgress,
        required: WorkBudget,
    },
    Complete(T),
}

struct State<T> {
    remaining: WorkBudget,
    required: WorkBudget,
    active: bool,
    cancelled: bool,
    finished: bool,
    result: Option<Result<T>>,
}
struct Gate<T> {
    job: u64,
    creator: u32,
    base: BackupControl,
    cancellation: CancellationToken,
    deadline: Option<Instant>,
    state: Mutex<State<T>>,
    changed: Condvar,
}
impl<T> Gate<T> {
    fn stopped(&self, state: &State<T>) -> bool {
        state.cancelled
            || self.cancellation.is_cancelled()
            || self.deadline.is_some_and(|end| Instant::now() >= end)
    }
    fn stop_error(&self) -> Error {
        match self.base.checkpoint(0, 0) {
            Err(error) => error,
            Ok(()) => Error::cancelled(),
        }
    }
    fn finish(&self, result: Result<T>) {
        clear_local(self.job);
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.active = false;
        state.finished = true;
        state.result = Some(result);
        self.changed.notify_all();
    }
    fn cancel(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.finished {
            return;
        }
        self.cancellation.cancel();
        state.cancelled = true;
        self.changed.notify_all();
    }
}
impl<T: Send> WorkObserver for Gate<T> {
    fn checkpoint(&self, units: u64, bytes: u64) -> Result<()> {
        let owner = match take_local(self.job, units, bytes) {
            SliceCheck::Allowed => return self.base.checkpoint(units, bytes),
            SliceCheck::Exhausted => true,
            SliceCheck::OtherThread => false,
        };
        // Normal worker checkpoints never enter this slow path while their
        // exact grant suffices. Foreign callers must be rejected before any
        // inherited mutex can be touched after fork.
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("backup job belongs to another process"));
        }
        clear_local(self.job);
        let mut state = self.state.lock().map_err(|_| Error::OutcomeUnknown)?;
        loop {
            if state.finished {
                drop(state);
                return self.base.checkpoint(units, bytes);
            }
            if !owner {
                // A helper thread cannot consume the owner's cached permits or
                // report Pending while the owner is still running on a grant.
                // The job's exposed base control remains freely shareable.
                return Err(Error::InvalidInput(
                    "active backup job control belongs to its worker",
                ));
            }
            if self.stopped(&state) {
                drop(state);
                return Err(self.stop_error());
            }
            if state.active {
                let grant = std::mem::take(&mut state.remaining);
                grant_local(self.job, grant);
                if matches!(take_local(self.job, units, bytes), SliceCheck::Allowed) {
                    drop(state);
                    // Total caps, token and deadline remain checked before
                    // every item, including the mutex-free worker fast path.
                    return self.base.checkpoint(units, bytes);
                }
                clear_local(self.job);
            }
            if ulmdb_core::work::in_critical() {
                // Parking here would return control while retaining Capture or
                // Registry. Abort and release the bootstrap instead. Callers
                // can allow a larger initial slice; total limits still apply.
                return Err(Error::work_limit_exceeded());
            }
            state.required = WorkBudget {
                max_units: units,
                max_bytes: bytes,
            };
            state.active = false;
            self.changed.notify_all();
            // A token can be cancelled independently of this job. Periodic
            // blocking waits observe that flag/deadline without spinning or
            // retaining a native Capture/Registry gate across slices.
            let wait = self
                .changed
                .wait_timeout(state, Duration::from_millis(20))
                .map_err(|_| Error::OutcomeUnknown)?;
            state = wait.0;
        }
    }
}

/// Owned background operation with real pause/resume, not restart-on-budget.
/// The closure must perform native work through the supplied control. Engine
/// and IPDon job factories do this throughout opening, verification and export.
/// Each job owns one worker thread. Caller callbacks, allocation, a kernel call
/// and the documented final publication boundary are not preemptible mid-call.
/// Cold initial descriptor/pin bootstrap cannot pause while holding source
/// gates: an insufficient initial slice returns a work-limit error instead.
/// Already-admitted snapshot factories do not repeat that cold bootstrap.
/// The operation control passed to the closure is worker-affine while running;
/// using it from a helper thread returns `InvalidInput`. Use `control()` for a
/// shared total-budget/cancellation handle independent of the worker's slices.
pub struct BackupJob<T: Send + 'static> {
    creator: u32,
    gate: Arc<Gate<T>>,
    worker: Option<JoinHandle<()>>,
    consumed: bool,
}
impl<T: Send + 'static> BackupJob<T> {
    pub fn spawn(
        control: BackupControl,
        work: impl FnOnce(BackupControl) -> Result<T> + Send + 'static,
    ) -> Result<Self> {
        static NEXT_JOB: AtomicU64 = AtomicU64::new(1);
        let job = NEXT_JOB
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| Error::LimitExceeded)?;
        let creator = std::process::id();
        let gate = Arc::new(Gate {
            job,
            creator,
            base: control.clone(),
            cancellation: control.cancellation_token(),
            deadline: control.limits().deadline,
            state: Mutex::new(State {
                remaining: WorkBudget::default(),
                required: WorkBudget::default(),
                active: false,
                cancelled: false,
                finished: false,
                result: None,
            }),
            changed: Condvar::new(),
        });
        let operation = control.with_observer_override(gate.clone());
        let worker_gate = gate.clone();
        let worker = thread::Builder::new()
            .name("ulmdb-backup".into())
            .spawn(move || {
                let _worker_scope = WorkerScope::enter(worker_gate.job);
                let result = catch_unwind(AssertUnwindSafe(|| {
                    operation.scope(|| work(operation.clone()))
                }))
                .unwrap_or(Err(Error::OutcomeUnknown));
                worker_gate.finish(result);
            })?;
        Ok(Self {
            creator,
            gate,
            worker: Some(worker),
            consumed: false,
        })
    }

    pub fn control(&self) -> &BackupControl {
        &self.gate.base
    }

    /// Grant one bounded slice and wait until the next checkpoint or completion.
    /// A larger `required` grant is needed to start an indivisible request.
    pub fn advance(&mut self, budget: WorkBudget) -> Result<JobStep<T>> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("backup job belongs to another process"));
        }
        if self.consumed {
            return Err(Error::InvalidInput("backup job already completed"));
        }
        let mut state = self.gate.state.lock().map_err(|_| Error::OutcomeUnknown)?;
        if !state.finished {
            state.remaining = budget;
            state.active = true;
            self.gate.changed.notify_all();
            while state.active && !state.finished {
                state = self
                    .gate
                    .changed
                    .wait(state)
                    .map_err(|_| Error::OutcomeUnknown)?;
            }
        }
        if state.finished {
            let result = state.result.take().ok_or(Error::OutcomeUnknown)?;
            drop(state);
            self.consumed = true;
            if let Some(worker) = self.worker.take() {
                worker.join().map_err(|_| Error::OutcomeUnknown)?;
            }
            result.map(JobStep::Complete)
        } else {
            Ok(JobStep::Pending {
                progress: self.gate.base.progress(),
                required: state.required,
            })
        }
    }

    /// Request cancellation. `advance`/`cancel_and_wait` acknowledges its result;
    /// publication may already have crossed its documented terminal boundary.
    pub fn cancel(&self) {
        if self.creator != std::process::id() {
            return;
        }
        self.gate.cancel();
    }

    pub fn cancel_and_wait(&mut self) -> Result<T> {
        self.cancel();
        match self.advance(WorkBudget::UNLIMITED)? {
            JobStep::Complete(value) => Ok(value),
            JobStep::Pending { .. } => Err(Error::OutcomeUnknown),
        }
    }
}
impl<T: Send + 'static> Drop for BackupJob<T> {
    fn drop(&mut self) {
        if self.creator != std::process::id() {
            // A fork child has no worker to join and must not lock inherited
            // thread state or destroy caller-owned results behind its mutex.
            std::mem::forget(self.gate.clone());
            std::mem::forget(self.worker.take());
            return;
        }
        self.gate.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        // Release any completed result, including a result that itself owns a
        // control. It must not retain a cycle through the job's observer gate.
        let result = {
            self.gate
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .result
                .take()
        };
        drop(result);
    }
}

/// Verify the exact existing read while other threads publish newer commits.
pub fn verify_snapshot_job(
    environment: Arc<Environment>,
    read: ReadTxn,
    control: BackupControl,
) -> Result<BackupJob<Summary>> {
    BackupJob::spawn(control, move |control| {
        super::verify_snapshot_with_control(&environment, &read, &control)
    })
}

pub fn export_snapshot_job(
    environment: Arc<Environment>,
    read: ReadTxn,
    target: impl AsRef<Path>,
    control: BackupControl,
) -> Result<BackupJob<Summary>> {
    let target = target.as_ref().to_path_buf();
    BackupJob::spawn(control, move |control| {
        super::export_snapshot_with_control(&environment, &read, &target, &control)
    })
}

pub fn verify_independent_snapshot_job(
    source: impl AsRef<Path>,
    read: ReadTxn,
    control: BackupControl,
) -> Result<BackupJob<Summary>> {
    let source = source.as_ref().to_path_buf();
    BackupJob::spawn(control, move |control| {
        super::verify_independent_snapshot_with_control(&source, &read, &control)
    })
}

pub fn export_independent_snapshot_job(
    source: impl AsRef<Path>,
    read: ReadTxn,
    target: impl AsRef<Path>,
    control: BackupControl,
) -> Result<BackupJob<Summary>> {
    let source = source.as_ref().to_path_buf();
    let target = target.as_ref().to_path_buf();
    BackupJob::spawn(control, move |control| {
        super::export_independent_snapshot_with_control(&source, &read, &target, &control)
    })
}
