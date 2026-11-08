use super::super::Environment;
use super::*;
impl Environment {
    /// Explicit writer-only cache. Cold replay always runs the original full
    /// auditor, and reopen starts disabled. This method performs no graph walk.
    pub fn enable_collect_cache(&self, limits: Limits) -> Result<()> {
        limits.validate()?;
        let mut writer = self.writer.lock().map_err(|_| Error::Busy)?;
        if writer.poisoned || writer.journal.is_poisoned() || self.pool.stats()?.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        if writer.prefix_audit_cache.is_some() {
            return Err(Error::InvalidInput("Collect cache already enabled"));
        }
        writer.prefix_audit_cache = Some(Cache::new(&self.pool, limits)?);
        Ok(())
    }
    pub fn disable_collect_cache(&self) -> Result<()> {
        self.writer
            .lock()
            .map_err(|_| Error::Busy)?
            .prefix_audit_cache = None;
        Ok(())
    }
    /// Observe outside a held WriteTxn; counted bytes are not allocator/RSS.
    pub fn collect_cache_stats(&self) -> Result<Option<Stats>> {
        Ok(self
            .writer
            .lock()
            .map_err(|_| Error::Busy)?
            .prefix_audit_cache
            .as_ref()
            .map(|c| c.stats))
    }
}
