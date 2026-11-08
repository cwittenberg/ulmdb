//! Explicit resident literal family, two ordered durability fences, full WAL retained.
use crate::WriterFence;
use crate::resident::capacity::CapacityProbe;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
mod fault;
mod scan;
mod tail;
mod wire;
use crate::hybrid::redo::WalReceipt;
use crate::resident::capture::{CaptureGate, CaptureLimits, ReadCapture};
use crate::resident::wal_codec::{self, Boot, Record, ReplayState};
use crate::{Error, Result};
use fault::point;
pub use scan::Recovery;
use std::{
    fs::{File, OpenOptions},
    path::Path,
};
use wire::{BLOCK, HEADS, Witness};
pub struct Journal {
    capture: Option<CaptureGate>,
    file: crate::transform::ResidentFile,
    capacity_probe: CapacityProbe,
    boot: Boot,
    capacity: u64,
    state: ReplayState,
    slot: usize,
    cached: [[u8; BLOCK]; 2],
    poisoned: bool,
}
impl Journal {
    pub fn capture_snapshot(
        &self,
        view: &crate::resident::View,
    ) -> Result<crate::resident::SnapshotExport> {
        if self.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        crate::resident::SnapshotExport::new(
            self.boot,
            self.state.slots(),
            self.state.selected(),
            self.state.root(),
            self.state.next_database(),
            view,
            self.capacity,
            false,
            self.file.context().cloned(),
        )
    }
    pub(super) fn snapshot_headers(
        boot: Boot,
        capacity: u64,
        seed: crate::resident::checkpoint::Seed,
        _slots: &[wal_codec::SlotState],
    ) -> Result<([u8; BLOCK], [u8; BLOCK])> {
        seed.validate(boot)?;
        Ok((
            wire::header_seeded(boot, capacity, seed)?,
            wire::head(
                boot,
                capacity,
                Witness {
                    receipt: seed.receipt(),
                    commit: boot
                        .commit
                        .checked_add(seed.sequence as u128)
                        .ok_or(Error::LimitExceeded)?,
                },
            ),
        ))
    }
    pub fn create(path: &Path, boot: Boot, capacity: u64) -> Result<Self> {
        Self::create_with_transform(path, boot, capacity, None)
    }
    pub fn create_with_transform(
        path: &Path,
        boot: Boot,
        capacity: u64,
        context: Option<std::sync::Arc<crate::transform::TransformContext>>,
    ) -> Result<Self> {
        let header = wire::header(boot, capacity)?;
        let state = ReplayState::new(boot)?;
        let witness = Witness {
            receipt: state.selected(),
            commit: state.commit(),
        };
        let encoded = wire::head(boot, capacity, witness);
        let file = ulmdb_platform::create_private_file(path, true)?;
        let file = crate::transform::ResidentFile::new(lock(file)?, context);
        point("resident.create.before_zero")?;
        tail::clear(&file, 0, capacity)?;
        point("resident.create.after_zero")?;
        file.write_all_at(&header, 0)?;
        point("resident.create.after_header")?;
        for (index, at) in HEADS.into_iter().enumerate() {
            file.write_all_at(&encoded, at)?;
            point(if index == 0 {
                "resident.create.after_head0"
            } else {
                "resident.create.after_head1"
            })?;
        }
        file.sync_all()?;
        point("resident.create.after_file_sync")?;
        sync_parent(path)?;
        point("resident.create.after_parent_sync")?;
        let capacity_probe = CapacityProbe::for_file(&file);
        Ok(Self {
            capture: None,
            file,
            capacity_probe,
            boot,
            capacity,
            state,
            slot: 0,
            cached: [encoded; 2],
            poisoned: false,
        })
    }
    /// The callback must stream ALL frames, reconstruct fresh maps, certify every
    /// retirement and validate the selected complete typed graph before returning.
    /// It runs before every heal/clear/ready mutation. No checkpoint/trim fallback.
    pub fn open_checked<T>(
        path: &Path,
        validate: impl FnOnce(&mut Recovery<'_>) -> Result<T>,
    ) -> Result<(Self, T)> {
        Self::open_checked_with_transform(path, None, validate)
    }
    pub fn open_checked_with_transform<T>(
        path: &Path,
        context: Option<std::sync::Arc<crate::transform::TransformContext>>,
        validate: impl FnOnce(&mut Recovery<'_>) -> Result<T>,
    ) -> Result<(Self, T)> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let file = crate::transform::ResidentFile::new(lock(file)?, context);
        let mut capture_header = [0; BLOCK];
        file.read_exact_at(&mut capture_header, 0)?;
        let (capture_boot, _) = wire::decode_header(&capture_header)?;
        let capture = crate::resident::capture::protocol(&capture_header)?
            .map(|id| {
                CaptureGate::writer(
                    path.parent()
                        .ok_or(Error::InvalidInput("Resident WAL parent"))?,
                    capture_boot.uuid,
                    id,
                )
            })
            .transpose()?;
        let capture_guard = capture.as_ref().map(CaptureGate::exclusive).transpose()?;
        let selection = scan::select(&file)?;
        let mut recovery = Recovery::new(
            &file,
            selection.boot,
            selection.selected,
            selection.older,
            selection.capacity,
            selection.seed,
            path.parent()
                .ok_or(Error::InvalidInput("resident WAL parent"))?,
        )?;
        let output = validate(&mut recovery)?;
        if !recovery.complete {
            return Err(Error::InvalidInput(
                "resident recovery callback did not consume selected prefix",
            ));
        }
        let state = recovery.state;
        if selection.heal {
            point("resident.recovery.before_heal")?;
            file.write_all_at(
                &wire::head(selection.boot, selection.capacity, selection.selected),
                HEADS[selection.slot],
            )?;
            point("resident.recovery.after_heal")?;
            file.sync_data()?;
            point("resident.recovery.after_heal_sync")?;
        }
        point("resident.recovery.before_clear")?;
        tail::clear(&file, selection.selected.receipt.end()?, selection.capacity)?;
        point("resident.recovery.after_clear")?;
        file.sync_data()?;
        point("resident.recovery.after_clear_sync")?;
        point("resident.recovery.before_ready_file_sync")?;
        file.sync_all()?;
        point("resident.recovery.after_ready_file_sync")?;
        sync_parent(path)?;
        point("resident.recovery.after_ready_parent_sync")?;
        let mut cached = [[0; BLOCK]; 2];
        for (b, at) in cached.iter_mut().zip(HEADS) {
            file.read_exact_at(b, at)?;
        }
        let capacity_probe = CapacityProbe::for_file(&file);
        if let Some(g) = &capture_guard {
            g.check().map_err(|_| Error::OutcomeUnknown)?;
        }
        Ok((
            Self {
                capture,
                file,
                capacity_probe,
                boot: selection.boot,
                capacity: selection.capacity,
                state,
                slot: selection.slot,
                cached,
                poisoned: false,
            },
            output,
        ))
    }
    pub fn read_only_checked<T>(
        capture: &ReadCapture,
        limits: CaptureLimits,
        context: Option<std::sync::Arc<crate::transform::TransformContext>>,
        validate: impl FnOnce(&mut Recovery<'_>) -> Result<T>,
    ) -> Result<T> {
        limits.validate()?;
        capture.check()?;
        let path = capture.directory().join("RESIDENT.wal");
        let raw = OpenOptions::new()
            .read(true)
            .custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS)
            .open(&path)?;
        capture.validate_wal(&raw)?;
        let file = crate::transform::ResidentFile::readonly(raw, context);
        let mut header = [0; BLOCK];
        file.read_exact_at(&mut header, 0)?;
        capture.validate_header(&header)?;
        let selection = scan::select_limited(
            &file,
            wal_codec::DATA_START
                .checked_add(limits.max_replay_bytes)
                .ok_or(Error::LimitExceeded)?,
        )?;
        if selection.boot.uuid != capture.uuid() {
            return Err(Error::Corrupt("Resident capture WAL/gate UUID mismatch"));
        }
        let checkpoint = selection.seed.map(|s| s.path(capture.directory()));
        capture.preflight(
            selection.selected.receipt.end()?,
            checkpoint.as_deref(),
            limits,
        )?;
        let mut recovery = Recovery::new(
            &file,
            selection.boot,
            selection.selected,
            selection.older,
            selection.capacity,
            selection.seed,
            capture.directory(),
        )?;
        let output = validate(&mut recovery)?;
        if !recovery.complete {
            return Err(Error::InvalidInput(
                "Resident read capture did not consume selected prefix",
            ));
        }
        capture.validate_wal(&file)?;
        Ok(output)
    }
    /// Build a streamed checkpoint and a bounded empty tail. The old WAL and
    /// every old View remain unchanged until the single atomic rename. An error
    /// after rename is uncertain and permanently poisons this owner.
    pub fn capture_enabled(&self) -> bool {
        self.capture.is_some()
    }
    pub fn checkpoint(
        &mut self,
        path: &Path,
        view: &crate::resident::View,
        capacity: u64,
    ) -> Result<Self> {
        let capture = self.capture.clone();
        let guard = capture.as_ref().map(CaptureGate::exclusive).transpose()?;
        let result = self.checkpoint_inner(path, view, capacity, capture);
        crate::resident::capture::finish(result, guard.as_ref(), &mut self.poisoned)
    }
    pub fn checkpoint_and_prune(
        &mut self,
        path: &Path,
        view: &crate::resident::View,
        capacity: u64,
    ) -> Result<(Self, u64)> {
        let capture = self.capture.clone();
        let guard = capture.as_ref().map(CaptureGate::exclusive).transpose()?;
        let result = self
            .checkpoint_inner(path, view, capacity, capture)
            .and_then(|j| {
                j.prune_unlocked(path)
                    .map(|n| (j, n))
                    .map_err(|_| Error::OutcomeUnknown)
            });
        crate::resident::capture::finish(result, guard.as_ref(), &mut self.poisoned)
    }
    pub fn enable_capture_and_prune(
        &mut self,
        path: &Path,
        view: &crate::resident::View,
        capacity: u64,
    ) -> Result<(Self, u64)> {
        if self.capture.is_some() {
            return self.checkpoint_and_prune(path, view, capacity);
        }
        let (capture, guard) = CaptureGate::prepare(
            path.parent()
                .ok_or(Error::InvalidInput("Resident WAL parent"))?,
            self.boot.uuid,
        )?;
        let result = self
            .checkpoint_inner(path, view, capacity, Some(capture))
            .and_then(|j| {
                j.prune_unlocked(path)
                    .map(|n| (j, n))
                    .map_err(|_| Error::OutcomeUnknown)
            });
        crate::resident::capture::finish(result, Some(&guard), &mut self.poisoned)
    }
    fn checkpoint_inner(
        &mut self,
        path: &Path,
        view: &crate::resident::View,
        capacity: u64,
        capture: Option<CaptureGate>,
    ) -> Result<Self> {
        if self.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        wire::header(self.boot, capacity)?;
        let parent = path
            .parent()
            .ok_or(Error::InvalidInput("resident WAL parent"))?;
        let directory = ulmdb_platform::open_directory(parent)?;
        let dm = ulmdb_platform::file_identity(&directory)?;
        let identity = ulmdb_platform::file_identity(&self.file)?;
        let check = || -> Result<()> {
            if let Some(g) = &capture {
                g.check()?;
            }
            let now = ulmdb_platform::path_identity(path)?;
            let current = ulmdb_platform::path_identity(parent)?;
            if !now.is_file()
                || now.links != 1
                || now.volume != identity.volume
                || now.file_id != identity.file_id
                || !current.is_dir()
                || current.volume != dm.volume
                || current.file_id != dm.file_id
            {
                return Err(Error::Corrupt("resident checkpoint namespace changed"));
            }
            Ok(())
        };
        check()?;
        for (expected, at) in self.cached.iter().zip(HEADS) {
            let mut actual = [0; BLOCK];
            self.file.read_exact_at(&mut actual, at)?;
            if actual != *expected {
                return Err(Error::Corrupt("resident checkpoint witness changed"));
            }
        }
        if self.capacity_probe.length(&self.file)? != self.capacity {
            return Err(Error::Corrupt("resident checkpoint capacity changed"));
        }
        point("resident.checkpoint.before_image")?;
        let seed = crate::resident::checkpoint::create(
            parent,
            self.boot,
            self.state.slots(),
            self.state.selected(),
            self.state.root(),
            self.state.next_database(),
            view,
            self.file.context().cloned(),
        )?;
        let temporary = parent.join(format!(".resident-wal-{:032x}.tmp", seed.generation));
        let build = (|| {
            point("resident.checkpoint.after_image")?;
            let mut header = wire::header_seeded(self.boot, capacity, seed)?;
            if let Some(g) = &capture {
                header = crate::resident::capture::negotiated(header, g.identity());
            }
            let state = ReplayState::from_checkpoint(self.boot, seed, self.state.slots().to_vec())?;
            let encoded = wire::head(
                self.boot,
                capacity,
                Witness {
                    receipt: state.selected(),
                    commit: state.commit(),
                },
            );
            let file = ulmdb_platform::create_private_file(&temporary, true)?;
            let file =
                crate::transform::ResidentFile::new(lock(file)?, self.file.context().cloned());
            tail::clear(&file, 0, capacity)?;
            file.write_all_at(&header, 0)?;
            for at in HEADS {
                file.write_all_at(&encoded, at)?;
            }
            file.sync_all()?;
            point("resident.checkpoint.after_wal_sync")?;
            check()?;
            point("resident.checkpoint.before_select")?;
            check()?;
            if let Err(error) = ulmdb_platform::rename_replace(&temporary, path) {
                if ulmdb_platform::rename_outcome_unknown(&error) {
                    self.poisoned = true;
                    return Err(Error::OutcomeUnknown);
                }
                return Err(error.into());
            }
            self.poisoned = true;
            let publication = (|| {
                point("resident.checkpoint.after_select")?;
                ulmdb_platform::sync_directory_file(&directory)?;
                point("resident.checkpoint.after_directory_sync")?;
                if let Some(g) = &capture {
                    g.check()?;
                }
                let current = ulmdb_platform::path_identity(parent)?;
                if current.volume != dm.volume || current.file_id != dm.file_id {
                    return Err(Error::Corrupt("resident checkpoint directory changed"));
                }
                let selected = ulmdb_platform::path_identity(path)?;
                let pinned = ulmdb_platform::file_identity(&file)?;
                if !selected.is_file()
                    || selected.volume != pinned.volume
                    || selected.file_id != pinned.file_id
                    || selected.links != 1
                {
                    return Err(Error::Corrupt("resident checkpoint selected path changed"));
                }
                Ok::<(), Error>(())
            })();
            if publication.is_err() {
                return Err(Error::OutcomeUnknown);
            }
            let capacity_probe = CapacityProbe::for_file(&file);
            Ok(Self {
                capture: capture.clone(),
                file,
                capacity_probe,
                boot: self.boot,
                capacity,
                state,
                slot: 0,
                cached: [encoded; 2],
                poisoned: false,
            })
        })();
        if build.is_err() && !self.poisoned {
            let _ = std::fs::remove_file(&temporary);
            let _ = std::fs::remove_file(seed.path(parent));
        }
        build
    }
    /// Reclaim only unselected checkpoint/staging files, after durable recovery
    /// or rotation. Readers own anonymous maps and never consult these paths.
    pub fn prune_checkpoints(&self, path: &Path) -> Result<u64> {
        let guard = self
            .capture
            .as_ref()
            .map(CaptureGate::exclusive)
            .transpose()?;
        let result = self.prune_unlocked(path);
        if let Some(g) = guard {
            g.check()?;
        }
        result
    }
    fn prune_unlocked(&self, path: &Path) -> Result<u64> {
        if self.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        let parent = path
            .parent()
            .ok_or(Error::InvalidInput("resident WAL parent"))?;
        let mut header = [0; BLOCK];
        self.file.read_exact_at(&mut header, 0)?;
        let keep = wire::seed(&header).map(|s| s.path(parent));
        let mut removed = 0;
        for entry in std::fs::read_dir(parent)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let cp = name
                .strip_prefix(".resident-checkpoint-")
                .is_some_and(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()));
            let tmp = name
                .strip_prefix(".resident-wal-")
                .and_then(|n| n.strip_suffix(".tmp"))
                .is_some_and(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()));
            if (cp || tmp) && Some(entry.path()) != keep {
                let m = entry.file_type()?;
                if !m.is_file() {
                    return Err(Error::Corrupt("resident checkpoint garbage file kind"));
                }
                std::fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
        if removed > 0 {
            ulmdb_platform::sync_path(parent)?;
        }
        Ok(removed)
    }
    pub fn boot(&self) -> Boot {
        self.boot
    }
    pub fn transform_context(&self) -> Option<&std::sync::Arc<crate::transform::TransformContext>> {
        self.file.context()
    }
    pub fn can_append(&self, length: u64) -> Result<bool> {
        self.file
            .can_write_extent(self.state.selected().end()?, length)
    }
    pub fn physical_bytes(&self) -> Result<u64> {
        self.file.physical_end(self.state.selected().end()?)
    }
    pub fn capacity(&self) -> u64 {
        self.capacity
    }
    pub fn state(&self) -> &ReplayState {
        &self.state
    }
    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }
    pub fn commit_checked(
        &mut self,
        record: Record,
        ready: impl FnOnce(&Record, WalReceipt) -> Result<()>,
    ) -> Result<WalReceipt> {
        let guard = self
            .capture
            .as_ref()
            .map(CaptureGate::exclusive)
            .transpose()?;
        let result = self.commit_checked_inner(record, ready);
        crate::resident::capture::finish(result, guard.as_ref(), &mut self.poisoned)
    }
    fn commit_checked_inner(
        &mut self,
        record: Record,
        ready: impl FnOnce(&Record, WalReceipt) -> Result<()>,
    ) -> Result<WalReceipt> {
        let Self {
            file,
            capacity_probe,
            boot,
            capacity,
            state,
            slot,
            cached,
            poisoned,
            ..
        } = self;
        if *poisoned {
            return Err(Error::OutcomeUnknown);
        }
        let start = state.selected().end()?;
        let remaining = capacity
            .checked_sub(start)
            .ok_or(Error::Corrupt("resident selected end beyond capacity"))?;
        if record.encoded_length(*boot)? as u64 > remaining
            || !file.can_write_extent(start, record.encoded_length(*boot)? as u64)?
        {
            return Err(Error::LimitExceeded);
        }
        let (encoded, decoded) = wal_codec::encode_owned(*boot, record)?;
        let receipt = decoded.receipt();
        let transition = state.prepare(&decoded)?;
        ready(decoded.record(), receipt)?;
        for (expected, at) in cached.iter().zip(HEADS) {
            let mut actual = [0; BLOCK];
            file.read_exact_at(&mut actual, at)?;
            if actual != *expected {
                *poisoned = true;
                return Err(Error::Corrupt("resident active witness changed"));
            }
        }
        if capacity_probe.length(file)? != *capacity {
            *poisoned = true;
            return Err(Error::Corrupt("resident active capacity changed"));
        }
        let inactive = 1 - *slot;
        let witness = wire::head(
            *boot,
            *capacity,
            Witness {
                receipt,
                commit: decoded.record().commit,
            },
        );
        let end = receipt.end()?;
        let appended = (|| {
            point("resident.commit.before_append")?;
            write_cut(file, &encoded.bytes, start, "resident.commit.half_record")?;
            point("resident.commit.after_append")?;
            point("resident.commit.before_data_sync")?;
            file.sync_data()?;
            point("resident.commit.after_data_sync")?;
            Ok::<(), Error>(())
        })();
        if let Err(error) = appended {
            let reset = (|| {
                point("resident.commit.before_reset")?;
                tail::clear(file, start, end)?;
                point("resident.commit.after_reset")?;
                file.sync_data()?;
                point("resident.commit.after_reset_sync")?;
                Ok::<(), Error>(())
            })();
            if reset.is_err() {
                *poisoned = true;
                return Err(Error::OutcomeUnknown);
            }
            return Err(error);
        }
        let selected = (|| {
            point("resident.commit.before_witness")?;
            write_cut(
                file,
                &witness,
                HEADS[inactive],
                "resident.commit.half_witness",
            )?;
            point("resident.commit.after_witness")?;
            point("resident.commit.before_witness_sync")?;
            file.sync_data()?;
            point("resident.commit.after_witness_sync")?;
            point("resident.commit.before_ack")?;
            Ok::<(), Error>(())
        })();
        if selected.is_err() {
            *poisoned = true;
            return Err(Error::OutcomeUnknown);
        }
        cached[inactive] = witness;
        *slot = inactive;
        transition.activate();
        Ok(receipt)
    }
}
fn lock(file: File) -> Result<WriterFence> {
    WriterFence::try_lock(file).map_err(|e| match e {
        std::fs::TryLockError::WouldBlock => Error::Busy,
        std::fs::TryLockError::Error(e) => Error::Io(e),
    })
}
fn sync_parent(path: &Path) -> Result<()> {
    let real = std::fs::canonicalize(path)?;
    ulmdb_platform::sync_path(
        real.parent()
            .ok_or(Error::InvalidInput("resident WAL parent"))?,
    )?;
    Ok(())
}
fn write_cut(
    file: &crate::transform::ResidentFile,
    bytes: &[u8],
    at: u64,
    name: &str,
) -> Result<()> {
    file.write_cut(bytes, at, || point(name))
}
