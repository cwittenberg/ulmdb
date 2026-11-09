//! Atomic, bounded, multi-origin logical replication.
//!
//! Payloads are application-defined. Authenticate incoming transport separately;
//! SHA-256 identities detect corruption/conflicting replay, not publisher authority.
//! Logical pruning does not itself reclaim native copy-on-write storage.
mod baseline;
mod error;
mod snapshot;
mod store;
mod verified;
mod wire;

pub use error::{ProtocolError, ProtocolResult};
pub use verified::{OriginCheckpoint, VerifiedPage, apply_verified_page, checkpoint, encoded_len, verified_page};
pub use baseline::{Baseline, BaselineActivation, BaselineReceipt, create_from_baseline, import_baseline,
    identity as baseline_identity, provenance as baseline_provenance};
pub(crate) use baseline::reset_copied_journal;

pub use snapshot::{
    HandoverGuard, SnapshotReceipt, export_snapshot, export_snapshot_job, export_snapshot_with_control,
    rebase_all_snapshot, rebase_snapshot, restore_snapshot, validate_full_handover,
    validate_handover, validate_baseline_handover, verify_snapshot, verify_snapshot_job,
    verify_snapshot_with_control,
};
pub use store::{
    apply_page, cursor, cursors, initialize, initialize_with_realm, mark_serving, page, prune,
    realm, set_versions, versions,
};

/// Immutable identity is `(origin, sequence)`. An origin must have one fenced
/// writer. Ordinary history starts at one; imported history continues above its
/// explicit baseline anchor. Sequence numbers never reset within one realm.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Event {
    pub origin: u64,
    pub sequence: u64,
    /// Local durable-recording time; effective observation time belongs in payload.
    pub recorded_at: u64,
    pub payload: Vec<u8>,
}

/// Each page is charged for its complete persisted event encoding, not payload
/// alone. Limits apply to input, output and replay during recovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_events: usize,
    pub max_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_events: 1024,
            max_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyMode {
    /// Callback completes serving projections in this same atomic page.
    Synchronous,
    /// Callback stores evidence; serving advances explicitly after materialization.
    EvidenceOnly,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ApplyReport {
    pub applied: usize,
    pub replayed: usize,
    pub encoded_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cursor {
    pub applied: u64,
    pub serving: u64,
    /// Last removed sequence. A consumer with `after < floor` requires a snapshot.
    pub floor: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Versions {
    pub schema: u64,
    pub policy: u64,
}
impl Default for Versions {
    fn default() -> Self {
        Self {
            schema: 1,
            policy: 1,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Page {
    pub events: Vec<Event>,
    pub cursor: Cursor,
    pub more: bool,
    pub encoded_bytes: usize,
}

/// An application-authenticated durable acknowledgement. Distinct peer IDs are
/// counted once; this library does not infer durability from a network response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerAck {
    pub peer: u64,
    pub origin: u64,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PruneReport {
    pub removed: usize,
    pub encoded_bytes: usize,
    pub floor: u64,
}
