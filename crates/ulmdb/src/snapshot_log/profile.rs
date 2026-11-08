//! Optional diagnostic phase clocks for the actual snapshot-log engine.
//!
//! With `snapshot-log-profile` disabled, start/finish inline to no-ops and all
//! metrics stay zero. Enabled timings include observer overhead; nested caller
//! regions may overlap and are not an exclusive CPU decomposition. They must
//! never be substituted for an ordinary uninstrumented throughput measurement.

use std::time::Instant;

#[derive(Clone, Copy, Debug, Default)]
pub struct Metrics {
    pub put_ns: u64,
    pub prepare_ns: u64,
    pub encode_ns: u64,
    pub checkpoint_ns: u64,
    pub append_ns: u64,
    pub publish_ns: u64,
    pub staged_clear_ns: u64,
    /// Completed instrumented put regions, including failed puts if timed.
    pub put_calls: u64,
    /// Completed publication regions; no-op/aborted transactions are excluded.
    pub commits: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Stage {
    Put,
    Prepare,
    Encode,
    Checkpoint,
    Append,
    Publish,
    StagedClear,
}

#[inline(always)]
pub(super) fn start() -> Option<Instant> {
    #[cfg(feature = "snapshot-log-profile")]
    {
        Some(Instant::now())
    }
    #[cfg(not(feature = "snapshot-log-profile"))]
    {
        None
    }
}

#[inline(always)]
pub(super) fn finish(metrics: &mut Metrics, stage: Stage, started: Option<Instant>) {
    #[cfg(feature = "snapshot-log-profile")]
    {
        let Some(started) = started else {
            return;
        };
        let elapsed = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        let target = match stage {
            Stage::Put => {
                metrics.put_calls = metrics.put_calls.saturating_add(1);
                &mut metrics.put_ns
            }
            Stage::Prepare => &mut metrics.prepare_ns,
            Stage::Encode => &mut metrics.encode_ns,
            Stage::Checkpoint => &mut metrics.checkpoint_ns,
            Stage::Append => &mut metrics.append_ns,
            Stage::Publish => {
                metrics.commits = metrics.commits.saturating_add(1);
                &mut metrics.publish_ns
            }
            Stage::StagedClear => &mut metrics.staged_clear_ns,
        };
        *target = target.saturating_add(elapsed);
    }
    #[cfg(not(feature = "snapshot-log-profile"))]
    {
        let _ = (metrics, stage, started);
    }
}
