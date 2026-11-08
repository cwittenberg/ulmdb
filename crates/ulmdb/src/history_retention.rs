//! Explicit history-expiration configuration. This never changes business roots.
use crate::{Environment, Error, Result};
use std::sync::atomic::Ordering;
impl Environment {
    /// Request bounded orphan discovery on a subsequent ordinary maintenance
    /// step. This changes no policy or business root and performs no cleanup.
    /// Disabled owners remain disabled. Useful after restoring process-local
    /// progress that predates an abandoned preparation.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub fn request_history_maintenance(&self) -> Result<()> {
        self.require_coordinated()?;
        self.require_file_backend()?;
        self.require_writable()?;
        self.check_file_namespace()?;
        let _writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        self.maintenance
            .lock()
            .map_err(|_| Error::Busy)?
            .request_orphan_discovery();
        Ok(())
    }
    /// Default KeepAll preserves historical behavior. Before enabling finite
    /// retention, keep named snapshot pins for closed copies needed as future
    /// exact incremental bases. Active readers/copies and named pins clamp expiry.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub fn set_history_policy(
        &self,
        policy: ulmdb_storage::HistoryPolicy,
    ) -> Result<ulmdb_storage::HistoryAvailability> {
        self.require_coordinated()?;
        self.require_file_backend()?;
        self.require_writable()?;
        self.check_file_namespace()?;
        policy.validate()?;
        let _writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        let current = self.current.load_full();
        let cp = current
            .physical_pin
            .as_ref()
            .ok_or(Error::InvalidInput(
                "history policy requires a physical pin",
            ))?
            .root()
            .checkpoint();
        let gate = self
            .coordination
            .as_ref()
            .ok_or(Error::InvalidInput("history policy requires coordination"))?
            .try_maintenance()?;
        let info = gate.configure_history(cp, policy)?;
        if let Ok(mut state) = self.maintenance.lock() {
            state.note_history_configuration();
        }
        Ok(info)
    }
    /// Advance optional maintenance without a fake business commit. Each call
    /// releases writer/Capture before returning; inactive policies remain inert.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub fn step_history_maintenance(&self) -> Result<crate::MaintenanceStatus> {
        self.require_coordinated()?;
        self.require_file_backend()?;
        self.require_writable()?;
        self.check_file_namespace()?;
        let _writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        if let Ok(mut state) = self.maintenance.lock() {
            state.request_orphan_discovery();
        }
        self.automatic_reclaim();
        self.maintenance_status()
    }
    pub fn history_availability(&self) -> Result<ulmdb_storage::HistoryAvailability> {
        self.require_file_backend()?;
        self.check_file_namespace()?;
        let current = self.current.load();
        ulmdb_storage::read_history_availability(
            &self.directory,
            current.manifest.uuid,
            current.manifest.epoch,
            current.segments.transform().cloned(),
        )
    }
}
