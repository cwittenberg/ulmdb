//! Compound recovery-base selection; WAL bytes remain physically retained.
mod activate;
pub(in crate::hybrid) mod marker;
pub(in crate::hybrid) mod publication;
pub use activate::RetirementInstallReport;
pub(in crate::hybrid) mod wire;
use super::Environment;
use crate::{Error, Result};
use std::sync::Arc;
use ulmdb_storage::hybrid::{
    checkpoint::{CheckpointArchive, CheckpointLimits, SnapshotSelection},
    retirement::{CheckpointAliases, ExclusiveLease},
};
pub(super) struct SeedContext {
    pub compound: ulmdb_storage::hybrid::retirement::selection::Compound,
    pub aliases: Arc<CheckpointAliases>,
}
#[derive(Clone, Copy, Debug)]
pub struct RetirementReport {
    pub checkpoint_sequence: u64,
    pub validated_through: u64,
    /// Zero denotes the canonical genesis boundary, so no record prefix is eligible.
    pub first_required_lsn: u64,
    pub first_required_offset: u64,
    pub proposed_prefix_end: u64,
    pub declared_failure_granule: u64,
    pub alias_digest: [u8; 32],
}
/// Private construction and no Clone. The exclusive inode gate closes before
/// the retained Env writer fence. This does not authorize any destructive IO.
pub struct RetirementPreparation {
    lease: Option<ExclusiveLease>,
    fence: Option<Arc<ulmdb_storage::WriterFence>>,
    issuer: Arc<()>,
    expected: ulmdb_storage::hybrid::retirement::selection::RecoveryBase,
    report: RetirementReport,
    proposal: wire::Draft,
    seed: Arc<SeedContext>,
}
impl RetirementPreparation {
    pub fn report(&self) -> RetirementReport {
        self.report
    }
    pub fn proposal_digest(&self) -> [u8; 32] {
        self.proposal.digest()
    }
    fn release(&mut self) {
        drop(self.lease.take());
        drop(self.fence.take());
    }
}
impl Environment {
    /// Prepare a uniquely owned selected-floor capability. Canonical genesis
    /// itself has no predecessor and is explicitly unsupported without mutation.
    pub fn prepare_retirement(&self, limits: CheckpointLimits) -> Result<RetirementPreparation> {
        use super::checkpoint::{
            audit,
            history::{Anchor, History},
        };
        use ulmdb_storage::hybrid::retirement::selection::header_digest;
        let _maintenance = self.checkpoint_ops.lock().map_err(|_| Error::Busy)?;
        let expected = audit::protocol(self)?.ok_or(Error::InvalidInput(
            "retirement requires a selected checkpoint",
        ))?;
        let selected = expected.selector();
        if selected.cut.receipt.lsn == 0 {
            return Err(Error::InvalidInput(
                "genesis floor selection is unsupported",
            ));
        }
        let path = self.directory.join("HYBRID.wal");
        let (anchor, captured, prior) = {
            let mut w = self.writer.lock().map_err(|_| Error::Busy)?;
            w.ledger.healthy()?;
            if w.checkpoint_poisoned || w.journal.is_poisoned() {
                return Err(Error::OutcomeUnknown);
            }
            let current = self.current.load_full();
            let captured = SnapshotSelection {
                receipt: current.receipt,
                commit: current.commit,
                root: current.root,
            };
            if captured.receipt != w.journal.selected()
                || captured.commit != w.journal.commit_id()
                || captured.root != w.journal.root()
            {
                w.checkpoint_poisoned = true;
                return Err(Error::OutcomeUnknown);
            }
            let seed = w.seed.clone();
            let result = if let Some(seed) = &seed {
                w.journal
                    .capture_seeded(&path, seed.compound, Arc::clone(&seed.aliases))
                    .map(Anchor::Seeded)
            } else {
                w.journal.capture_committed(&path).map(Anchor::Full)
            };
            let anchor = match result {
                Ok(a) => a,
                Err(e) => {
                    if audit::integrity_failure(&e) {
                        w.checkpoint_poisoned = true;
                    }
                    return Err(e);
                }
            };
            (anchor, captured, seed.map(|s| s.compound))
        };
        let history: History = audit::selected_result(self, selected, anchor.verify())?;
        let checked = (|| {
            let directory = selected.directory(&self.directory);
            audit::budget(&directory, selected, limits)?;
            let archive = CheckpointArchive::open(&directory, limits)?;
            history.verify_archive(&archive, selected)?;
            let aliases = Arc::new(CheckpointAliases::from_archive(&archive)?);
            let boundary = history.boundary(selected.cut)?;
            let report = RetirementReport {
                checkpoint_sequence: selected.cut.receipt.sequence,
                validated_through: captured.receipt.sequence,
                first_required_lsn: boundary.predecessor.receipt.lsn,
                first_required_offset: boundary.predecessor.receipt.offset,
                proposed_prefix_end: (boundary.predecessor.receipt.offset / 65536 * 65536)
                    .max(196608),
                declared_failure_granule: 65536,
                alias_digest: aliases.descriptor_digest(),
            };
            let proposal = wire::Draft::new(
                selected,
                boundary,
                report,
                header_digest(history.header(), history.layout())?,
                prior,
            )?;
            let seed = Arc::new(SeedContext {
                compound: proposal.context(),
                aliases,
            });
            Ok((report, proposal, seed))
        })();
        let (report, proposal, seed) = audit::selected_result(self, selected, checked)?;
        drop(anchor); // Our own shared gate must not conflict with the exclusive permit.
        if audit::protocol(self)? != Some(expected) {
            return Err(Error::InvalidInput("retirement authority changed"));
        }
        let mut w = self.writer.lock().map_err(|_| Error::Busy)?;
        if w.checkpoint_poisoned || w.journal.is_poisoned() {
            return Err(Error::OutcomeUnknown);
        }
        if w.recovery_base != Some(selected) || w.journal.selected().lsn < captured.receipt.lsn {
            return Err(Error::InvalidInput("retirement base/head changed"));
        }
        let lease = match w.journal.try_retirement_lease(&path) {
            Ok(l) => l,
            Err(e) => {
                if audit::integrity_failure(&e) {
                    w.checkpoint_poisoned = true;
                }
                return Err(e);
            }
        };
        Ok(RetirementPreparation {
            lease: Some(lease),
            fence: Some(Arc::clone(&self.fence)),
            issuer: Arc::clone(&self.owner),
            expected,
            report,
            proposal,
            seed,
        })
    }
}
