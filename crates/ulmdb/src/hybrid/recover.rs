//! Cold replay into fresh mapping incarnations; no writes to escaped old readers.
use super::{
    Options, Snapshot,
    checkpoint::{base, selector},
    environment::new_chunk,
    provenance::{Allocation, CheckedImage, Ledger, Pending, Sources},
};
use crate::{DatabaseKind, Error, Result, catalog::Database};
use std::{
    collections::{BTreeMap, BTreeSet, LinkedList},
    ops::Bound,
    path::{Path, PathBuf},
    sync::Arc,
};
use ulmdb_storage::hybrid::{
    ChunkLimits, ChunkWriter, ImageTable,
    provenance::ChunkIdentity,
    recovery::RecoveryPlan,
    redo::{self, Authority, SourceReceipt},
};

pub(super) struct Rebuilt {
    pub snapshot: Snapshot,
    pub chunk: ChunkWriter,
    pub generation: u128,
    pub ledger: Ledger,
    pub image_bytes: u64,
    pub paths: LinkedList<PathBuf>,
    pub recovery_base: Option<selector::Selector>,
    pub seed: Option<Arc<super::retirement::SeedContext>>,
}
struct Paths(LinkedList<PathBuf>);
impl Drop for Paths {
    fn drop(&mut self) {
        while let Some(path) = self.0.pop_front() {
            let _ = std::fs::remove_file(path);
        }
    }
}
/// Validate complete chunk lineage/resource admission before creating images.
fn preflight(plan: &RecoveryPlan, options: Options) -> Result<u64> {
    let mut chunks = BTreeMap::new();
    let mut active = 0;
    let mut total = 0u64;
    let mut next_chunk = 1; // exact canonical Journal genesis; cache SID1 is unpublished
    for decoded in &plan.records {
        let r = &decoded.record;
        if r.chunks.is_empty() {
            if !r.objects.is_empty() || r.root.next_chunk != next_chunk {
                return Err(Error::Corrupt("hybrid metadata-only chunk lineage"));
            }
            continue;
        }
        // Data records publish exactly one active chunk delta.
        if r.chunks.len() != 1 {
            return Err(Error::Corrupt("hybrid pilot active delta count"));
        }
        let c = r.chunks[0];
        if c.chunk < active || c.chunk >= r.root.next_chunk {
            return Err(Error::Corrupt("hybrid chunk lineage order"));
        }
        if let Some((generation, end, capacity)) = chunks.get(&c.chunk).copied() {
            if c.logical_generation != generation || c.before != end || c.capacity != capacity {
                return Err(Error::Corrupt("hybrid chunk base/generation/capacity"));
            }
        } else {
            if c.before != 4096
                || (active != 0 && c.chunk != active + 1)
                || (active == 0 && c.chunk != 1)
            {
                return Err(Error::Corrupt("hybrid new chunk lineage"));
            }
            total = total.checked_add(c.capacity).ok_or(Error::LimitExceeded)?;
            if total > options.max_image_bytes {
                return Err(Error::LimitExceeded);
            }
        }
        if r.root.next_chunk != c.chunk.checked_add(1).ok_or(Error::LimitExceeded)? {
            return Err(Error::Corrupt("hybrid next chunk counter"));
        }
        next_chunk = r.root.next_chunk;
        active = c.chunk;
        chunks.insert(c.chunk, (c.logical_generation, c.after, c.capacity));
    }
    if plan.selected.receipt.end()? > options.max_wal_bytes {
        return Err(Error::LimitExceeded);
    }
    Ok(total)
}
pub(super) fn rebuild(
    directory: &Path,
    fence: &Arc<ulmdb_storage::WriterFence>,
    options: Options,
    plan: &RecoveryPlan,
) -> Result<Rebuilt> {
    let bytes = preflight(plan, options)?;
    let recovery_base = selector::read(directory)?;
    let limits = ulmdb_storage::hybrid::checkpoint::CheckpointLimits::default();
    let archive = if let Some(selected) = recovery_base {
        let path = selected.directory(directory);
        if !std::fs::symlink_metadata(directory.join("HYBRID.checkpoints"))?
            .file_type()
            .is_dir()
            || !std::fs::symlink_metadata(&path)?.file_type().is_dir()
        {
            return Err(Error::Corrupt("checkpoint selected directory type"));
        }
        let descriptor = ulmdb_storage::hybrid::checkpoint::read_descriptor(
            &path.join("CHECKPOINT"),
            limits.max_chunks,
        )?;
        if base::check_descriptor(&descriptor, selected, plan.header, &plan.records)?
            > options.max_image_bytes
        {
            return Err(Error::LimitExceeded);
        }
        Some(ulmdb_storage::hybrid::checkpoint::CheckpointArchive::open(
            &path, limits,
        )?)
    } else {
        None
    };
    let initial_ledger = if let (Some(archive), Some(selected)) = (&archive, recovery_base) {
        Some(base::verify_archive(
            archive,
            selected,
            plan.header,
            &plan.records,
        )?)
    } else {
        None
    };
    materialize(
        directory,
        fence,
        options,
        ReplayView {
            header: plan.header,
            selected: plan.selected,
            root: plan.root(),
            records: &plan.records,
        },
        archive.as_ref(),
        initial_ledger.unwrap_or_default(),
        bytes,
        recovery_base,
        None,
    )
}
pub(super) struct ReplayView<'a> {
    pub header: ulmdb_storage::hybrid::journal::Bootstrap,
    pub selected: ulmdb_storage::hybrid::journal::Witness,
    pub root: ulmdb_storage::hybrid::redo::RootState,
    pub records: &'a [ulmdb_storage::hybrid::codec::Decoded],
}
impl ReplayView<'_> {
    fn root(&self) -> ulmdb_storage::hybrid::redo::RootState {
        self.root
    }
}
// Both full and seeded recovery supply these separately checked authorities
// explicitly; this cold helper does not infer archive/ledger/seed from a path.
#[allow(clippy::too_many_arguments)]
pub(super) fn materialize(
    directory: &Path,
    fence: &Arc<ulmdb_storage::WriterFence>,
    options: Options,
    plan: ReplayView<'_>,
    archive: Option<&ulmdb_storage::hybrid::checkpoint::CheckpointArchive>,
    mut ledger: Ledger,
    mut bytes: u64,
    recovery_base: Option<selector::Selector>,
    seed: Option<Arc<super::retirement::SeedContext>>,
) -> Result<Rebuilt> {
    super::cache::remove_old_names(directory)?;
    let mut paths = Paths(LinkedList::new());
    let mut image = ImageTable::default();
    let mut current: Option<ChunkWriter> = None;
    let mut current_id = 0;
    let mut generation = 0;
    if let Some(archive) = archive {
        for (&id, source) in archive.chunks() {
            let identity = source.table().identity();
            let capacity = usize::try_from(identity.capacity).map_err(|_| Error::LimitExceeded)?;
            let limits = ChunkLimits {
                capacity,
                reserve_quantum: options.chunk.reserve_quantum.min(capacity),
                max_burned_bytes: options.chunk.max_burned_bytes.min(capacity),
            };
            let (mut writer, path) = new_chunk(directory, fence, id, limits, options.image)?;
            if let Some(path) = path {
                paths.0.push_back(path);
            }
            let mut arena = writer.begin(image.clone())?;
            for origin in source.table().origins() {
                arena.allocate_at(
                    origin.extent.at,
                    source.read(
                        origin.extent.at.offset as usize,
                        origin.extent.length as usize,
                    )?,
                )?;
            }
            arena.advance_to(source.table().end() as usize)?;
            let frozen = arena.freeze()?;
            image = image.with_active(frozen.view())?;
            frozen.accept();
            current_id = id;
        }
    }
    let through = recovery_base.map_or(0, |s| s.cut.receipt.lsn);
    for decoded in plan.records.iter().filter(|d| d.record.lsn > through) {
        let record = &decoded.record;
        let materialized = redo::replay(
            record,
            &Sources {
                image: &image,
                ledger: &ledger,
                sequence: record.parent.sequence,
            },
        )?;
        let Some(&delta) = record.chunks.first() else {
            continue;
        };
        if delta.chunk != current_id {
            let capacity = usize::try_from(delta.capacity).map_err(|_| Error::LimitExceeded)?;
            let limits = ChunkLimits {
                capacity,
                reserve_quantum: options.chunk.reserve_quantum.min(capacity),
                max_burned_bytes: options.chunk.max_burned_bytes.min(capacity),
            };
            let old_end = if current_id == 0 {
                4096
            } else {
                image
                    .chunk(current_id)
                    .ok_or(Error::Corrupt("hybrid previous image missing"))?
                    .end() as u64
            };
            ledger.rotate(
                ChunkIdentity {
                    uuid: record.uuid,
                    epoch: record.epoch,
                    chunk: delta.chunk,
                    logical_generation: delta.logical_generation,
                    capacity: delta.capacity,
                },
                old_end,
                record.parent.lsn,
                record.parent.sequence,
            )?;
            let (writer, path) = new_chunk(directory, fence, delta.chunk, limits, options.image)?;
            if let Some(path) = path {
                paths.0.push_back(path);
            }
            image = image.with_active(writer.read_view())?;
            current = Some(writer);
            current_id = delta.chunk;
            generation = delta.logical_generation;
        }
        let writer = current
            .as_mut()
            .ok_or(Error::Corrupt("hybrid replay writer absent"))?;
        let mut arena = writer.begin(image.clone())?;
        for object in &materialized {
            arena.allocate_at(object.extent.at, &object.bytes)?;
        }
        arena.advance_to(usize::try_from(delta.after).map_err(|_| Error::LimitExceeded)?)?;
        let frozen = arena.freeze()?;
        image = image.with_active(frozen.view())?;
        frozen.accept();
        let mut pending = Pending::new(&ledger, materialized.len())?;
        for object in materialized {
            pending.insert(Allocation {
                extent: object.extent,
                receipt: SourceReceipt {
                    uuid: record.uuid,
                    epoch: record.epoch,
                    creation_lsn: record.lsn,
                    creation_sequence: record.sequence,
                    creation_commit: record.commit,
                    provenance_digest: decoded.receipt.digest,
                    authority: Authority::Wal {
                        lsn: record.lsn,
                        sequence: record.sequence,
                        digest: decoded.receipt.digest,
                    },
                },
            })?;
        }
        pending.accept();
    }
    if current.is_none() {
        bytes = bytes
            .checked_add(options.chunk.capacity as u64)
            .filter(|n| *n <= options.max_image_bytes)
            .ok_or(Error::LimitExceeded)?;
        let id = plan.root().next_chunk;
        let (writer, path) = new_chunk(directory, fence, id, options.chunk, options.image)?;
        if let Some(path) = path {
            paths.0.push_back(path);
        }
        generation = ulmdb_storage::new_identity()?;
        let end = if current_id == 0 {
            4096
        } else {
            image
                .chunk(current_id)
                .ok_or(Error::Corrupt("checkpoint last sealed image absent"))?
                .end() as u64
        };
        ledger.rotate(
            ChunkIdentity {
                uuid: plan.header.uuid,
                epoch: plan.header.epoch,
                chunk: id,
                logical_generation: generation,
                capacity: options.chunk.capacity as u64,
            },
            end,
            plan.selected.receipt.lsn,
            plan.selected.receipt.sequence,
        )?;
        current = Some(writer);
    }
    let root = plan.root();
    let checked = CheckedImage {
        image: &image,
        ledger: &ledger,
    };
    crate::backup::validate_materialized_graph(&checked, root.catalog, root.next_database, bytes)?;
    let mut catalog = BTreeMap::new();
    let mut identities = BTreeSet::new();
    for item in ulmdb_kv::Cursor::new(
        &checked,
        root.catalog,
        Bound::Unbounded,
        Bound::Unbounded,
        false,
    )? {
        let (name, value) = item?;
        let db = Database::decode(value)?;
        if name.is_empty()
            || name.len() > 255
            || catalog.len() >= 4096
            || db.id >= root.next_database
            || !identities.insert(db.id)
            || matches!(db.kind, DatabaseKind::Duplicates { .. })
        {
            return Err(Error::Corrupt("hybrid catalog descriptor/limit"));
        }
        catalog.insert(name.to_vec(), db);
    }
    Ok(Rebuilt {
        snapshot: Snapshot {
            image,
            catalog,
            root,
            receipt: plan.selected.receipt,
            commit: plan.selected.commit,
        },
        chunk: current.ok_or(Error::Corrupt("hybrid replay no active chunk"))?,
        generation,
        ledger,
        image_bytes: bytes.max(options.chunk.capacity as u64),
        paths: std::mem::take(&mut paths.0),
        recovery_base,
        seed,
    })
}

#[cfg(test)]
mod path_drop_tests {
    use super::*;
    #[test]
    fn many_image_paths_drop_iteratively_on_a_small_stack() {
        let paths = Paths((0..4096).map(|_| PathBuf::new()).collect());
        std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || drop(paths))
            .unwrap()
            .join()
            .unwrap();
    }
}
