//! Cold selected-compound recovery; graph readiness precedes every repair.
use super::{CommittedAnchor, Journal, lock, point, sync_parent, tail, wire};
use crate::hybrid::{
    redo::RootState,
    retirement::{
        self, CheckpointAliases, SeededPrefix,
        selection::{Compound, header_digest},
    },
};
use crate::{Error, Result};
use std::{fs::OpenOptions, path::Path, sync::Arc};
use ulmdb_platform::FileExt;
/// An owning immutable-prefix anchor bound to one selected CP alias context.
/// Its independent OFD retains the same conservative shared inode gate.
pub struct SeededAnchor {
    anchor: CommittedAnchor,
    compound: Compound,
    aliases: Arc<CheckpointAliases>,
}
impl SeededAnchor {
    pub fn verify(&self) -> Result<SeededPrefix> {
        self.anchor.verify_compound(&self.aliases, self.compound)
    }
    pub fn aliases(&self) -> &Arc<CheckpointAliases> {
        &self.aliases
    }
    pub fn compound(&self) -> Compound {
        self.compound
    }
}
impl Journal {
    pub fn capture_seeded(
        &mut self,
        path: &Path,
        compound: Compound,
        aliases: Arc<CheckpointAliases>,
    ) -> Result<SeededAnchor> {
        let s = compound.selector();
        if s.uuid != self.header.uuid
            || s.epoch != self.header.epoch
            || s.cut.receipt.lsn > self.selected.receipt.lsn
            || s.cut != aliases.selection()
            || s.digest != aliases.descriptor_digest()
            || compound.header_digest() != header_digest(self.header, self.layout)?
        {
            return Err(Error::InvalidInput(
                "seeded anchor context does not match journal",
            ));
        }
        Ok(SeededAnchor {
            anchor: self.capture_committed(path)?,
            compound,
            aliases,
        })
    }
    /// Low-level checked planner, not an Environment authority issuer. The
    /// caller must supply its trusted selected compound/alias context, then
    /// reconstruct sources/typed graph/admission in validate before readiness.
    pub fn open_seeded_checked<T>(
        path: &Path,
        compound: Compound,
        aliases: &CheckpointAliases,
        validate: impl FnOnce(&SeededPrefix) -> Result<T>,
    ) -> Result<(Self, T)> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let file = lock(file)?;
        let plan = retirement::inspect_compound(&file, aliases, compound)?;
        let state = validate(&plan)?;
        if plan.heal {
            point("hybrid.recovery.before_heal")?;
            file.write_all_at(
                &wire::encode_layout_witness(plan.header, plan.selected, plan.layout),
                wire::WITNESS_OFFSETS[plan.slot],
            )?;
            point("hybrid.recovery.after_heal")?;
            ulmdb_platform::sync_file(&file)?;
            point("hybrid.recovery.after_heal_sync")?;
        }
        let end = plan.selected.receipt.end()?;
        match plan.layout {
            wire::WalLayout::Dense if file.metadata()?.len() != end => {
                point("hybrid.recovery.before_trim")?;
                file.set_len(end)?;
                point("hybrid.recovery.after_trim")?;
                ulmdb_platform::sync_file(&file)?;
                point("hybrid.recovery.after_trim_sync")?;
            }
            wire::WalLayout::PreSized { capacity } => {
                point("hybrid.recovery.before_clear")?;
                tail::clear(&file, end, capacity)?;
                point("hybrid.recovery.after_clear")?;
                ulmdb_platform::sync_data(&file)?;
                point("hybrid.recovery.after_clear_sync")?;
            }
            _ => {}
        }
        point("hybrid.recovery.before_ready_file_sync")?;
        ulmdb_platform::sync_file(&file)?;
        point("hybrid.recovery.after_ready_file_sync")?;
        sync_parent(path)?;
        point("hybrid.recovery.after_ready_parent_sync")?;
        let mut cached = [[0; wire::BLOCK]; 2];
        for (bytes, offset) in cached.iter_mut().zip(wire::WITNESS_OFFSETS) {
            file.read_exact_at(bytes, offset)?;
        }
        let root: RootState = plan.root();
        Ok((
            Self {
                file,
                header: plan.header,
                layout: plan.layout,
                selected: plan.selected,
                root,
                slot: plan.slot,
                poisoned: false,
                cached,
                encode_workspace: Vec::new(),
            },
            state,
        ))
    }
    /// Non-escaping under-writer check. Unlike capture, this creates no shared
    /// lease that would conflict with the caller's own exclusive preparation.
    pub fn verify_selected_envelope(&mut self) -> Result<()> {
        if self.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        let mut header = [0; wire::BLOCK];
        self.file.read_exact_at(&mut header, 0)?;
        match wire::decode_layout_header(&header) {
            Ok(actual) if actual == (self.header, self.layout) => {}
            _ => {
                self.poisoned = true;
                return Err(Error::Corrupt("hybrid immutable header drift"));
            }
        }
        for (expected, offset) in self.cached.iter().zip(wire::WITNESS_OFFSETS) {
            let mut actual = [0; wire::BLOCK];
            self.file.read_exact_at(&mut actual, offset)?;
            if &actual != expected {
                self.poisoned = true;
                return Err(Error::Corrupt("hybrid live envelope drift"));
            }
        }
        if self.file.metadata()?.len() != self.layout.physical_end(self.selected.receipt.end()?) {
            self.poisoned = true;
            return Err(Error::Corrupt("hybrid live envelope extent"));
        }
        Ok(())
    }
}
