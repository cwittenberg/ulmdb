//! Backend strategy selected at the API boundary, never inside a tree traversal.
use super::{ReadTxn, Snapshot, WriteTxn, provenance::Ledger};
use crate::{Error, Result};
use arc_swap::ArcSwap;
use std::{
    collections::{BTreeMap, LinkedList},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use ulmdb_storage::hybrid::{
    ChunkLimits, ChunkWriter, ImageMode, ImageTable,
    journal::{Bootstrap, Journal, WalLayout},
    provenance::ChunkIdentity,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RedoMode {
    #[default]
    Compact,
    FullPostimage,
}
/// Experimental live writer recipe; public owned controls remain the default.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LiveRedoMode {
    #[default]
    Owned,
    Borrowed,
}
#[derive(Clone, Copy, Debug)]
pub struct LiveRedoUsage {
    pub configured: LiveRedoMode,
    pub last_committed: Option<LiveRedoMode>,
    pub encoding_capacity_bytes: usize,
    pub patch_metadata_capacity_bytes: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// File remains default; anonymous replay cache is explicit and WAL stays durable.
    pub image: ImageMode,
    pub wal_layout: WalLayout,
    pub chunk: ChunkLimits,
    pub max_image_bytes: u64,
    pub max_wal_bytes: u64,
    pub redo: RedoMode,
    pub live_redo: LiveRedoMode,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            image: ImageMode::File,
            wal_layout: WalLayout::Dense,
            chunk: ChunkLimits::default(),
            max_image_bytes: 256 * 1024 * 1024,
            max_wal_bytes: 256 * 1024 * 1024,
            redo: RedoMode::Compact,
            live_redo: LiveRedoMode::Owned,
        }
    }
}
impl Options {
    pub(super) fn validate(self) -> Result<()> {
        self.wal_layout.validate()?;
        if let WalLayout::PreSized { capacity } = self.wal_layout
            && capacity > self.max_wal_bytes
        {
            return Err(Error::InvalidInput("hybrid WAL capacity exceeds policy"));
        }
        if self.max_image_bytes < self.chunk.capacity as u64
            || self.max_image_bytes > 268_435_456
            || self.max_wal_bytes > 268_435_456
            || self.max_wal_bytes < 196608
        {
            return Err(Error::InvalidInput("hybrid pilot capacity policy"));
        }
        Ok(())
    }
}
/// Cold diagnostic counters, separate from operating-system RSS/PSS observation.
#[derive(Clone, Copy, Debug)]
pub struct ImageUsage {
    pub mode: ImageMode,
    pub virtual_capacity_bytes: u64,
    pub owned_cache_files: usize,
    pub active: ulmdb_storage::hybrid::ChunkStatus,
}
pub struct Environment {
    pub(super) current: ArcSwap<Snapshot>,
    pub(super) owner: Arc<()>,
    pub(super) checkpoint_ops: Mutex<()>,
    pub(super) writer: Mutex<Writer>,
    pub(super) options: Options,
    // Mutable/frozen image permits retain clones; published read spans do not.
    pub(super) fence: Arc<ulmdb_storage::WriterFence>,
    pub(super) directory: PathBuf,
}
pub(super) struct Writer {
    pub journal: Journal,
    pub last_live_redo: Option<LiveRedoMode>,
    pub recipe_workspace: ulmdb_storage::hybrid::borrowed::RecipeWorkspace,
    pub checkpoint_poisoned: bool,
    pub recovery_base: Option<ulmdb_storage::hybrid::checkpoint::selection::Selector>,
    pub seed: Option<Arc<super::retirement::SeedContext>>,
    pub chunk: ChunkWriter,
    pub generation: u128,
    pub ledger: Ledger,
    pub next_database: u64,
    pub image_bytes: u64,
    pub image_paths: LinkedList<PathBuf>,
}
impl Drop for Writer {
    fn drop(&mut self) {
        // Images are replay caches, never recovery authority. Unlink is safe with
        // escaped read mappings: their owners retain the original inode bytes.
        // Pop each node before dropping its payload: no recursively owned path
        // chain remains for the field destructor, even on a tiny thread stack.
        while let Some(path) = self.image_paths.pop_front() {
            let _ = std::fs::remove_file(path);
        }
    }
}
impl Environment {
    pub fn create(path: impl AsRef<Path>, options: Options) -> Result<Self> {
        options.validate()?;
        let directory = path.as_ref();
        ulmdb_storage::create_layout(directory)?;
        let fence = Arc::new(crate::Environment::lock(directory)?);
        let header = Bootstrap {
            uuid: ulmdb_storage::new_identity()?.to_le_bytes(),
            epoch: 1,
            commit: ulmdb_storage::new_identity()?,
        };
        let (chunk, image_path) = new_chunk(directory, &fence, 1, options.chunk, options.image)?;
        let image = ImageTable::default();
        let generation = ulmdb_storage::new_identity()?;
        let ledger = Ledger::new(ChunkIdentity {
            uuid: header.uuid,
            epoch: header.epoch,
            chunk: 1,
            logical_generation: generation,
            capacity: options.chunk.capacity as u64,
        })?;
        let journal =
            Journal::create_with_layout(&directory.join("HYBRID.wal"), header, options.wal_layout)?;
        let snapshot = Snapshot {
            image,
            catalog: BTreeMap::new(),
            root: journal.root(),
            receipt: journal.selected(),
            commit: journal.commit_id(),
        };
        Ok(Self {
            current: ArcSwap::from_pointee(snapshot),
            owner: Arc::new(()),
            checkpoint_ops: Mutex::new(()),
            options,
            writer: Mutex::new(Writer {
                journal,
                last_live_redo: None,
                recipe_workspace: Default::default(),
                checkpoint_poisoned: false,
                recovery_base: None,
                seed: None,
                chunk,
                generation,
                ledger,
                next_database: 1,
                image_bytes: options.chunk.capacity as u64,
                image_paths: image_path.into_iter().collect(),
            }),
            fence,
            directory: directory.to_path_buf(),
        })
    }
    pub fn open(path: impl AsRef<Path>, options: Options) -> Result<Self> {
        options.validate()?;
        let directory = path.as_ref();
        let fence = Arc::new(crate::Environment::lock(directory)?);
        use ulmdb_storage::hybrid::retirement::selection::{RecoveryBase, read_typed};
        let required = super::retirement::marker::read(directory)?;
        let selected = read_typed(directory)?;
        if let Some(required) = required {
            let Some(RecoveryBase::Compound(c)) = selected else {
                return Err(Error::Corrupt("required V2 selector missing/legacy"));
            };
            required.check(c)?;
        }
        let (journal, rebuilt) = if let Some(RecoveryBase::Compound(compound)) = selected {
            let archive = super::recover_seeded::open_archive(directory, compound, options)?;
            let aliases = Arc::new(
                ulmdb_storage::hybrid::retirement::CheckpointAliases::from_archive(&archive)?,
            );
            Journal::open_seeded_checked(
                &directory.join("HYBRID.wal"),
                compound,
                &aliases,
                |plan| {
                    if plan.layout != options.wal_layout {
                        return Err(Error::InvalidInput(
                            "hybrid WAL persisted layout/options mismatch",
                        ));
                    }
                    super::recover_seeded::rebuild(
                        directory,
                        &fence,
                        options,
                        plan,
                        &archive,
                        compound,
                        Arc::clone(&aliases),
                    )
                },
            )?
        } else {
            Journal::open_checked(&directory.join("HYBRID.wal"), |plan| {
                if plan.layout != options.wal_layout {
                    return Err(Error::InvalidInput(
                        "hybrid WAL persisted layout/options mismatch",
                    ));
                }
                super::recover::rebuild(directory, &fence, options, plan)
            })?
        };
        // A first-selection cut may contain a durable V2 selector but no marker.
        // Repair only after complete fresh-image/typed graph readiness, before
        // returning a writable Environment. Never infer an older base.
        if required.is_none()
            && let Some(RecoveryBase::Compound(compound)) = selected
        {
            super::retirement::publication::Publication::required(directory, compound)?
                .publish()
                .map_err(|_| Error::OutcomeUnknown)?;
        }
        let super::recover::Rebuilt {
            snapshot,
            chunk,
            generation,
            ledger,
            image_bytes,
            paths: image_paths,
            recovery_base,
            seed,
        } = rebuilt;
        let next_database = snapshot.root.next_database;
        Ok(Self {
            current: ArcSwap::from_pointee(snapshot),
            owner: Arc::new(()),
            checkpoint_ops: Mutex::new(()),
            writer: Mutex::new(Writer {
                journal,
                last_live_redo: None,
                recipe_workspace: Default::default(),
                checkpoint_poisoned: false,
                recovery_base,
                seed,
                chunk,
                generation,
                ledger,
                next_database,
                image_bytes,
                image_paths,
            }),
            options,
            fence,
            directory: directory.to_path_buf(),
        })
    }
    /// Cold observation of the branch actually used by the last complete commit.
    pub fn live_redo_usage(&self) -> Result<LiveRedoUsage> {
        let w = self.writer.lock().map_err(|_| Error::Busy)?;
        Ok(LiveRedoUsage {
            configured: self.options.live_redo,
            last_committed: w.last_live_redo,
            encoding_capacity_bytes: w.journal.encoding_workspace_bytes(),
            patch_metadata_capacity_bytes: w.recipe_workspace.retained_bytes(),
        })
    }
    pub fn begin_read(&self) -> ReadTxn {
        ReadTxn {
            snapshot: self.current.load_full(),
        }
    }
    pub fn begin_write(&self) -> Result<WriteTxn<'_>> {
        let mut writer = self.writer.lock().map_err(|_| Error::Busy)?;
        if writer.checkpoint_poisoned || writer.journal.is_poisoned() {
            return Err(Error::OutcomeUnknown);
        }
        writer.ledger.healthy()?;
        let base = self.current.load_full();
        let arena = writer.chunk.begin(base.image.clone())?;
        Ok(WriteTxn {
            env: self,
            guard: writer,
            catalog: base.catalog.clone(),
            changed: Default::default(),
            arena,
            base,
            failed: false,
        })
    }
    /// Prepare a new bounded active chunk. The next successful transaction
    /// publishes it atomically; an empty pending chunk is reused on retry.
    pub fn rotate_chunk(&self) -> Result<()> {
        let mut writer = self.writer.lock().map_err(|_| Error::Busy)?;
        if writer.checkpoint_poisoned || writer.journal.is_poisoned() {
            return Err(Error::OutcomeUnknown);
        }
        self.rotate_locked(&mut writer)?;
        Ok(())
    }
    pub(super) fn rotate_locked(&self, writer: &mut Writer) -> Result<bool> {
        writer.ledger.healthy()?;
        if writer.checkpoint_poisoned || writer.journal.is_poisoned() {
            return Err(Error::OutcomeUnknown);
        }
        let base = self.current.load_full();
        if writer.chunk.chunk_id() == base.root.next_chunk {
            return Ok(false);
        }
        let committed_end = base
            .image
            .chunk(writer.chunk.chunk_id())
            .ok_or(Error::Corrupt("hybrid selected active image absent"))?
            .end() as u64;
        writer
            .ledger
            .check_rotate(committed_end, base.receipt.lsn, base.receipt.sequence)?;
        let total = writer
            .image_bytes
            .checked_add(self.options.chunk.capacity as u64)
            .filter(|n| *n <= self.options.max_image_bytes)
            .ok_or(Error::LimitExceeded)?;
        let generation = ulmdb_storage::new_identity()?;
        let (chunk, path) = new_chunk(
            &self.directory,
            &self.fence,
            base.root.next_chunk,
            self.options.chunk,
            self.options.image,
        )?;
        let header = writer.journal.bootstrap();
        if let Err(error) = writer.ledger.rotate(
            ChunkIdentity {
                uuid: header.uuid,
                epoch: header.epoch,
                chunk: base.root.next_chunk,
                logical_generation: generation,
                capacity: self.options.chunk.capacity as u64,
            },
            committed_end,
            base.receipt.lsn,
            base.receipt.sequence,
        ) {
            drop(chunk);
            if let Some(path) = path {
                let _ = std::fs::remove_file(&path);
            }
            return Err(error);
        }
        if let Some(path) = path {
            writer.image_paths.push_back(path);
        }
        writer.chunk = chunk;
        writer.generation = generation;
        writer.image_bytes = total;
        Ok(true)
    }
    /// Outside transaction/timing paths. No mapping scan or residency promise.
    pub fn image_usage(&self) -> Result<ImageUsage> {
        let writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        Ok(ImageUsage {
            mode: self.options.image,
            virtual_capacity_bytes: writer.image_bytes,
            owned_cache_files: writer.image_paths.len(),
            active: writer.chunk.status(),
        })
    }
    /// The exact configured pilot strategy; no fallback mode is hidden.
    pub fn options(&self) -> Options {
        self.options
    }
    pub fn sequence(&self) -> u64 {
        self.current.load().receipt.sequence
    }
    pub fn wal_bytes(&self) -> Result<u64> {
        self.current.load().receipt.end()
    }
}
pub(super) fn new_chunk(
    directory: &Path,
    fence: &Arc<ulmdb_storage::WriterFence>,
    id: u64,
    limits: ChunkLimits,
    mode: ImageMode,
) -> Result<(ChunkWriter, Option<PathBuf>)> {
    if mode == ImageMode::Anonymous {
        return Ok((
            ChunkWriter::create_anonymous_with_fence(id, limits, Arc::clone(fence))?,
            None,
        ));
    }
    let path = directory.join(format!(
        "image-{:032x}-{id}.cache",
        ulmdb_storage::new_identity()?
    ));
    let writer = ChunkWriter::create_with_fence(&path, id, limits, Arc::clone(fence))?;
    Ok((writer, Some(path)))
}
