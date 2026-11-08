//! Private live source issuer. Hints become copy authority only against this
//! serialized selected Snapshot+Journal pair; public raw keys grant none.
use super::{RedoFormat, Snapshot};
use crate::{Error, Result};
use ulmdb_storage::{
    Reference,
    hybrid::redo::final_patches,
    resident::{
        Prepared,
        wal_codec::{PAGE, SlotState},
        wal_codec_v2::{Allocation, Body, ReplayState},
    },
};
pub(super) fn allocations(
    base: &Snapshot,
    state: &ReplayState,
    candidate: &Prepared,
    format: RedoFormat,
) -> Result<Vec<Allocation>> {
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
    let mut out = Vec::new();
    let slots = candidate.slots();
    out.try_reserve(slots.size_hint().0)
        .map_err(|_| Error::LimitExceeded)?;
    for frozen in slots {
        let previous_generation = match state.slot(frozen.key.slot) {
            Some(SlotState::Free) => 0,
            Some(SlotState::Retired(_)) if frozen.key.slot >= boot.kv_pages => 0,
            Some(SlotState::Retired(g)) => g,
            _ => return Err(Error::Corrupt("RL2 destination logically live")),
        };
        let mut copy = None;
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
                offset: (source.generation << 28) | ((source.slot as u64 + 1) * PAGE as u64),
            };
            let (bytes, key) = base.image.read_with_key(at, PAGE)?;
            if key != source || frozen.bytes.len() != PAGE || frozen.ranges != [(0, PAGE as u16)] {
                return Err(Error::Corrupt("RL2 source exact allocation binding"));
            }
            // Private construction compares the actual immutable old/new bytes once.
            // The frozen candidate/base and owned minimal patches cannot mutate before IO.
            let patches = final_patches(bytes, frozen.bytes)?;
            let encoded = patches.iter().try_fold(16usize, |n, p| {
                n.checked_add(8 + p.bytes.len().next_multiple_of(8))
                    .ok_or(Error::LimitExceeded)
            })?;
            if encoded < PAGE {
                copy = Some(Body::Copy { source, patches });
            }
        }
        out.push(Allocation {
            destination: frozen.key,
            previous_generation,
            ranges: frozen.ranges.to_vec(),
            body: copy.unwrap_or_else(|| Body::Literal(frozen.bytes.to_vec())),
        });
    }
    Ok(out)
}
