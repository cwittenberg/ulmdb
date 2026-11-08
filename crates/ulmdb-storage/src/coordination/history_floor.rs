//! Genuine namespace-bound, resumable history-floor proof. No data pin/gate is
//! retained between calls; each operation needs the current maintenance lease.
use super::{Inner, MaintenanceLease, PhysicalPin};
use crate::{Error, HistoryAvailability, HistoryPolicy, Manifest, Result};
use std::{mem::ManuallyDrop, sync::Arc};
pub struct HistoryFloorBuilder {
    inner: ManuallyDrop<Option<Arc<Inner>>>,
    current: ManuallyDrop<Option<Manifest>>,
    latest: [u8; 64],
    target: u64,
    revision: u128,
    creator: u32,
}
impl Drop for HistoryFloorBuilder {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            drop(ManuallyDrop::into_inner(std::mem::replace(
                &mut self.current,
                ManuallyDrop::new(None),
            )));
            drop(ManuallyDrop::into_inner(std::mem::replace(
                &mut self.inner,
                ManuallyDrop::new(None),
            )));
        }
    }
}
impl HistoryFloorBuilder {
    fn check(&self) -> Result<&Arc<Inner>> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited history floor builder"));
        }
        let i = self
            .inner
            .as_ref()
            .ok_or(Error::InvalidInput("consumed history floor builder"))?;
        i.check()?;
        Ok(i)
    }
    pub fn current(&self) -> Result<&Manifest> {
        self.check()?;
        self.current
            .as_ref()
            .ok_or(Error::InvalidInput("history floor has no root"))
    }
    pub fn target_sequence(&self) -> u64 {
        self.target
    }
    pub fn lower_target(&mut self, sequence: u64) -> Result<()> {
        self.check()?;
        self.target = self.target.min(sequence);
        Ok(())
    }
    pub fn ready(&self) -> Result<bool> {
        Ok(self.current()?.sequence <= self.target)
    }
    pub fn observe_forward(&mut self, before: &PhysicalPin, after: &PhysicalPin) -> Result<()> {
        let i = self.check()?;
        before.inner.check()?;
        after.inner.check()?;
        if !before.matches_directory(&i.directory)?
            || !after.matches_directory(&i.directory)?
            || before.root.checkpoint != self.latest
        {
            return Err(Error::InvalidInput("foreign history forward pins"));
        }
        let b = &before.root.manifest;
        let a = &after.root.manifest;
        let business = a.sequence == b.sequence.checked_add(1).ok_or(Error::LimitExceeded)?
            && a.parent == b.commit;
        let maintenance = a.sequence == b.sequence && a.commit == b.commit && a.parent == b.parent;
        if a.uuid != b.uuid
            || a.uuid != i.uuid
            || a.epoch != b.epoch
            || a.previous != b.generation
            || (!business && !maintenance)
            || crate::publication::selected_checkpoint_only(&i.directory)? != after.root.checkpoint
        {
            return Err(Error::InvalidInput(
                "history observation is not selected forward lineage",
            ));
        }
        self.latest = after.root.checkpoint;
        Ok(())
    }
    pub fn advance(&mut self, gate: &MaintenanceLease, max_inventory_entries: usize) -> Result<()> {
        let i = self.check()?;
        gate.check()?;
        if (i.identity != gate.inner.identity
            || i.uuid != gate.inner.uuid
            || i.directory != gate.inner.directory)
        {
            return Err(Error::InvalidInput("foreign history maintenance lease"));
        }
        if crate::publication::selected_checkpoint_only(&i.directory)? != self.latest {
            return Err(Error::Busy);
        }
        let old = self.current()?;
        if old.sequence <= self.target {
            return Ok(());
        }
        if old.previous == 0 {
            return Err(Error::InvalidInput("retained history chain unavailable"));
        }
        let next = crate::read_generation_with_transform(
            &i.directory,
            old.previous,
            old.format_minor,
            i.transform.clone(),
        )?;
        if next.segments.len() > max_inventory_entries {
            return Err(Error::LimitExceeded);
        }
        if next.uuid != old.uuid
            || next.epoch != old.epoch
            || next.sequence > old.sequence
            || next.generation == old.generation
            || (next.sequence < old.sequence && old.parent != next.commit)
            || (next.sequence == old.sequence
                && (old.commit != next.commit || old.parent != next.parent))
        {
            return Err(Error::Corrupt("history floor lineage"));
        }
        *self.current = Some(next);
        Ok(())
    }
}
impl MaintenanceLease {
    pub fn configure_history(
        &self,
        expected: &[u8; 64],
        policy: HistoryPolicy,
    ) -> Result<HistoryAvailability> {
        self.check()?;
        policy.validate()?;
        if crate::publication::selected_checkpoint_only(&self.inner.directory)? != *expected {
            return Err(Error::Busy);
        }
        let m = crate::read_checkpoint_with_transform(
            &self.inner.directory,
            expected,
            self.inner.transform.clone(),
        )?;
        if m.uuid != self.inner.uuid {
            return Err(Error::Corrupt("foreign history policy realm"));
        }
        let mut info = crate::read_history_availability(
            &self.inner.directory,
            m.uuid,
            m.epoch,
            self.inner.transform.clone(),
        )?;
        info.policy = policy;
        info.revision = crate::new_identity()?;
        crate::history_lifecycle::write(&self.inner.directory, m.uuid, m.epoch, &info)?;
        self.check()?;
        Ok(info)
    }
    pub fn begin_history_floor(
        &self,
        expected: &[u8; 64],
        target: u64,
    ) -> Result<HistoryFloorBuilder> {
        self.check()?;
        if crate::publication::selected_checkpoint_only(&self.inner.directory)? != *expected {
            return Err(Error::Busy);
        }
        let current = crate::read_checkpoint_with_transform(
            &self.inner.directory,
            expected,
            self.inner.transform.clone(),
        )?;
        if current.uuid != self.inner.uuid || target == 0 || target > current.sequence {
            return Err(Error::InvalidInput("invalid history floor target"));
        }
        let info = crate::read_history_availability(
            &self.inner.directory,
            current.uuid,
            current.epoch,
            self.inner.transform.clone(),
        )?;
        if !matches!(info.policy, HistoryPolicy::Retain { .. }) {
            return Err(Error::InvalidInput("history retention is KeepAll"));
        }
        Ok(HistoryFloorBuilder {
            inner: ManuallyDrop::new(Some(self.inner.clone())),
            current: ManuallyDrop::new(Some(current)),
            latest: *expected,
            target,
            revision: info.revision,
            creator: std::process::id(),
        })
    }
    pub fn publish_history_floor(
        &mut self,
        builder: &HistoryFloorBuilder,
        max_inventory_entries: usize,
    ) -> Result<HistoryAvailability> {
        self.check()?;
        let i = builder.check()?;
        if (i.identity != self.inner.identity
            || i.uuid != self.inner.uuid
            || i.directory != self.inner.directory)
            || !builder.ready()?
            || crate::publication::selected_checkpoint_only(&i.directory)? != builder.latest
        {
            return Err(Error::Busy);
        }
        let floor = builder.current()?;
        let mut info = crate::read_history_availability(
            &i.directory,
            i.uuid,
            floor.epoch,
            i.transform.clone(),
        )?;
        if info.revision != builder.revision || floor.sequence < info.floor_sequence {
            return Err(Error::Busy);
        }
        // A genuine independent copy/export pin also clamps the floor. Holding
        // Capture EX excludes a new pin until the durable floor has published.
        let scan = self.scan()?;
        let mut inventory_entries = scan.live_roots().iter().try_fold(0usize, |n, r| {
            n.checked_add(r.manifest.segments.len())
                .ok_or(Error::LimitExceeded)
        })?;
        if inventory_entries > max_inventory_entries {
            return Err(Error::LimitExceeded);
        }
        if scan
            .live_roots()
            .iter()
            .any(|r| r.manifest.sequence < floor.sequence)
        {
            return Err(Error::Busy);
        }
        scan.cleanup_stale()?;
        let retained = i.directory.join("retained");
        match std::fs::read_dir(&retained) {
            Ok(entries) => {
                for (n, entry) in entries.enumerate() {
                    if n >= 4096 {
                        return Err(Error::LimitExceeded);
                    }
                    let entry = entry?;
                    if !entry
                        .file_name()
                        .to_str()
                        .is_some_and(|n| n.ends_with(".pin"))
                    {
                        continue;
                    }
                    let mode = crate::read_mode::ReadMode::Sync;
                    let mut file = mode.open(&entry.path())?;
                    use std::io::Read;
                    let mut cp = [0; 64];
                    if file.metadata()?.len() != 64 {
                        return Err(Error::Corrupt("retained history pin"));
                    }
                    file.read_exact(&mut cp)?;
                    mode.check(&entry.path(), &file)?;
                    let root = crate::read_checkpoint_with_transform(
                        &i.directory,
                        &cp,
                        i.transform.clone(),
                    )?;
                    inventory_entries = inventory_entries
                        .checked_add(root.segments.len())
                        .ok_or(Error::LimitExceeded)?;
                    if inventory_entries > max_inventory_entries {
                        return Err(Error::LimitExceeded);
                    }
                    if root.uuid != i.uuid {
                        return Err(Error::Corrupt("foreign retained history root"));
                    }
                    if root.sequence < floor.sequence {
                        return Err(Error::Busy);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        info.floor_sequence = floor.sequence;
        info.floor_checkpoint = Some(crate::checkpoint_bytes(floor)?);
        crate::history_lifecycle::write(&i.directory, i.uuid, floor.epoch, &info)?;
        self.check()?;
        Ok(info)
    }
}
