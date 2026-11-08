//! Private capability issuance; auxiliary publication never swaps live head H.
use super::{CheckpointCapture, selector};
use crate::hybrid::Environment;
use crate::{Error, Result};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    sync::Arc,
};
use ulmdb_storage::hybrid::checkpoint::{
    CheckpointArchive, CheckpointArtifact, CheckpointLimits, SnapshotSelection,
};
const MAX_GENERATIONS: usize = 8;
pub struct PreparedCheckpoint {
    capture: CheckpointCapture,
    artifact: CheckpointArtifact,
    selector: selector::Selector,
    may_be_selected: bool,
}
impl PreparedCheckpoint {
    pub fn directory(&self) -> &Path {
        self.artifact.directory()
    }
    pub fn selection(&self) -> SnapshotSelection {
        self.selector.cut
    }
}
impl Drop for PreparedCheckpoint {
    fn drop(&mut self) {
        if !self.may_be_selected {
            let _ = fs::remove_dir_all(self.artifact.directory());
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct CheckpointInstallReport {
    pub checkpoint_sequence: u64,
    pub current_sequence: u64,
    pub generation: u128,
    pub already_selected: bool,
}
struct Unselected(PathBuf, bool);
impl Drop for Unselected {
    fn drop(&mut self) {
        if !self.1 {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
impl Environment {
    /// Stage an Env-issued cut in the engine-owned namespace. Full WAL and old
    /// selected generations remain retained; this pilot caps generations at 8.
    pub fn prepare_checkpoint(&self, limits: CheckpointLimits) -> Result<PreparedCheckpoint> {
        let _maintenance = self.checkpoint_ops.lock().map_err(|_| Error::Busy)?;
        let parent = self.directory.join("HYBRID.checkpoints");
        match ulmdb_platform::create_private_directory(&parent) {
            Ok(()) => {
                ulmdb_platform::sync_path(&parent)?;
                ulmdb_platform::sync_path(&self.directory)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        if !fs::symlink_metadata(&parent)?.file_type().is_dir() {
            return Err(Error::Corrupt(
                "checkpoint namespace is not owned directory",
            ));
        }
        let mut count = 0;
        for entry in fs::read_dir(&parent)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or(Error::Corrupt("checkpoint generation name"))?;
            if !entry.file_type()?.is_dir()
                || name.len() != 32
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err(Error::Corrupt("checkpoint namespace entry"));
            }
            count += 1;
            if count >= MAX_GENERATIONS {
                return Err(Error::LimitExceeded);
            }
        }
        let capture = self.capture_checkpoint()?;
        let prefix = super::verified_prefix(self, &capture)?;
        prefix.bind_cut(capture.selection())?;
        let generation = ulmdb_storage::new_identity()?;
        let target = parent.join(format!("{generation:032x}"));
        let artifact = capture.stage(&target, generation, limits)?;
        let mut cleanup = Unselected(target, false);
        let d = capture.cut.verify_staged(artifact.directory(), limits)?;
        let selected = selector::from_descriptor(&d);
        let archive = CheckpointArchive::open(artifact.directory(), limits)?;
        prefix.verify_archive(&archive, selected)?;
        cleanup.1 = true;
        Ok(PreparedCheckpoint {
            capture,
            artifact,
            selector: selected,
            may_be_selected: false,
        })
    }
    pub fn activate_checkpoint(
        &self,
        prepared: &mut PreparedCheckpoint,
        limits: CheckpointLimits,
    ) -> Result<CheckpointInstallReport> {
        if !Arc::ptr_eq(&self.owner, &prepared.capture.issuer) {
            return Err(Error::InvalidInput("foreign/reopened checkpoint issuer"));
        }
        let _maintenance = self.checkpoint_ops.lock().map_err(|_| Error::Busy)?;
        let prefix = super::verified_prefix(self, &prepared.capture)?;
        let protocol = super::audit::protocol(self)?;
        let assets = (|| {
            super::audit::budget(prepared.directory(), prepared.selector, limits)?;
            prepared
                .capture
                .cut
                .verify_staged(prepared.directory(), limits)?;
            let archive = CheckpointArchive::open(prepared.directory(), limits)?;
            prefix.verify_archive(&archive, prepared.selector)?;
            if let Some(ulmdb_storage::hybrid::retirement::selection::RecoveryBase::Compound(
                prior,
            )) = protocol
            {
                let boundary = prefix.boundary(prepared.selector.cut)?;
                let draft = crate::hybrid::retirement::wire::Draft::carry(
                    prepared.selector,
                    boundary,
                    prior,
                )?;
                let aliases = Arc::new(
                    ulmdb_storage::hybrid::retirement::CheckpointAliases::from_archive(&archive)?,
                );
                let seed = Arc::new(crate::hybrid::retirement::SeedContext {
                    compound: draft.context(),
                    aliases,
                });
                Ok(Some((draft, seed)))
            } else {
                Ok(None)
            }
        })();
        let compound = super::audit::selected_result(self, prepared.selector, assets)?;
        // Every file/graph/origin check and temporary-file fence is off the
        // writer lock. Only the final live-head check and pointer publication lock it.
        enum CheckpointPublication {
            Legacy(selector::Publication),
            Compound(crate::hybrid::retirement::publication::Publication),
        }
        let (publication, next_seed) = if let Some((draft, seed)) = compound {
            (
                CheckpointPublication::Compound(
                    crate::hybrid::retirement::publication::Publication::floor(
                        &self.directory,
                        draft.bytes(),
                    )?,
                ),
                Some(seed),
            )
        } else {
            (
                CheckpointPublication::Legacy(selector::Publication::prepare(
                    &self.directory,
                    prepared.selector,
                )?),
                None,
            )
        };
        let mut writer = self.writer.lock().map_err(|_| Error::Busy)?;
        writer.ledger.healthy()?;
        if writer.checkpoint_poisoned || writer.journal.is_poisoned() {
            return Err(Error::OutcomeUnknown);
        }
        let live = self.current.load_full();
        if live.receipt != writer.journal.selected()
            || live.commit != writer.journal.commit_id()
            || live.root != writer.journal.root()
        {
            writer.checkpoint_poisoned = true;
            return Err(Error::OutcomeUnknown);
        }
        // Current head may have advanced since C. A same-issuer Journal only
        // appends exact parent-linked commits; its immutable C prefix was audited.
        writer.journal.verify_selected_envelope()?;
        let header = writer.journal.bootstrap();
        if header.uuid != prepared.selector.uuid
            || header.epoch != prepared.selector.epoch
            || live.receipt.lsn < prepared.selector.cut.receipt.lsn
        {
            return Err(Error::InvalidInput("checkpoint is not live-head ancestor"));
        }
        let report = CheckpointInstallReport {
            checkpoint_sequence: prepared.selector.cut.receipt.sequence,
            current_sequence: live.receipt.sequence,
            generation: prepared.selector.generation,
            already_selected: false,
        };
        if let Some(old) = writer.recovery_base {
            if old == prepared.selector {
                prepared.may_be_selected = true;
                return Ok(CheckpointInstallReport {
                    already_selected: true,
                    ..report
                });
            }
            if prepared.selector.cut.receipt.sequence <= old.cut.receipt.sequence {
                return Err(Error::InvalidInput(
                    "checkpoint coverage would regress/conflict",
                ));
            }
        }
        // No allocation, graph change or serving-root swap follows the durable
        // selector decision. On uncertain publication, preserve all artifacts.
        let previous_seed = writer.seed.clone();
        prepared.may_be_selected = true;
        writer.checkpoint_poisoned = true;
        match publication {
            CheckpointPublication::Legacy(p) => p.publish(),
            CheckpointPublication::Compound(p) => p.publish(),
        }
        .map_err(|_| Error::OutcomeUnknown)?;
        writer.recovery_base = Some(prepared.selector);
        writer.seed = next_seed;
        writer.checkpoint_poisoned = false;
        drop(writer);
        drop(previous_seed);
        Ok(report)
    }
}
