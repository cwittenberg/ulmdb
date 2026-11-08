//! One compound decision, then the permanent protocol gate. Never swaps live H.
use super::{Environment, RetirementPreparation, marker, publication::Publication};
use crate::{Error, Result};
use std::sync::Arc;
use ulmdb_storage::hybrid::retirement::selection::RecoveryBase;
#[derive(Clone, Copy, Debug)]
pub struct RetirementInstallReport {
    pub checkpoint_sequence: u64,
    pub current_sequence: u64,
    pub first_required_lsn: u64,
    pub proposed_prefix_end: u64,
    pub serial: u64,
    pub already_selected: bool,
}
impl Environment {
    pub fn activate_retirement(
        &self,
        prepared: &mut RetirementPreparation,
    ) -> Result<RetirementInstallReport> {
        if !Arc::ptr_eq(&self.owner, &prepared.issuer) {
            return Err(Error::InvalidInput("foreign/reopened retirement issuer"));
        }
        let _maintenance = self.checkpoint_ops.lock().map_err(|_| Error::Busy)?;
        let actual = super::super::checkpoint::audit::protocol(self)?;
        let compound = prepared.proposal.context();
        if actual == Some(RecoveryBase::Compound(compound)) {
            let sequence = self.current.load().receipt.sequence;
            prepared.release();
            return Ok(RetirementInstallReport {
                checkpoint_sequence: compound.selector().cut.receipt.sequence,
                current_sequence: sequence,
                first_required_lsn: compound.first_required().receipt.lsn,
                proposed_prefix_end: compound.prefix_end(),
                serial: compound.serial(),
                already_selected: true,
            });
        }
        if actual != Some(prepared.expected) || prepared.lease.is_none() {
            return Err(Error::InvalidInput("stale/spent retirement capability"));
        }
        // Both bounded publication objects and the ready cached seed already exist
        // before the first selector decision. Their errors here are unselected IO.
        let publication = Publication::floor(&self.directory, prepared.proposal.bytes())?;
        let required = if marker::read(&self.directory)?.is_none() {
            Some(Publication::required(&self.directory, compound)?)
        } else {
            None
        };
        let mut writer = self.writer.lock().map_err(|_| Error::Busy)?;
        writer.ledger.healthy()?;
        if writer.checkpoint_poisoned || writer.journal.is_poisoned() {
            return Err(Error::OutcomeUnknown);
        }
        let cached = writer
            .seed
            .as_ref()
            .map(|s| RecoveryBase::Compound(s.compound))
            .or_else(|| writer.recovery_base.map(RecoveryBase::Legacy));
        if cached != Some(prepared.expected) {
            return Err(Error::InvalidInput("retirement cached authority changed"));
        }
        let live = self.current.load_full();
        if live.receipt != writer.journal.selected()
            || live.commit != writer.journal.commit_id()
            || live.root != writer.journal.root()
            || live.receipt.lsn < compound.selector().cut.receipt.lsn
        {
            writer.checkpoint_poisoned = true;
            return Err(Error::OutcomeUnknown);
        }
        if let Err(e) = writer.journal.verify_selected_envelope() {
            if super::super::checkpoint::audit::integrity_failure(&e) {
                writer.checkpoint_poisoned = true;
            }
            return Err(e);
        }
        // Keep the old metadata root alive until after releasing the writer
        // mutex, so replacing a unique seed cannot deallocate a full inventory
        // under the foreground writer fence.
        let previous_seed = writer.seed.clone();
        writer.checkpoint_poisoned = true;
        let result = publication.publish().and_then(|()| {
            if let Some(required) = required {
                required.publish()
            } else {
                Ok(())
            }
        });
        prepared.release();
        if result.is_err() {
            return Err(Error::OutcomeUnknown);
        }
        writer.seed = Some(Arc::clone(&prepared.seed));
        writer.recovery_base = Some(compound.selector());
        writer.checkpoint_poisoned = false;
        let current_sequence = live.receipt.sequence;
        drop(writer);
        drop(previous_seed);
        Ok(RetirementInstallReport {
            checkpoint_sequence: compound.selector().cut.receipt.sequence,
            current_sequence,
            first_required_lsn: compound.first_required().receipt.lsn,
            proposed_prefix_end: compound.prefix_end(),
            serial: compound.serial(),
            already_selected: false,
        })
    }
}
