//! An owned, immutable Resident checkpoint export. Capturing this token copies
//! only bounded slot metadata; source writers need not stay locked during IO.
use super::{
    View, checkpoint,
    wal_codec::{Boot, SlotState},
};
use crate::{Error, Reference, Result, hybrid::redo::WalReceipt};
use std::{fs::File, path::Path, sync::Arc};

/// Atomically install an owned completed snapshot without replacing any target.
/// An error after installation is explicitly uncertain; its bytes remain valid.
#[allow(unsafe_code)]
pub fn install_snapshot_export(
    source_directory: &File,
    target_parent: &File,
    target_name: &std::ffi::OsStr,
) -> Result<()> {
    let image = ulmdb_platform::open_relative(
        source_directory,
        std::ffi::OsStr::new("image"),
        ulmdb_platform::RelativeKind::Directory,
    )?;
    ulmdb_platform::owned_export::install(source_directory, &image, target_parent, target_name)
        .map_err(|error| {
            if ulmdb_platform::rename_outcome_unknown(&error) {
                Error::OutcomeUnknown
            } else {
                Error::Io(error)
            }
        })
}

pub struct SnapshotExport {
    boot: Boot,
    creator: u32,
    slots: Vec<SlotState>,
    selected: WalReceipt,
    root: Reference,
    next_database: u64,
    image: View,
    capacity: u64,
    compact: bool,
    context: Option<Arc<crate::transform::TransformContext>>,
    _metadata: super::private_quota::Reservation,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotExportReport {
    pub uuid: [u8; 16],
    pub sequence: u64,
    pub commit: u128,
    pub source_lsn: u64,
    pub target_lsn: u64,
    pub live_slots: usize,
    pub wal_capacity: u64,
}
impl SnapshotExport {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        boot: Boot,
        slots: &[SlotState],
        selected: WalReceipt,
        root: Reference,
        next_database: u64,
        image: &View,
        capacity: u64,
        compact: bool,
        context: Option<Arc<crate::transform::TransformContext>>,
    ) -> Result<Self> {
        if slots.len() != boot.slots()? || image.slot_capacity() != slots.len() {
            return Err(Error::Corrupt("resident export geometry"));
        }
        let mut live = 0;
        for (slot, state) in slots.iter().enumerate() {
            let present = image.version(slot as u32).map(|v| v.key().generation);
            let expected = match state {
                SlotState::Live(g) => Some(*g),
                _ => None,
            };
            if present != expected {
                return Err(Error::Corrupt("resident export view/state"));
            }
            live += usize::from(expected.is_some());
        }
        if live != image.member_count() {
            return Err(Error::Corrupt("resident export membership"));
        }
        let charge = |count: usize| -> Result<u64> {
            count
                .checked_mul(std::mem::size_of::<SlotState>())
                .and_then(|n| {
                    n.checked_add(std::mem::size_of::<Self>() + 4 * std::mem::size_of::<usize>())
                })
                .map(|n| n as u64)
                .ok_or(Error::LimitExceeded)
        };
        let mut metadata = image.pool.private_quota.reserve(charge(slots.len())?)?;
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(slots.len())
            .map_err(|_| Error::LimitExceeded)?;
        owned.extend_from_slice(slots);
        metadata.reconcile(charge(owned.capacity())?)?;
        Ok(Self {
            boot,
            creator: std::process::id(),
            slots: owned,
            selected,
            root,
            next_database,
            image: image.clone(),
            capacity,
            compact,
            context,
            _metadata: metadata,
        })
    }
    /// Write into an already-created, privately owned empty layout. The caller
    /// owns cleanup/publication of that layout; existing WAL files are rejected.
    pub fn write_to(&self, directory: &Path) -> Result<SnapshotExportReport> {
        self.check_process()?;
        let wal = directory.join("RESIDENT.wal");
        if wal.try_exists()? {
            return Err(Error::InvalidInput("resident export WAL exists"));
        }
        crate::transform::create_configuration(directory, self.boot.uuid, self.context.as_ref())?;
        let seed = checkpoint::create(
            directory,
            self.boot,
            &self.slots,
            self.selected,
            self.root,
            self.next_database,
            &self.image,
            self.context.clone(),
        )?;
        let file = ulmdb_platform::create_private_file(&wal, true)?;
        let report = self.write_wal(file, seed)?;
        ulmdb_platform::sync_path(directory)?;
        Ok(report)
    }
    fn check_process(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited Resident snapshot export"));
        }
        Ok(())
    }
    /// Write only into a genuine fresh owned output. All files/cleanup/fences
    /// remain relative to the held image; no staging pathname is rediscovered.
    pub fn write_to_owned(
        &self,
        output: &mut ulmdb_platform::owned_export::OwnedExport,
    ) -> Result<SnapshotExportReport> {
        self.check_process()?;
        let lock = output.create_file(std::ffi::OsStr::new("LOCK"))?;
        ulmdb_platform::sync_file(&lock)?;
        for name in ["segments", "commits", "transactions", "cohorts"] {
            output.create_directory(std::ffi::OsStr::new(name))?;
        }
        if let Some(context) = &self.context {
            let mut file = output.create_file(std::ffi::OsStr::new("TRANSFORM"))?;
            context.write_encoded(
                &mut file,
                crate::transform::ObjectKind::Configuration,
                0,
                &self.boot.uuid,
            )?;
            ulmdb_platform::sync_file(&file)?;
        }
        let seed = checkpoint::create_in_export(
            output,
            self.boot,
            &self.slots,
            self.selected,
            self.root,
            self.next_database,
            &self.image,
            self.context.clone(),
        )?;
        let file = output.create_file(std::ffi::OsStr::new("RESIDENT.wal"))?;
        let report = self.write_wal(file, seed)?;
        output.sync()?;
        Ok(report)
    }
    fn write_wal(&self, file: File, seed: checkpoint::Seed) -> Result<SnapshotExportReport> {
        let (header, head) = if self.compact {
            super::journal_v2::Journal::snapshot_headers(
                self.boot,
                self.capacity,
                seed,
                &self.slots,
            )?
        } else {
            super::journal::Journal::snapshot_headers(self.boot, self.capacity, seed, &self.slots)?
        };
        let fence = crate::WriterFence::try_lock(file).map_err(|e| match e {
            std::fs::TryLockError::WouldBlock => Error::Busy,
            std::fs::TryLockError::Error(e) => Error::Io(e),
        })?;
        let file = crate::transform::ResidentFile::new(fence, self.context.clone());
        file.clear(0, self.capacity)?;
        file.write_all_at(&header, 0)?;
        for at in [65536, 131072] {
            file.write_all_at(&head, at)?;
        }
        file.sync_all()?;
        Ok(SnapshotExportReport {
            uuid: self.boot.uuid,
            sequence: self.selected.sequence,
            commit: self
                .boot
                .commit
                .checked_add(self.selected.sequence as u128)
                .ok_or(Error::LimitExceeded)?,
            source_lsn: self.selected.lsn,
            target_lsn: seed.lsn,
            live_slots: self.image.member_count(),
            wal_capacity: self.capacity,
        })
    }
}
