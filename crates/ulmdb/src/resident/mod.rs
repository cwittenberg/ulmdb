//! Explicit resident-slot pilot. Separate versioned WAL families; literal default.
//! Old engines/defaults unchanged. Demand anonymous maps, authenticated streamed
//! checkpoints and bounded-tail rotation retain old snapshot ownership. Keys/
//! duplicate values above65536 and keys above4080 remain unsupported.
//! Live independent reads require explicit capture-protocol negotiation.
//! Independent exports deliberately start without that source lease binding;
//! their owners can negotiate a new target-local capture protocol. Replication
//! remains separate work.
//! KV/IP overflow values use quota-owned extents up to16MiB; pool and WAL
//! admission may require more configured slots/tail capacity.
//! KV/duplicate ordering uses persisted builtin tags or explicit caller-owned
//! custom comparators. Cold paths validate custom trees structurally; callers
//! must supply the same comparison semantics for every access.
//! No sustained-speed claim is made by the lifecycle implementation.
mod audit;
mod catalog;
mod catalog_codec;
mod checkpoint;
mod collect;
pub use checkpoint::CheckpointReport;
#[cfg(feature = "resident-prefix-audit-cache")]
mod collect_cache;
#[cfg(feature = "resident-prefix-audit-cache")]
pub use collect_cache::{Limits as CollectCacheLimits, Stats as CollectCacheStats};
mod environment;
mod export;
pub use export::CapturedSnapshot;
pub use ulmdb_storage::resident::SnapshotExportReport;
mod graph_certificate;
mod incremental;
pub use graph_certificate::{Limits as IncrementalLimits, Metrics as IncrementalMetrics};
pub const MAX_DUPLICATE_VALUE_BYTES: usize = 65536;
mod duplicates;
mod journal;
mod live_recipe;
#[path = "../lookup/resident.rs"]
mod lookup_support;
pub mod object_graph;
mod ordering;
mod read;
mod readonly;
pub use ulmdb_storage::resident::CaptureLimits;
mod recipe;
mod recover;
mod savepoint;
mod write;
pub use collect::CollectReport;
pub use duplicates::{DuplicatePairs, DuplicateValues, DuplicatesRead};
pub use environment::{Environment, Options};
pub use savepoint::Savepoint;
pub use ulmdb_storage::resident::{MAX_OVERFLOW_VALUE_BYTES, OverflowStats};
/// Creation/writer policy; FullPostimageV2 is a same-family control, not a new reader.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedoFormat {
    LiteralV1,
    CompactV2,
    FullPostimageV2,
}
/// Writer-only implementation control. Canonical RW2 bytes and serving layout
/// are identical; the existing owned path remains the default.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RedoEncoding {
    #[default]
    Owned,
    Borrowed,
}
pub use read::{KvRead, PrefixRead, ReadTxn};
use std::sync::Arc;
use ulmdb_storage::{Reference, hybrid::redo::WalReceipt, resident::View};
pub use write::WriteTxn;
struct Snapshot {
    image: View,
    catalog: catalog::Catalog,
    root: Reference,
    next_database: u64,
    receipt: WalReceipt,
    commit: u128,
}
type SharedSnapshot = Arc<Snapshot>;
