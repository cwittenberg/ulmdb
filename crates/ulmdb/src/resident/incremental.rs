//! Explicit experimental writer control; no serving query checks this state.
use super::{Environment, IncrementalLimits, IncrementalMetrics, RedoFormat, graph_certificate};
use crate::{Error, Result};

impl Environment {
    /// Build a source-bound graph certificate for subsequent ordinary writes.
    /// This explicit cold operation runs the full existing validator. The
    /// default writer remains unchanged; reopening requires enabling again.
    /// Limits bound this experimental graph's accounted state and work, not RSS.
    pub fn enable_incremental(&self, limits: IncrementalLimits) -> Result<()> {
        if self.redo_format() == RedoFormat::LiteralV1 {
            return Err(Error::InvalidInput("incremental writer requires RL2"));
        }
        let mut writer = self.writer.lock().map_err(|_| Error::Busy)?;
        if writer.poisoned || writer.journal.is_poisoned() || self.pool.stats()?.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        let snapshot = self.current.load_full();
        let certificate = match graph_certificate::Certificate::bootstrap(&snapshot, limits) {
            Ok(certificate) => certificate,
            Err(error) => {
                if matches!(error, Error::Corrupt(_)) {
                    writer.poisoned = true;
                    self.pool.quarantine();
                }
                return Err(error);
            }
        };
        writer.incremental = Some(certificate);
        writer.incremental_limits = Some(limits);
        writer.incremental_metrics = Default::default();
        Ok(())
    }

    /// Last acknowledged incremental preparation, observed outside held writes.
    /// Cold bootstrap/Collect work is separate; no read path updates counters.
    pub fn incremental_metrics(&self) -> Result<Option<IncrementalMetrics>> {
        let writer = self.writer.lock().map_err(|_| Error::Busy)?;
        Ok(writer
            .incremental
            .as_ref()
            .map(|_| writer.incremental_metrics))
    }
}
