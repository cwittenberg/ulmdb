//! Explicit cooperative control for cold snapshot work. Ordinary database
//! reads do not consult a control. Legacy calls outside an explicit control
//! scope retain their established execution paths.
use crate::{Error, Result};
use std::{
    ops::ControlFlow,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::Instant,
};
use ulmdb_core::work::WorkObserver;

/// A cloneable cancellation request. Cancelling never modifies a database or
/// makes an incomplete export eligible for publication.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);
impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Total operation limits. Work units count native traversal/read/metadata
/// actions; bytes count requested logical reads, comparisons and copy/hash
/// chunks, including repeated visits. They are work bounds, not disk-usage
/// estimates. The deadline is cooperative, not a preemptive syscall timeout.
#[derive(Clone, Copy, Debug)]
pub struct BackupLimits {
    pub max_units: u64,
    pub max_bytes: u64,
    pub deadline: Option<Instant>,
}
impl BackupLimits {
    pub const UNLIMITED: Self = Self {
        max_units: u64::MAX,
        max_bytes: u64::MAX,
        deadline: None,
    };
}
impl Default for BackupLimits {
    fn default() -> Self {
        Self::UNLIMITED
    }
}

/// Cumulative reserved work. Concurrent users share the same monotonic
/// counters; a rejected reservation may conservatively consume units, but no
/// underlying action starts after its reservation was rejected.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BackupProgress {
    pub units: u64,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackupStopReason {
    Cancelled,
    UnitLimit,
    ByteLimit,
    Deadline,
}
impl BackupStopReason {
    fn code(self) -> u8 {
        match self {
            Self::Cancelled => 1,
            Self::UnitLimit => 2,
            Self::ByteLimit => 3,
            Self::Deadline => 4,
        }
    }
    fn decode(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Cancelled),
            2 => Some(Self::UnitLimit),
            3 => Some(Self::ByteLimit),
            4 => Some(Self::Deadline),
            _ => None,
        }
    }
    fn error(self) -> Error {
        if self == Self::Cancelled {
            Error::cancelled()
        } else {
            Error::work_limit_exceeded()
        }
    }
}

type ProgressCallback = Arc<dyn Fn(BackupProgress) -> ControlFlow<()> + Send + Sync>;
#[derive(Default)]
struct Counters {
    units: AtomicU64,
    bytes: AtomicU64,
    stop: AtomicU8,
}
struct State {
    limits: BackupLimits,
    token: CancellationToken,
    callback: Option<ProgressCallback>,
    counters: Arc<Counters>,
}
impl State {
    fn progress(&self) -> BackupProgress {
        BackupProgress {
            units: self.counters.units.load(Ordering::Acquire),
            bytes: self.counters.bytes.load(Ordering::Acquire),
        }
    }
    fn reason(&self) -> Option<BackupStopReason> {
        BackupStopReason::decode(self.counters.stop.load(Ordering::Acquire))
    }
    fn stop(&self, reason: BackupStopReason) -> Error {
        let _ = self.counters.stop.compare_exchange(
            0,
            reason.code(),
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.reason().unwrap_or(reason).error()
    }
    #[inline]
    fn poll(&self) -> Result<()> {
        if let Some(reason) = self.reason() {
            return Err(reason.error());
        }
        if self.token.is_cancelled() {
            return Err(self.stop(BackupStopReason::Cancelled));
        }
        if self
            .limits
            .deadline
            .is_some_and(|end| Instant::now() >= end)
        {
            return Err(self.stop(BackupStopReason::Deadline));
        }
        Ok(())
    }
    #[inline]
    fn reserve(
        &self,
        counter: &AtomicU64,
        amount: u64,
        limit: u64,
        reason: BackupStopReason,
    ) -> Result<()> {
        if amount == 0 {
            return Ok(());
        }
        let mut old = counter.load(Ordering::Acquire);
        loop {
            let next = old
                .checked_add(amount)
                .filter(|n| *n <= limit)
                .ok_or_else(|| self.stop(reason))?;
            match counter.compare_exchange_weak(old, next, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return Ok(()),
                Err(actual) => old = actual,
            }
        }
    }
}
impl WorkObserver for State {
    #[inline]
    fn checkpoint(&self, units: u64, bytes: u64) -> Result<()> {
        self.poll()?;
        self.reserve(
            &self.counters.units,
            units,
            self.limits.max_units,
            BackupStopReason::UnitLimit,
        )?;
        self.reserve(
            &self.counters.bytes,
            bytes,
            self.limits.max_bytes,
            BackupStopReason::ByteLimit,
        )?;
        if let Some(callback) = &self.callback
            && !ulmdb_core::work::in_critical()
        {
            let progress = self.progress();
            // User callbacks may perform independent work. Do not accidentally
            // inherit this backup's observer in that work or recursively poll
            // the observer while a callback is executing.
            if ulmdb_core::work::without_observer(|| callback(progress)).is_break() {
                self.token.cancel();
                return Err(self.stop(BackupStopReason::Cancelled));
            }
            // A callback may request cancellation and still return Continue.
            return self.poll();
        }
        Ok(())
    }
}

/// Owned, cloneable operation control. Clones share quotas and stop state.
/// Configure token/callback builders before starting an operation. Callbacks
/// run between native work items and may request cancellation. Caller callbacks
/// and one kernel call cannot themselves be preempted by the engine.
#[derive(Clone)]
pub struct BackupControl {
    state: Arc<State>,
    observer_override: Option<Arc<dyn WorkObserver>>,
}
impl Default for BackupControl {
    fn default() -> Self {
        Self::new(BackupLimits::default())
    }
}
impl BackupControl {
    pub fn new(limits: BackupLimits) -> Self {
        Self {
            state: Arc::new(State {
                limits,
                token: CancellationToken::new(),
                callback: None,
                counters: Arc::new(Counters::default()),
            }),
            observer_override: None,
        }
    }
    pub fn with_cancel_token(mut self, token: CancellationToken) -> Self {
        self.state = Arc::new(State {
            limits: self.state.limits,
            token,
            callback: self.state.callback.clone(),
            counters: self.state.counters.clone(),
        });
        self
    }
    pub fn with_progress_callback(mut self, callback: ProgressCallback) -> Self {
        self.state = Arc::new(State {
            limits: self.state.limits,
            token: self.state.token.clone(),
            callback: Some(callback),
            counters: self.state.counters.clone(),
        });
        self
    }
    pub fn cancellation_token(&self) -> CancellationToken {
        self.state.token.clone()
    }
    pub fn limits(&self) -> BackupLimits {
        self.state.limits
    }
    pub fn progress(&self) -> BackupProgress {
        self.state.progress()
    }
    pub fn stop_reason(&self) -> Option<BackupStopReason> {
        self.state.reason()
    }
    /// Reserve before doing work; zero/zero is a cancellation/deadline poll.
    #[inline]
    pub fn checkpoint(&self, units: u64, bytes: u64) -> Result<()> {
        match &self.observer_override {
            Some(observer) => observer.checkpoint(units, bytes),
            None => self.state.checkpoint(units, bytes),
        }
    }
    pub(crate) fn observer(&self) -> Arc<dyn WorkObserver> {
        self.observer_override
            .clone()
            .unwrap_or_else(|| self.state.clone())
    }
    pub(crate) fn with_observer_override(mut self, observer: Arc<dyn WorkObserver>) -> Self {
        self.observer_override = Some(observer);
        self
    }
    /// Observe cold loaders and bounded file hashing in this thread for the
    /// duration of the closure. Nested scopes and unwinding restore the prior
    /// observer. Logical graph traversal also uses explicit checked adapters.
    /// Custom native admission work must run inside this scope so namespace
    /// gates suppress callbacks and reject work slices instead of pausing.
    pub fn scope<T>(&self, body: impl FnOnce() -> Result<T>) -> Result<T> {
        self.checkpoint(0, 0)?;
        // Do not perform a generic exit poll: export has an explicit terminal
        // publication boundary after which a late cancellation cannot truthfully
        // turn an already-published result into a cancelled operation.
        ulmdb_core::work::with_observer(self.observer(), body)
    }
}

impl WorkObserver for BackupControl {
    #[inline]
    fn checkpoint(&self, units: u64, bytes: u64) -> Result<()> {
        BackupControl::checkpoint(self, units, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quotas_reserve_before_work_and_all_clones_share_the_stop() {
        let control = BackupControl::new(BackupLimits {
            max_units: 3,
            max_bytes: 5,
            deadline: None,
        });
        let other = control.clone();
        control.checkpoint(2, 3).unwrap();
        assert!(matches!(
            other.checkpoint(1, 3),
            Err(ref error) if error.is_work_limit_exceeded()
        ));
        assert_eq!(control.stop_reason(), Some(BackupStopReason::ByteLimit));
        assert!(control.progress().units <= 3 && control.progress().bytes <= 5);
        assert!(matches!(
            control.checkpoint(0, 0),
            Err(ref error) if error.is_work_limit_exceeded()
        ));
    }
    #[test]
    fn expired_deadline_and_pre_cancelled_token_stop_before_work() {
        let expired = BackupControl::new(BackupLimits {
            deadline: Some(Instant::now()),
            ..Default::default()
        });
        assert!(matches!(
            expired.checkpoint(1, 1),
            Err(ref error) if error.is_work_limit_exceeded()
        ));
        assert_eq!(expired.stop_reason(), Some(BackupStopReason::Deadline));
        assert_eq!(expired.progress(), BackupProgress::default());
        let token = CancellationToken::new();
        let control = BackupControl::default().with_cancel_token(token.clone());
        std::thread::spawn(move || token.cancel()).join().unwrap();
        assert!(matches!(control.checkpoint(1, 1), Err(ref error) if error.is_cancelled()));
        assert_eq!(control.stop_reason(), Some(BackupStopReason::Cancelled));
        assert_eq!(control.progress(), BackupProgress::default());
    }
    #[test]
    fn callback_work_is_outside_the_native_observer_and_scopes_restore_after_error() {
        let calls = Arc::new(AtomicU64::new(0));
        let observed = calls.clone();
        let control = BackupControl::default().with_progress_callback(Arc::new(move |_| {
            observed.fetch_add(1, Ordering::Relaxed);
            ulmdb_core::work::checkpoint(10_000, 10_000).unwrap();
            ControlFlow::Continue(())
        }));
        let result: Result<()> = control.scope(|| {
            ulmdb_core::work::checkpoint(3, 7)?;
            Err(Error::InvalidInput("test stop"))
        });
        assert!(result.is_err());
        assert_eq!(control.progress(), BackupProgress { units: 3, bytes: 7 });
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        assert!(ulmdb_core::work::current_observer().is_none());
    }
    #[test]
    fn critical_capture_work_is_charged_without_invoking_user_callbacks() {
        let calls = Arc::new(AtomicU64::new(0));
        let seen = calls.clone();
        let control = BackupControl::default().with_progress_callback(Arc::new(move |_| {
            seen.fetch_add(1, Ordering::Relaxed);
            ControlFlow::Continue(())
        }));
        control
            .scope(|| {
                assert_eq!(calls.load(Ordering::Acquire), 1);
                ulmdb_core::work::critical(|| control.checkpoint(2, 16))?;
                assert_eq!(calls.load(Ordering::Acquire), 1);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            control.progress(),
            BackupProgress {
                units: 2,
                bytes: 16
            }
        );
        assert_eq!(calls.load(Ordering::Acquire), 1);
        control.checkpoint(0, 0).unwrap();
        assert_eq!(calls.load(Ordering::Acquire), 2);
    }
}
