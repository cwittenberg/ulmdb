//! Move process-local progress between successive owners of one physical root.
//! The token owns no writer lease and cannot authorize deletion by itself.
use super::{MaintenancePolicy, MaintenanceState};
use crate::{Environment, Error, Result};
use std::{fs::File, mem::ManuallyDrop, sync::atomic::Ordering};

/// Opaque, process-bound maintenance progress for the same physical environment.
/// This permits facades with short-lived native writers to continue marking
/// rather than restart on every transaction. It cannot be serialized or cloned.
/// All deletion still requires the scheduler's fresh protected-root discovery.
pub struct MaintenanceTransfer {
    creator: u32,
    directory: ManuallyDrop<Option<File>>,
    identity: ulmdb_platform::FileIdentity,
    uuid: [u8; 16],
    epoch: u64,
    format: u16,
    checkpoint: [u8; 64],
    state: ManuallyDrop<MaintenanceState>,
}

impl Drop for MaintenanceTransfer {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            let state = std::mem::replace(&mut self.state, ManuallyDrop::new(Default::default()));
            drop(ManuallyDrop::into_inner(state));
            let directory = std::mem::replace(&mut self.directory, ManuallyDrop::new(None));
            drop(ManuallyDrop::into_inner(directory));
        }
    }
}
impl MaintenanceTransfer {
    /// Whether transferred progress has work independent of a fresh trigger.
    /// This observation takes no locks and retains no physical snapshots.
    pub fn has_pending_work(&self) -> Result<bool> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited maintenance transfer"));
        }
        if self.state.rewrite.is_some() || self.state.reclaim_cursor.is_some() { return Ok(true); }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if let Some(scheduler) = &self.state.resumable { return scheduler.has_pending_work(); }
        Ok(false)
    }
}

impl Environment {
    /// Suspend this owner's policy and move its progress into a non-cloneable
    /// token. Existing physical snapshots are released before transfer; the
    /// token retains only immutable metadata and its directory identity handle.
    pub fn take_maintenance(&self) -> Result<MaintenanceTransfer> {
        self.require_coordinated()?;
        self.require_file_backend()?;
        self.require_writable()?;
        self.check_file_namespace()?;
        let _writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        self.check_file_namespace()?;
        let directory = ulmdb_platform::open_directory(&self.directory)?;
        self.check_file_namespace()?;
        let identity = ulmdb_platform::file_identity(&directory)?;
        if !identity.is_dir() {
            return Err(Error::Corrupt("maintenance transfer directory"));
        }
        let current = self.current.load_full();
        let pin = current.physical_pin.as_ref().ok_or(Error::InvalidInput(
            "maintenance transfer requires physical pin",
        ))?;
        if !pin.matches_directory_handle(&directory)? {
            return Err(Error::Corrupt("maintenance transfer directory binding"));
        }
        let checkpoint = *pin.root().checkpoint();
        let mut state = self.maintenance.lock().map_err(|_| Error::Busy)?;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if let Some(scheduler) = state.resumable.as_mut() {
            scheduler.release_optional_pin();
        }
        let moved = std::mem::take(&mut *state);
        self.maintenance_enabled.store(false, Ordering::Release);
        Ok(MaintenanceTransfer {
            creator: std::process::id(),
            directory: ManuallyDrop::new(Some(directory)),
            identity,
            uuid: current.manifest.uuid,
            epoch: current.manifest.epoch,
            format: current.manifest.format_minor,
            checkpoint,
            state: ManuallyDrop::new(moved),
        })
    }

    /// Resume transferred progress only in the same process, directory inode,
    /// UUID, epoch and file format. A changed head discards forward coverage;
    /// authenticated retired inventories remain merely cleanup candidates.
    /// The receiving owner must have its maintenance policy disabled.
    pub fn restore_maintenance(&self, mut transfer: MaintenanceTransfer) -> Result<()> {
        // Reject inherited authority before any lock or field destructor.
        if transfer.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited maintenance transfer"));
        }
        self.require_coordinated()?;
        self.require_file_backend()?;
        self.require_writable()?;
        self.check_file_namespace()?;
        let _writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        self.check_file_namespace()?;
        let directory = ulmdb_platform::open_directory(&self.directory)?;
        self.check_file_namespace()?;
        let held = ulmdb_platform::file_identity(
            transfer
                .directory
                .as_ref()
                .ok_or(Error::InvalidInput("consumed maintenance transfer"))?,
        )?;
        let actual = ulmdb_platform::file_identity(&directory)?;
        let current = self.current.load_full();
        let pin = current.physical_pin.as_ref().ok_or(Error::InvalidInput(
            "maintenance transfer requires physical pin",
        ))?;
        if !held.same_object(&transfer.identity)
            || !actual.same_object(&transfer.identity)
            || !pin.matches_directory_handle(&directory)?
            || !actual.is_dir()
            || current.manifest.uuid != transfer.uuid
            || current.manifest.epoch != transfer.epoch
            || current.manifest.format_minor != transfer.format
        {
            return Err(Error::InvalidInput("foreign maintenance transfer"));
        }
        let checkpoint = current
            .physical_pin
            .as_ref()
            .ok_or(Error::InvalidInput(
                "maintenance transfer requires physical pin",
            ))?
            .root()
            .checkpoint();
        let mut state = self.maintenance.lock().map_err(|_| Error::Busy)?;
        if state.policy != MaintenancePolicy::Disabled {
            return Err(Error::Busy);
        }
        if checkpoint != &transfer.checkpoint {
            // The unobserved interval may contain rollback/import. Neither an
            // equal sequence nor a numeric ID floor proves forward ancestry.
            transfer.state.rewrite = None;
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            if let Some(scheduler) = transfer.state.resumable.as_mut() {
                scheduler.reset_transfer_window();
            }
        }
        *state = ManuallyDrop::into_inner(std::mem::replace(
            &mut transfer.state,
            ManuallyDrop::new(Default::default()),
        ));
        self.maintenance_enabled.store(
            state.policy != MaintenancePolicy::Disabled,
            Ordering::Release,
        );
        Ok(())
    }
}
