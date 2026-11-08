//! One owning authority context per captured immutable prefix. No read overlay.
use crate::{Error, Result};
use std::sync::Arc;
use ulmdb_storage::hybrid::{
    checkpoint::{CheckpointArchive, SnapshotSelection},
    journal::{Bootstrap, CommittedAnchor, SeededAnchor, Witness},
    recovery::CommittedPrefix,
    redo::RootState,
    retirement::{CheckpointAliases, SeededPrefix},
};
// One cold captured context, already held inside Arc by its caller. Boxing
// a variant would add a second allocation without reducing per-read state.
#[allow(clippy::large_enum_variant)]
pub(in crate::hybrid) enum Anchor {
    Full(CommittedAnchor),
    Seeded(SeededAnchor),
}
// One temporary checked prefix owns its bounded scalar header and record Vec;
// keep it direct rather than introducing a second cold allocation.
#[allow(clippy::large_enum_variant)]
pub(in crate::hybrid) enum History {
    Full(CommittedPrefix),
    Seeded {
        prefix: SeededPrefix,
        aliases: Arc<CheckpointAliases>,
    },
}
impl Anchor {
    pub fn verify(&self) -> Result<History> {
        match self {
            Self::Full(a) => Ok(History::Full(a.verify()?)),
            Self::Seeded(a) => Ok(History::Seeded {
                prefix: a.verify()?,
                aliases: Arc::clone(a.aliases()),
            }),
        }
    }
}
impl History {
    pub fn header(&self) -> Bootstrap {
        match self {
            Self::Full(p) => p.header(),
            Self::Seeded { prefix, .. } => prefix.header,
        }
    }
    pub fn layout(&self) -> ulmdb_storage::hybrid::journal::WalLayout {
        match self {
            Self::Full(p) => p.layout(),
            Self::Seeded { prefix, .. } => prefix.layout,
        }
    }
    pub fn boundary(
        &self,
        cut: SnapshotSelection,
    ) -> Result<ulmdb_storage::hybrid::retirement::Boundary> {
        self.bind_cut(cut)?;
        if let Self::Seeded { prefix, .. } = self
            && cut.receipt.lsn == prefix.boundary.base.receipt.lsn
        {
            return Ok(prefix.boundary);
        }
        let prior = cut
            .receipt
            .lsn
            .checked_sub(1)
            .ok_or(Error::InvalidInput("floor has no genesis predecessor"))?;
        let predecessor = self
            .at(prior)
            .ok_or(Error::Corrupt(
                "checkpoint predecessor outside retained context",
            ))?
            .0;
        Ok(ulmdb_storage::hybrid::retirement::Boundary {
            base: Witness {
                receipt: cut.receipt,
                commit: cut.commit,
            },
            root: cut.root,
            predecessor,
            first_suffix_lsn: cut.receipt.lsn.checked_add(1).ok_or(Error::LimitExceeded)?,
        })
    }
    pub fn at(&self, lsn: u64) -> Option<(Witness, RootState)> {
        match self {
            Self::Full(p) => p.at(lsn),
            Self::Seeded { prefix, .. } => at_seeded(prefix, lsn),
        }
    }
    pub fn bind_cut(&self, cut: SnapshotSelection) -> Result<()> {
        if self.at(cut.receipt.lsn)
            != Some((
                Witness {
                    receipt: cut.receipt,
                    commit: cut.commit,
                },
                cut.root,
            ))
        {
            return Err(Error::Corrupt(
                "checkpoint cut is not exact retained ancestor",
            ));
        }
        Ok(())
    }
    pub fn verify_archive(
        &self,
        archive: &CheckpointArchive,
        selected: super::selector::Selector,
    ) -> Result<super::super::provenance::Ledger> {
        match self {
            Self::Full(p) => {
                super::base::verify_archive(archive, selected, p.header(), p.records())
            }
            Self::Seeded { prefix, aliases } => {
                super::base::verify_archive_seeded(archive, selected, prefix, aliases)
            }
        }
    }
}
pub(in crate::hybrid) fn at_seeded(
    prefix: &SeededPrefix,
    lsn: u64,
) -> Option<(Witness, RootState)> {
    if lsn == prefix.boundary.base.receipt.lsn {
        return Some((prefix.boundary.base, prefix.boundary.root));
    }
    let index = usize::try_from(lsn.checked_sub(prefix.first_lsn())?).ok()?;
    let d = prefix.records().get(index)?;
    if d.record.lsn != lsn {
        return None;
    }
    Some((
        Witness {
            receipt: d.receipt,
            commit: d.record.commit,
        },
        d.record.root,
    ))
}
