//! Bounded Resident ownership with streamed authenticated checkpoint support.
//! Existing hybrid ReadSpan, append-only formats and defaults are untouched.
mod capacity;
mod capture;
pub use capture::{CaptureLimits, ReadCapture};
mod checkpoint;
mod directory;
mod extent;
pub use extent::{ExtentSource, MAX_OVERFLOW_VALUE_BYTES, OverflowStats, reference_version};
pub mod journal;
pub mod journal_v2;
mod pool;
mod private_workspace;
mod read_index;
mod snapshot_export;
pub use snapshot_export::{SnapshotExport, SnapshotExportReport, install_snapshot_export};
mod store;
pub mod versioned_redo;
pub mod wal_codec;
pub mod wal_codec_v2;
pub use directory::View;
pub use pool::{Config, Pool, Stats, VersionKey};
#[cfg(feature = "resident-private-index")]
pub use private_workspace::PrivateCache;
pub use private_workspace::PrivateWorkspaceStats;
pub use store::{Arena, ArenaCheckpoint, FrozenSlot, Prepared};
#[cfg(test)]
mod tests;

mod private_quota;

pub fn journal_magic_with_transform(
    path: &std::path::Path,
    context: Option<&std::sync::Arc<crate::transform::TransformContext>>,
) -> crate::Result<[u8; 8]> {
    crate::transform::resident_magic(path, context)
}
