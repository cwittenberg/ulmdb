//! Immutable mapped segments and writer-private copy-on-write allocation.
macro_rules! fault_point {
    ($name:expr) => {
        #[cfg(feature = "fault-injection")]
        crate::fault::point($name)?;
    };
}
pub(crate) use fault_point;

mod arena;
mod builder;
pub mod chunk_cache;
pub mod chunk_table;
mod cohort;
mod cohort_history;
mod creator_owned;
mod descriptor_cursor;
mod descriptor_extent;
mod descriptor_source;
pub use descriptor_extent::{DescriptorSourceExtent, open_descriptor_source, open_descriptor_generation};
pub use descriptor_cursor::{DescriptorAuthentication, DescriptorCursorBudget, DescriptorCursorProgress, DescriptorFootprint, DescriptorFootprintHeader, FlatDescriptorCursor};
mod indexed_descriptor_cursor;
pub use indexed_descriptor_cursor::{IndexedDescriptorCursor, IndexedDescriptorFootprint, IndexedDescriptorProgress};
pub mod coordination;
mod durability;
#[cfg(feature = "fault-injection")]
mod fault;
mod file_fence;
mod history;
mod history_lifecycle;
pub use history_lifecycle::{HistoryAvailability, HistoryAvailabilityObservation, HistoryDescriptorIdentity, HistoryFileKind, HistoryLifecycleBudget, HistoryPolicy, inspect_history_descriptor, read_history_availability};
pub mod hybrid;
mod manifest;
mod mapping;
mod metadata;
mod publication;
pub mod range;
mod read_ahead;
mod read_mode;
mod residency;
pub mod resident;
#[cfg(all(feature = "snapshot-log", target_os = "linux"))]
pub mod snapshot_input;
#[cfg(all(feature = "snapshot-log", target_os = "linux"))]
pub mod snapshot_log;
#[cfg(all(feature = "snapshot-log", target_os = "linux"))]
pub mod snapshot_versions;
mod table;
mod temporary;
mod work;

pub use arena::{Arena, ArenaCheckpoint, ArenaReservation};
pub use builder::SegmentBuilder;
pub use durability::{Durability, FileSyncReport, SealedAssets, sync_file_environment};
pub use file_fence::WriterFence;
pub use manifest::{
    Manifest, checkpoint_bytes, create_layout, install_snapshot, new_identity, publish,
    read_checkpoint, read_current, read_previous, read_roots, read_roots_checked,
};
pub use publication::read_generation;
pub use publication::{PreparedSnapshotImport, prepare_imported_snapshot};
pub use read_ahead::ReadAhead;
pub use table::{SegmentEntry, SegmentTable, WeakPlainSegmentTable};
pub use ulmdb_core::storage::{Error, ReadStore, Reference, Result, WriteStore};

pub use cohort::{MAX_COHORT_BYTES, descriptor_path, segment_path};
pub use cohort_history::HistoryArchiveReport;
pub use publication::{
    BootstrapPairTarget, FileDurabilityState, PreparedBootstrapPair, PreparedPreviousPublication,
    PreparedPublication, PreparedRollback, PreviousSelection, ReadyPreviousPublication,
    ReadyPublication, ReadyRollback, SelectorObservation, capture_previous_selection,
    checkpoint_identity, observe_selector,
    prepare_existing, prepare_previous_publication, prepare_publication,
    prepare_publication_with_durability, prepare_publication_with_sealed_data, prepare_rollback,
    prepare_rollback_with_durability,
};

/// Opt-in test hook; absent from normal builds.
#[cfg(feature = "fault-injection")]
pub fn injected_fault(name: &str) -> Result<()> {
    fault::point(name)
}

pub use metadata::{Inventory, MAX_DESCRIPTOR_BYTES, MetadataOriginCursor};

pub use publication::read_checkpoint_with_fallback;

pub use residency::{
    ResidencyAdmission, ResidencyController, ResidencyPolicy, ResidencyReport, ResidencyStatus,
};

pub mod transform;

pub use publication::{
    read_checkpoint_with_transform, read_current_with_transform, read_generation_with_transform,
    read_roots_checked_with_transform, read_roots_with_transform,
};

pub use publication::{
    prepare_existing_with_transform, prepare_imported_snapshot_with_transform,
    read_checkpoint_with_fallback_and_transform,
};

pub use durability::sync_file_environment_with_transform;

pub mod access;

pub mod external;

pub use publication::ExternalReadyPublication;

pub use publication::ExternalSnapshotImport;

pub use arena::{MappedAllocation, MappedFlush};

pub use durability::{FileAsyncFlushReport, flush_async_file_environment};
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub mod fixed_address;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub mod fixed_map;
