//! Cold checkpoint correspondence, including immutable prior aliases.
mod seeded;
use super::selector;
use crate::hybrid::provenance::{CheckedImage, Ledger};
use crate::{Error, Result};
pub(in crate::hybrid) use seeded::{load_selected_seed, verify_archive_seeded};
use ulmdb_storage::hybrid::{
    checkpoint::{CheckpointArchive, SnapshotSelection},
    codec::Decoded,
    journal::{Bootstrap, Witness},
    provenance::ProvenanceDirectory,
    redo::{Authority, SourceReceipt},
};
pub(super) fn bind_cut(
    header: Bootstrap,
    records: &[Decoded],
    cut: SnapshotSelection,
) -> Result<()> {
    let (w, root) = if cut.receipt.lsn == 0 {
        (
            ulmdb_storage::hybrid::journal::genesis(header)?,
            ulmdb_storage::hybrid::redo::RootState::empty(),
        )
    } else {
        let index = usize::try_from(cut.receipt.lsn - 1).map_err(|_| Error::LimitExceeded)?;
        let r = records
            .get(index)
            .ok_or(Error::Corrupt("checkpoint cut exceeds retained WAL"))?;
        (
            Witness {
                receipt: r.receipt,
                commit: r.record.commit,
            },
            r.record.root,
        )
    };
    if w.receipt != cut.receipt || w.commit != cut.commit || root != cut.root {
        return Err(Error::Corrupt("checkpoint cut is not exact WAL ancestor"));
    }
    Ok(())
}
pub(in crate::hybrid) fn check_descriptor(
    d: &ulmdb_storage::hybrid::checkpoint::CheckpointDescriptor,
    selected: selector::Selector,
    header: Bootstrap,
    records: &[Decoded],
) -> Result<u64> {
    if selector::from_descriptor(d) != selected
        || selected.uuid != header.uuid
        || selected.epoch != header.epoch
    {
        return Err(Error::Corrupt("checkpoint selected descriptor/realm"));
    }
    bind_cut(header, records, d.selected)?;
    if d.chunks.len() as u64
        != d.selected
            .root
            .next_chunk
            .checked_sub(1)
            .ok_or(Error::Corrupt("checkpoint chunk count"))?
    {
        return Err(Error::Corrupt("checkpoint complete chunk inventory"));
    }
    let count = usize::try_from(d.selected.receipt.lsn).map_err(|_| Error::LimitExceeded)?;
    let prefix = records
        .get(..count)
        .ok_or(Error::Corrupt("checkpoint WAL cut count"))?;
    let mut bytes = 0u64;
    for c in &d.chunks {
        let final_delta = prefix
            .iter()
            .flat_map(|r| &r.record.chunks)
            .rfind(|delta| delta.chunk == c.identity.chunk)
            .ok_or(Error::Corrupt("checkpoint chunk absent from WAL"))?;
        if final_delta.logical_generation != c.identity.logical_generation
            || final_delta.capacity != c.identity.capacity
            || final_delta.after != c.end
        {
            return Err(Error::Corrupt("checkpoint chunk does not match WAL cut"));
        }
        bytes = bytes
            .checked_add(c.identity.capacity)
            .ok_or(Error::LimitExceeded)?;
    }
    for r in &records[count..] {
        if r.record
            .chunks
            .iter()
            .any(|c| c.chunk < d.selected.root.next_chunk)
        {
            return Err(Error::Corrupt(
                "checkpoint continuation reuses sealed chunk",
            ));
        }
    }
    Ok(bytes)
}
pub(in crate::hybrid) fn verify_archive(
    archive: &CheckpointArchive,
    selected: selector::Selector,
    header: Bootstrap,
    records: &[Decoded],
) -> Result<Ledger> {
    let d = archive.descriptor();
    check_descriptor(d, selected, header, records)?;
    let count = usize::try_from(d.selected.receipt.lsn).map_err(|_| Error::LimitExceeded)?;
    let prefix = records
        .get(..count)
        .ok_or(Error::Corrupt("checkpoint WAL cut count"))?;
    let expected = prefix
        .iter()
        .try_fold(0u64, |n, r| n.checked_add(r.record.objects.len() as u64))
        .ok_or(Error::LimitExceeded)?;
    let mut seen = 0u64;
    let mut directory = ProvenanceDirectory::default();
    let mut bytes = 0u64;
    for (&id, c) in archive.chunks() {
        let table = c.table();
        let mut final_delta = None;
        for r in prefix {
            for delta in &r.record.chunks {
                if delta.chunk == id {
                    if delta.logical_generation != table.identity().logical_generation
                        || delta.capacity != table.identity().capacity
                    {
                        return Err(Error::Corrupt("checkpoint chunk/WAL generation/capacity"));
                    }
                    final_delta = Some(delta);
                }
            }
        }
        if final_delta.is_none_or(|delta| delta.after != table.end()) {
            return Err(Error::Corrupt("checkpoint frozen bound not WAL cut"));
        }
        for origin in table.origins() {
            let index = usize::try_from(origin.receipt.creation_lsn.checked_sub(1).ok_or(
                Error::Corrupt("checkpoint origin has no retained WAL creation"),
            )?)
            .map_err(|_| Error::LimitExceeded)?;
            let creation = prefix
                .get(index)
                .ok_or(Error::Corrupt("checkpoint future origin"))?;
            let r = &creation.record;
            let expected_receipt = SourceReceipt {
                uuid: r.uuid,
                epoch: r.epoch,
                creation_lsn: r.lsn,
                creation_sequence: r.sequence,
                creation_commit: r.commit,
                provenance_digest: creation.receipt.digest,
                authority: Authority::Wal {
                    lsn: r.lsn,
                    sequence: r.sequence,
                    digest: creation.receipt.digest,
                },
            };
            let at = r
                .objects
                .binary_search_by_key(&origin.extent.at, |o| o.dest().at)
                .map_err(|_| Error::Corrupt("checkpoint origin absent from WAL allocation"))?;
            if r.objects[at].dest() != origin.extent || origin.receipt != expected_receipt {
                return Err(Error::Corrupt(
                    "checkpoint exact creation identity/authority",
                ));
            }
            seen = seen.checked_add(1).ok_or(Error::LimitExceeded)?;
        }
        bytes = bytes
            .checked_add(table.identity().capacity)
            .ok_or(Error::LimitExceeded)?;
        directory = directory.insert(std::sync::Arc::clone(table))?;
    }
    if seen != expected {
        return Err(Error::Corrupt("checkpoint omitted/extra origin"));
    }
    let ledger = Ledger::from_directory(directory);
    let checked = CheckedImage {
        image: archive,
        ledger: &ledger,
    };
    crate::backup::validate_materialized_graph(
        &checked,
        d.selected.root.catalog,
        d.selected.root.next_database,
        bytes,
    )?;
    Ok(ledger)
}
