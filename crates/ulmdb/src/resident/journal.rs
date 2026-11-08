//! Explicit format dispatch outside serving lookup paths; never parser fallback.
use super::RedoFormat;
use crate::{Error, Result};
use std::path::Path;
use ulmdb_storage::{
    Reference,
    hybrid::redo::WalReceipt,
    resident::{journal, journal_v2, wal_codec::Boot},
};
pub(super) enum Journal {
    Literal(journal::Journal),
    Compact(journal_v2::Journal),
}
#[derive(Clone, Copy)]
pub(super) enum State<'a> {
    Literal(&'a ulmdb_storage::resident::wal_codec::ReplayState),
    Compact(&'a ulmdb_storage::resident::wal_codec_v2::ReplayState),
}
impl Journal {
    pub fn capture_snapshot(
        &self,
        view: &ulmdb_storage::resident::View,
    ) -> Result<ulmdb_storage::resident::SnapshotExport> {
        match self {
            Self::Literal(j) => j.capture_snapshot(view),
            Self::Compact(j) => j.capture_snapshot(view),
        }
    }
    pub fn create(path: &Path, boot: Boot, capacity: u64, format: RedoFormat) -> Result<Self> {
        Self::create_with_transform(path, boot, capacity, format, None)
    }
    pub fn create_with_transform(
        path: &Path,
        boot: Boot,
        capacity: u64,
        format: RedoFormat,
        context: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        match format {
            RedoFormat::LiteralV1 => Ok(Self::Literal(journal::Journal::create_with_transform(
                path, boot, capacity, context,
            )?)),
            RedoFormat::CompactV2 | RedoFormat::FullPostimageV2 => Ok(Self::Compact(
                journal_v2::Journal::create_with_transform(path, boot, capacity, context)?,
            )),
        }
    }
    pub fn state(&self) -> State<'_> {
        match self {
            Self::Literal(j) => State::Literal(j.state()),
            Self::Compact(j) => State::Compact(j.state()),
        }
    }
    pub fn can_append(&self, length: u64) -> Result<bool> {
        match self {
            Self::Literal(j) => j.can_append(length),
            Self::Compact(j) => j.can_append(length),
        }
    }
    pub fn physical_bytes(&self) -> Result<u64> {
        match self {
            Self::Literal(j) => j.physical_bytes(),
            Self::Compact(j) => j.physical_bytes(),
        }
    }
    pub fn capacity(&self) -> u64 {
        match self {
            Self::Literal(j) => j.capacity(),
            Self::Compact(j) => j.capacity(),
        }
    }
    pub fn capture_enabled(&self) -> bool {
        match self {
            Self::Literal(j) => j.capture_enabled(),
            Self::Compact(j) => j.capture_enabled(),
        }
    }
    pub fn checkpoint_and_prune(
        &mut self,
        path: &Path,
        view: &ulmdb_storage::resident::View,
        capacity: u64,
        enable: bool,
    ) -> Result<(Self, u64)> {
        match self {
            Self::Literal(j) => {
                let (j, n) = if enable {
                    j.enable_capture_and_prune(path, view, capacity)?
                } else {
                    j.checkpoint_and_prune(path, view, capacity)?
                };
                Ok((Self::Literal(j), n))
            }
            Self::Compact(j) => {
                let (j, n) = if enable {
                    j.enable_capture_and_prune(path, view, capacity)?
                } else {
                    j.checkpoint_and_prune(path, view, capacity)?
                };
                Ok((Self::Compact(j), n))
            }
        }
    }
    pub fn prune_checkpoints(&self, path: &Path) -> Result<u64> {
        match self {
            Self::Literal(j) => j.prune_checkpoints(path),
            Self::Compact(j) => j.prune_checkpoints(path),
        }
    }
    pub fn boot(&self) -> Boot {
        self.state().boot()
    }
    pub fn is_poisoned(&self) -> bool {
        match self {
            Self::Literal(j) => j.is_poisoned(),
            Self::Compact(j) => j.is_poisoned(),
        }
    }
}
impl State<'_> {
    pub fn boot(self) -> Boot {
        match self {
            Self::Literal(s) => s.boot(),
            Self::Compact(s) => s.boot(),
        }
    }
    pub fn selected(self) -> WalReceipt {
        match self {
            Self::Literal(s) => s.selected(),
            Self::Compact(s) => s.selected(),
        }
    }
    pub fn commit(self) -> u128 {
        match self {
            Self::Literal(s) => s.commit(),
            Self::Compact(s) => s.commit(),
        }
    }
    pub fn root(self) -> Reference {
        match self {
            Self::Literal(s) => s.root(),
            Self::Compact(s) => s.root(),
        }
    }
    pub fn next_database(self) -> u64 {
        match self {
            Self::Literal(s) => s.next_database(),
            Self::Compact(s) => s.next_database(),
        }
    }
}
pub(super) fn detect(path: &Path) -> Result<RedoFormat> {
    detect_with_transform(path, None)
}
pub(super) fn detect_with_transform(
    path: &Path,
    context: Option<&std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
) -> Result<RedoFormat> {
    let bytes = ulmdb_storage::resident::journal_magic_with_transform(path, context)?;
    match &bytes {
        b"ULMDBRW1" | b"ULMDBRW3" => Ok(RedoFormat::LiteralV1),
        b"ULMDBRW2" | b"ULMDBRW4" => Ok(RedoFormat::CompactV2),
        _ => Err(Error::Corrupt("unknown resident WAL family; no fallback")),
    }
}
