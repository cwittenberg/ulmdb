//! File-backed ULMDB environment, atomic catalog/index commits and pinned readers.
pub mod backup;
mod bulk;
mod catalog;
pub mod chunked;
mod client_snapshot;
pub use chunked::ChunkedReadTxn;
mod client_views;
mod compact;
mod coordinated;
mod database_view;
mod detached;
mod duplicates;
mod durability;
mod environment;
mod file_capture;
mod handles;
pub mod hybrid;
pub mod lookup;
mod history_retention;
pub use ulmdb_storage::{HistoryAvailability, HistoryLifecycleBudget, HistoryPolicy};
mod maintenance;
mod migration;
pub use maintenance::{MaintenancePolicy, MaintenanceStatus, MaintenanceTransfer, ResumableReclaimBudget};
pub mod ordering;
mod ordering_audit;
mod physical;
mod physical_stats;
mod prepared;
mod previous;
mod prune;
#[cfg(feature = "range-lifecycle")]
mod range_retention;
#[cfg(feature="range-lifecycle")]
mod range_compaction;
#[cfg(feature="range-lifecycle")]
pub use range_compaction::RangeCompactReport;
mod read;
mod reclaim;
pub mod replication;
mod residency;
pub mod resident;
#[cfg(all(feature = "snapshot-log", target_os = "linux"))]
pub mod snapshot_log;
mod retention;
mod rollback;
mod savepoint;
mod stats;
pub use physical_stats::DatabaseStats;
mod views;
mod write;
mod write_map;
pub use catalog::DatabaseKind;
pub use compact::CompactionReport;
pub use coordinated::CoordinatedEnvironment;
pub use database_view::{
    DatabaseReadScope, DatabaseReadView, DatabaseView, DatabaseViewOrigin, DatabaseViewTransfer,
    RetainedDatabaseView, RetainedViewGroup,
};
pub use detached::DetachedDatabase;
pub use duplicates::{DuplicatePairs, DuplicateValues, DuplicatesRead};
pub use environment::{Environment, PublicationMode};
pub use file_capture::{FileCopyPair, FileCopyView, OwnedFileCapture, OwnedFileCopyPairCapture};
pub use handles::DatabaseHandle;
pub use prepared::{CommitIntent, PreparedWriteTxn};
pub use previous::{PreviousError, PreviousSession};
pub use prune::PruneReport;
pub use read::{KvPosition, ReadTxn};
pub use reclaim::ReclaimReport;
#[cfg(target_os = "linux")]
pub use reclaim::{ReclaimPlan, ReclaimPlanLimits, ReclaimProgress, ReclaimStepBudget};
pub use retention::RetainedSnapshot;
pub use rollback::{RollbackError, RollbackReport};
pub use savepoint::Savepoint;
pub use stats::Stats;
pub use ulmdb_core::storage::{Error, Result};
pub use ulmdb_core::{Address, Prefix};
pub use ulmdb_storage::{Durability, FileSyncReport, sync_file_environment};
pub use ulmdb_storage::{ReadAhead, ResidencyPolicy, ResidencyReport, ResidencyStatus};
pub use views::{KvRead, PrefixRead};
pub use write::WriteTxn;

#[cfg(test)]
mod inventory_tests;

#[cfg(test)]
mod range_tests;

pub use ulmdb_core::{PageSize, ValueLimit};
pub use ulmdb_storage::{MappedAllocation, MappedFlush};

mod reserve;
pub use reserve::ValueReservation;

mod file_access;
pub use file_access::{AccessMigrationReport, FileAccess, FileAccessContext};

mod external;
pub use external::ExternalSynchronization;

pub use external::ExternalPreviousCapture;
pub use file_capture::ExternalFileCopyPairCapture;

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
mod fixed_map;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub use ulmdb_storage::fixed_map::{
    FixedMap, FixedMapDeclaration, FixedMapInfo, FixedSlotReclaimLimits, FixedSlotReclaimReport,
};

mod catalog_profile;
pub use ulmdb_core::CatalogCapacity;

mod logical_size;
pub use logical_size::LogicalDatabaseSize;
