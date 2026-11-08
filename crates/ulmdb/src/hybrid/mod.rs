//! Experimental real KV/IP transaction backend: materialized pages + strict redo.
//!
//! This opt-in pilot has bounded append-only images/WAL and fresh-image recovery.
//! It does not replace `crate::Environment` or claim lifecycle/performance parity.
mod cache;
mod checkpoint;
mod environment;
mod live_recipe;
mod provenance;
mod read;
mod recover;
mod recover_seeded;
mod retirement;
pub use retirement::{RetirementInstallReport, RetirementPreparation, RetirementReport};
mod savepoint;
mod write;

pub use checkpoint::{
    CaptureReport, CheckpointCapture, CheckpointInstallReport, PreparedCheckpoint,
};
pub use environment::{Environment, ImageUsage, LiveRedoMode, LiveRedoUsage, Options, RedoMode};
pub use read::{KvRead, PrefixRead, ReadTxn};
pub use savepoint::Savepoint;
pub use write::WriteTxn;

use crate::catalog::Database;
use std::{collections::BTreeMap, sync::Arc};
use ulmdb_storage::hybrid::{
    ImageTable,
    redo::{RootState, WalReceipt},
};

struct Snapshot {
    image: ImageTable,
    catalog: BTreeMap<Vec<u8>, Database>,
    root: RootState,
    receipt: WalReceipt,
    commit: u128,
}
type SharedSnapshot = Arc<Snapshot>;

/// Optional replay-cache backing; WAL stays file-backed.
pub use ulmdb_storage::hybrid::ImageMode;

/// Opt-in physical WAL layout; Dense remains default.
pub use ulmdb_storage::hybrid::journal::WalLayout;
