//! Selected compound replay into fresh physical owners; full WAL stays retained.
use super::{
    Options,
    recover::{Rebuilt, ReplayView, materialize},
    retirement::SeedContext,
};
use crate::{Error, Result};
use std::{path::Path, sync::Arc};
use ulmdb_storage::hybrid::{
    checkpoint::{CheckpointArchive, CheckpointLimits},
    redo::ChunkDelta,
    retirement::{CheckpointAliases, SeededPrefix, selection::Compound},
};
pub(super) fn open_archive(
    directory: &Path,
    compound: Compound,
    options: Options,
) -> Result<CheckpointArchive> {
    let selected = compound.selector();
    let path = selected.directory(directory);
    if !std::fs::symlink_metadata(directory.join("HYBRID.checkpoints"))?
        .file_type()
        .is_dir()
        || !std::fs::symlink_metadata(&path)?.file_type().is_dir()
    {
        return Err(Error::Corrupt("compound checkpoint directory type"));
    }
    let limits = CheckpointLimits::default();
    let d = ulmdb_storage::hybrid::checkpoint::read_descriptor(
        &path.join("CHECKPOINT"),
        limits.max_chunks,
    )?;
    if super::checkpoint::selector::from_descriptor(&d) != selected {
        return Err(Error::Corrupt("compound selected descriptor"));
    }
    let bytes = d
        .chunks
        .iter()
        .try_fold(0u64, |n, c| n.checked_add(c.identity.capacity))
        .ok_or(Error::LimitExceeded)?;
    if bytes > options.max_image_bytes {
        return Err(Error::LimitExceeded);
    }
    CheckpointArchive::open(&path, limits)
}
fn preflight(plan: &SeededPrefix, archive: &CheckpointArchive, options: Options) -> Result<u64> {
    let mut bytes = archive
        .chunks()
        .try_fold(0u64, |n, (_, c)| {
            n.checked_add(c.table().identity().capacity)
        })
        .ok_or(Error::LimitExceeded)?;
    let base_next = plan.boundary.root.next_chunk;
    let mut next = base_next;
    let mut last: Option<ChunkDelta> = None;
    for decoded in plan.records() {
        let r = &decoded.record;
        if r.chunks.is_empty() {
            if !r.objects.is_empty() || r.root.next_chunk != next {
                return Err(Error::Corrupt("seeded metadata-only lineage"));
            }
            continue;
        }
        if r.chunks.len() != 1 {
            return Err(Error::Corrupt("seeded active delta count"));
        }
        let c = r.chunks[0];
        if c.chunk < base_next {
            return Err(Error::Corrupt("seeded suffix reuses sealed CP chunk"));
        }
        if let Some(old) = last.filter(|old| old.chunk == c.chunk) {
            if c.logical_generation != old.logical_generation
                || c.before != old.after
                || c.capacity != old.capacity
            {
                return Err(Error::Corrupt("seeded active chunk identity/bound"));
            }
        } else {
            if c.chunk != next || c.before != 4096 {
                return Err(Error::Corrupt("seeded new chunk lineage"));
            }
            bytes = bytes.checked_add(c.capacity).ok_or(Error::LimitExceeded)?;
        }
        next = c.chunk.checked_add(1).ok_or(Error::LimitExceeded)?;
        if r.root.next_chunk != next {
            return Err(Error::Corrupt("seeded next chunk counter"));
        }
        last = Some(c);
    }
    let admitted = if last.is_none() {
        bytes
            .checked_add(options.chunk.capacity as u64)
            .ok_or(Error::LimitExceeded)?
    } else {
        bytes
    };
    if admitted > options.max_image_bytes || plan.selected.receipt.end()? > options.max_wal_bytes {
        return Err(Error::LimitExceeded);
    }
    Ok(bytes)
}
pub(super) fn rebuild(
    directory: &Path,
    fence: &Arc<ulmdb_storage::WriterFence>,
    options: Options,
    plan: &SeededPrefix,
    archive: &CheckpointArchive,
    compound: Compound,
    aliases: Arc<CheckpointAliases>,
) -> Result<Rebuilt> {
    let bytes = preflight(plan, archive, options)?;
    let ledger = super::checkpoint::base::load_selected_seed(archive, compound)?;
    let seed = Arc::new(SeedContext { compound, aliases });
    materialize(
        directory,
        fence,
        options,
        ReplayView {
            header: plan.header,
            selected: plan.selected,
            root: plan.root(),
            records: plan.records(),
        },
        Some(archive),
        ledger,
        bytes,
        Some(compound.selector()),
        Some(seed),
    )
}
