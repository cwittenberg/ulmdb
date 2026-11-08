//! Writer-only recipe strategy. Serving readers never dispatch on this mode.
use super::{
    LiveRedoMode, RedoMode,
    provenance::{Ledger, allocation},
};
use crate::{Error, Result};
use ulmdb_storage::{
    ReadStore,
    hybrid::{
        Allocation, FrozenExtent, ImageTable,
        borrowed::{BorrowedRecord, RecipeBuilder, RecipeWorkspace},
        redo::{Extent, ObjectRedo, Record},
    },
};
pub(super) enum Prepared<'a, 'w> {
    Owned(Record),
    Borrowed(BorrowedRecord<'a, 'w>),
}
impl Prepared<'_, '_> {
    pub fn object_count(&self) -> usize {
        match self {
            Self::Owned(r) => r.objects.len(),
            Self::Borrowed(r) => r.object_count(),
        }
    }
    pub fn mode(&self) -> LiveRedoMode {
        match self {
            Self::Owned(_) => LiveRedoMode::Owned,
            Self::Borrowed(_) => LiveRedoMode::Borrowed,
        }
    }
    pub fn each_destination(&self, mut f: impl FnMut(Extent) -> Result<()>) -> Result<()> {
        match self {
            Self::Owned(r) => {
                for o in &r.objects {
                    f(o.dest())?;
                }
            }
            Self::Borrowed(r) => {
                for d in r.destinations() {
                    f(d)?;
                }
            }
        }
        Ok(())
    }
}
fn objects(
    frozen: &FrozenExtent,
    generation: u128,
) -> impl Iterator<Item = Result<(Allocation, Extent, &[u8])>> {
    frozen.allocations().map(move |a| {
        let d = Extent {
            at: a.reference,
            logical_generation: generation,
            length: u32::try_from(a.length).map_err(|_| Error::LimitExceeded)?,
        };
        let offset = usize::try_from(
            d.at.offset
                .checked_sub(frozen.start() as u64)
                .ok_or(Error::Corrupt("hybrid destination before frozen tail"))?,
        )
        .map_err(|_| Error::LimitExceeded)?;
        let end = offset
            .checked_add(d.length as usize)
            .ok_or(Error::LimitExceeded)?;
        let after = frozen
            .bytes()?
            .get(offset..end)
            .ok_or(Error::Corrupt("hybrid destination outside frozen tail"))?;
        Ok((a, d, after))
    })
}
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare<'a, 'w>(
    mut header: Record,
    frozen: &'a FrozenExtent,
    base: &ImageTable,
    ledger: &Ledger,
    generation: u128,
    redo: RedoMode,
    mode: LiveRedoMode,
    workspace: &'w mut RecipeWorkspace,
    bound: usize,
) -> Result<Prepared<'a, 'w>> {
    match mode {
        LiveRedoMode::Owned => {
            header
                .objects
                .try_reserve_exact(frozen.allocation_count())
                .map_err(|_| Error::LimitExceeded)?;
            for object in objects(frozen, generation) {
                let (a, dest, after) = object?;
                let source = a
                    .base_source
                    .and_then(|at| allocation(ledger, at))
                    .filter(|s| s.extent.length == dest.length);
                let object = if redo == RedoMode::Compact {
                    if let Some(source) = source {
                        ObjectRedo::from_committed_bytes(
                            dest,
                            source.extent,
                            source.receipt,
                            base.read(source.extent.at, source.extent.length as usize)?,
                            after,
                        )?
                    } else {
                        ObjectRedo::Literal {
                            dest,
                            bytes: after.to_vec(),
                        }
                    }
                } else {
                    ObjectRedo::Literal {
                        dest,
                        bytes: after.to_vec(),
                    }
                };
                header.objects.push(object);
            }
            Ok(Prepared::Owned(header))
        }
        LiveRedoMode::Borrowed => {
            let mut b = RecipeBuilder::new(header, workspace, frozen.allocation_count(), bound)?;
            for object in objects(frozen, generation) {
                let (a, dest, after) = object?;
                let source = a
                    .base_source
                    .and_then(|at| allocation(ledger, at))
                    .filter(|s| s.extent.length == dest.length);
                if redo == RedoMode::Compact {
                    if let Some(source) = source {
                        b.committed(
                            dest,
                            source.extent,
                            source.receipt,
                            base.read(source.extent.at, source.extent.length as usize)?,
                            after,
                        )?
                    } else {
                        b.literal(dest, after)?;
                    }
                } else {
                    b.literal(dest, after)?;
                }
            }
            Ok(Prepared::Borrowed(b.finish()))
        }
    }
}
