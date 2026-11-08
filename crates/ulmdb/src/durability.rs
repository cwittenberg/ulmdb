//! Runtime file policy and explicit selected-file synchronization.
use crate::{Durability, Environment, Error, Result, WriteTxn};
use std::sync::atomic::Ordering;

impl Environment {
    pub(crate) fn cached_file_durability(
        &self,
    ) -> Result<Option<ulmdb_storage::FileDurabilityState>> {
        Ok(self
            .file_durability
            .lock()
            .map_err(|_| Error::OutcomeUnknown)?
            .clone())
    }
    pub(crate) fn remember_file_durability(
        &self,
        selected: ulmdb_storage::FileDurabilityState,
    ) -> Result<()> {
        if !selected.matches(&self.begin_read().file_checkpoint()?) {
            self.poisoned.store(true, Ordering::Release);
            return Err(Error::OutcomeUnknown);
        }
        self.remember_pending_file_durability(selected)
    }
    /// A pending PreviousSession serves A while the actual selector is B.
    /// Only its authenticated PreviousSelection supplies these private facts.
    pub(crate) fn remember_pending_file_durability(
        &self,
        selected: ulmdb_storage::FileDurabilityState,
    ) -> Result<()> {
        *self
            .file_durability
            .lock()
            .map_err(|_| Error::OutcomeUnknown)? = Some(selected);
        Ok(())
    }
    /// Runtime policy for this owner. Reopening starts with Full; a previous
    /// weaker publication remains explicit in storage's selected-state metadata.
    pub fn durability(&self) -> Durability {
        match self.durability.load(Ordering::Acquire) {
            1 => Durability::NoMetaSync,
            2 => Durability::NoSync,
            _ => Durability::Full,
        }
    }
    /// Change subsequent data/metadata decisions without taking the writer
    /// mutex. This is valid while a writer or prepared owner is retained.
    /// Already prepared data is not refenced by changing this setting.
    pub fn set_durability(&self, policy: Durability) -> Result<()> {
        self.require_file_backend()?;
        self.require_writable()?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        self.check_file_namespace()?;
        let encoded = match policy {
            Durability::Full => 0,
            Durability::NoMetaSync => 1,
            Durability::NoSync => 2,
        };
        self.durability.store(encoded, Ordering::Release);
        Ok(())
    }
    /// Flush current and authenticated previous file assets, without taking
    /// this owner's writer mutex. Active/prepared writers remain unpublished.
    /// This does not validate user graphs or certify power-loss behavior.
    pub fn sync(&self) -> Result<ulmdb_storage::FileSyncReport> {
        self.sync_with_force(true)?
            .ok_or(Error::Corrupt("forced sync was skipped"))
    }
    /// Return None when a non-forced call is suppressed by NoSync. A forced
    /// call fences selected roots even while a writer is dirty or prepared.
    pub fn sync_with_force(&self, force: bool) -> Result<Option<ulmdb_storage::FileSyncReport>> {
        self.require_file_backend()?;
        self.require_writable()?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        self.check_file_namespace()?;
        if !force && self.durability() == Durability::NoSync {
            return Ok(None);
        }
        if let Some(external) = &self.current.load().external {
            return external
                .sync(self.current.load().segments.transform().cloned())
                .map(Some);
        }
        self.coordination
            .as_ref()
            .ok_or(Error::InvalidInput(
                "coordinated file sync requires a physical namespace capability",
            ))?
            .sync()
            .map(Some)
            .map_err(Into::into)
    }
}

impl WriteTxn<'_> {
    /// Effective policy if preparation/selection were performed now.
    pub fn durability(&self) -> Durability {
        self.env.durability().weaken(self.local_durability)
    }
    /// Add a local weakening. Savepoint abort restores the entry policy;
    /// accepting a savepoint retains the child's policy even when clean.
    pub fn weaken_durability(&mut self, local: Durability) -> Result<()> {
        self.live()?;
        self.env.require_file_backend()?;
        self.local_durability = self.local_durability.weaken(local);
        Ok(())
    }
}
