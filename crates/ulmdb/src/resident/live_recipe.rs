//! Selected Snapshot/state authority remains private to this facade. Borrowing
//! payloads changes allocation ownership, never receipt or source issuance.
use super::{RedoFormat, Snapshot};
use crate::{Error, Result};
use ulmdb_storage::{
    Reference,
    resident::{
        Prepared,
        wal_codec::{PAGE, SlotState},
        wal_codec_v2::{BorrowedRecord, RecipeBuilder, RecipeWorkspace, Record, ReplayState},
    },
};
pub(super) fn build<'a, 'w>(
    base: &Snapshot,
    state: &ReplayState,
    candidate: &'a Prepared,
    header: Record,
    workspace: &'w mut RecipeWorkspace,
    format: RedoFormat,
) -> Result<BorrowedRecord<'a, 'w>> {
    if state.selected() != base.receipt
        || state.commit() != base.commit
        || state.root() != base.root
        || state.next_database() != base.next_database
    {
        return Err(Error::Corrupt(
            "RL2 selected materialization issuer mismatch",
        ));
    }
    let boot = state.boot();
    let mut builder = RecipeBuilder::new(header, candidate, workspace)?;
    for (index, frozen) in candidate.slots().enumerate() {
        let previous = match state.slot(frozen.key.slot) {
            Some(SlotState::Free) => 0,
            Some(SlotState::Retired(_)) if frozen.key.slot >= boot.kv_pages => 0,
            Some(SlotState::Retired(g)) => g,
            _ => return Err(Error::Corrupt("RL2 destination logically live")),
        };
        if format == RedoFormat::CompactV2
            && let Some(source) = frozen.copy_source
        {
            if source.slot >= boot.kv_pages
                || frozen.key.slot >= boot.kv_pages
                || source.slot == frozen.key.slot
                || state.slot(source.slot) != Some(SlotState::Live(source.generation))
            {
                return Err(Error::Corrupt("RL2 hint not exact committed source"));
            }
            let at = Reference {
                segment: 1,
                offset: (source.generation << 28) | ((u64::from(source.slot) + 1) * PAGE as u64),
            };
            let (bytes, key) = base.image.read_with_key(at, PAGE)?;
            if key != source || frozen.bytes.len() != PAGE || frozen.ranges != [(0, PAGE as u16)] {
                return Err(Error::Corrupt("RL2 source exact allocation binding"));
            }
            builder.committed(index, previous, source, bytes)?;
        } else {
            builder.literal(index, previous)?;
        }
    }
    Ok(builder.finish())
}
