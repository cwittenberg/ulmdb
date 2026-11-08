//! Complete checkpoint correspondence from a prior immutable alias inventory
//! plus every authenticated suffix allocation. No historical receipt is reissued.
use crate::hybrid::{
    checkpoint::{history::at_seeded, selector},
    provenance::{CheckedImage, Ledger},
};
use crate::{Error, Result};
use std::{borrow::Borrow, sync::Arc};
use ulmdb_storage::hybrid::{
    checkpoint::CheckpointArchive,
    provenance::{Origin, ProvenanceDirectory},
    redo::{Authority, SourceReceipt},
    retirement::{CheckpointAliases, SeededPrefix},
};
pub(in crate::hybrid) fn verify_archive_seeded(
    archive: &CheckpointArchive,
    selected: selector::Selector,
    prefix: &SeededPrefix,
    aliases: &CheckpointAliases,
) -> Result<Ledger> {
    let d = archive.descriptor();
    let actual = at_seeded(prefix, selected.cut.receipt.lsn)
        .ok_or(Error::Corrupt("checkpoint cut below seeded authority"))?;
    if selector::from_descriptor(d) != selected
        || selected.uuid != prefix.header.uuid
        || selected.epoch != prefix.header.epoch
        || actual.0.receipt != selected.cut.receipt
        || actual.0.commit != selected.cut.commit
        || actual.1 != selected.cut.root
        || d.chunks.len() as u64
            != selected
                .cut
                .root
                .next_chunk
                .checked_sub(1)
                .ok_or(Error::Corrupt("checkpoint seeded chunk count"))?
    {
        return Err(Error::Corrupt("checkpoint seeded selected identity/cut"));
    }
    let records = prefix.records();
    let through = records.partition_point(|r| r.record.lsn <= selected.cut.receipt.lsn);
    let covered = &records[..through];
    for r in &records[through..] {
        if r.record
            .chunks
            .iter()
            .any(|c| c.chunk < selected.cut.root.next_chunk)
        {
            return Err(Error::Corrupt(
                "checkpoint seeded continuation reuses sealed chunk",
            ));
        }
    }
    let old_count = aliases
        .chunks()
        .try_fold(0u64, |n, c| n.checked_add(c.len() as u64))
        .ok_or(Error::LimitExceeded)?;
    let expected = covered
        .iter()
        .try_fold(old_count, |n, r| {
            n.checked_add(r.record.objects.len() as u64)
        })
        .ok_or(Error::LimitExceeded)?;
    let mut seen = 0u64;
    let mut bytes = 0u64;
    let mut directory = ProvenanceDirectory::default();
    for (&id, chunk) in archive.chunks() {
        let table = chunk.table();
        if id >= selected.cut.root.next_chunk {
            return Err(Error::Corrupt("checkpoint seeded chunk outside root"));
        }
        if let Some(old) = aliases.chunk(id) {
            if table.identity() != old.identity()
                || table.end() != old.end()
                || table.bounds() != old.bounds()
                || table.len() != old.len()
                || !table
                    .origins()
                    .map(|o| *Borrow::<Origin>::borrow(&o))
                    .eq(old.origins().map(|o| *Borrow::<Origin>::borrow(&o)))
            {
                return Err(Error::Corrupt(
                    "checkpoint changed old exact origin inventory",
                ));
            }
        } else {
            let delta = covered
                .iter()
                .flat_map(|r| &r.record.chunks)
                .rfind(|c| c.chunk == id)
                .ok_or(Error::Corrupt("checkpoint seeded chunk absent from suffix"))?;
            if delta.logical_generation != table.identity().logical_generation
                || delta.capacity != table.identity().capacity
                || delta.after != table.end()
            {
                return Err(Error::Corrupt("checkpoint seeded chunk bound/generation"));
            }
            for row in table.origins() {
                let origin = Borrow::<Origin>::borrow(&row);
                let index = usize::try_from(
                    origin
                        .receipt
                        .creation_lsn
                        .checked_sub(prefix.first_lsn())
                        .ok_or(Error::Corrupt("checkpoint seeded creation below suffix"))?,
                )
                .map_err(|_| Error::LimitExceeded)?;
                let creation = covered
                    .get(index)
                    .ok_or(Error::Corrupt("checkpoint seeded creation absent"))?;
                let r = &creation.record;
                let receipt = SourceReceipt {
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
                    .map_err(|_| Error::Corrupt("checkpoint seeded allocation absent"))?;
                if receipt != origin.receipt || r.objects[at].dest() != origin.extent {
                    return Err(Error::Corrupt("checkpoint seeded exact creation/authority"));
                }
            }
        }
        seen = seen
            .checked_add(table.len() as u64)
            .ok_or(Error::LimitExceeded)?;
        bytes = bytes
            .checked_add(table.identity().capacity)
            .ok_or(Error::LimitExceeded)?;
        directory = directory.insert(Arc::clone(table))?;
    }
    if seen != expected {
        return Err(Error::Corrupt("checkpoint seeded omitted/extra origin"));
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
/// A privately selected compound digest supplies the retained prefix authority.
/// Validate its complete inventory/rows and typed graph without reissuing any
/// creation receipt or trusting archived physical headers as fresh owners.
pub(in crate::hybrid) fn load_selected_seed(
    archive: &CheckpointArchive,
    compound: ulmdb_storage::hybrid::retirement::selection::Compound,
) -> Result<Ledger> {
    let d = archive.descriptor();
    let selected = compound.selector();
    if selector::from_descriptor(d) != selected
        || d.chunks.len() as u64
            != selected
                .cut
                .root
                .next_chunk
                .checked_sub(1)
                .ok_or(Error::Corrupt("compound chunk count"))?
    {
        return Err(Error::Corrupt("compound selected archive binding"));
    }
    let mut directory = ProvenanceDirectory::default();
    let mut bytes = 0u64;
    for (&id, c) in archive.chunks() {
        let table = c.table();
        if id >= selected.cut.root.next_chunk
            || table.bounds().last_creation_lsn > selected.cut.receipt.lsn
            || table.bounds().last_creation_sequence > selected.cut.receipt.sequence
            || table.bounds().last_authority_sequence > selected.cut.receipt.sequence
        {
            return Err(Error::Corrupt("compound future chunk/origin bounds"));
        }
        for row in table.origins() {
            let o = Borrow::<Origin>::borrow(&row);
            let Authority::Wal {
                lsn,
                sequence,
                digest,
            } = o.receipt.authority
            else {
                return Err(Error::Corrupt("compound noncanonical creation authority"));
            };
            if lsn == 0
                || lsn > selected.cut.receipt.lsn
                || sequence > selected.cut.receipt.sequence
                || lsn != o.receipt.creation_lsn
                || sequence != o.receipt.creation_sequence
                || digest != o.receipt.provenance_digest
            {
                return Err(Error::Corrupt("compound exact original WAL authority"));
            }
        }
        bytes = bytes
            .checked_add(table.identity().capacity)
            .ok_or(Error::LimitExceeded)?;
        directory = directory.insert(Arc::clone(table))?;
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
