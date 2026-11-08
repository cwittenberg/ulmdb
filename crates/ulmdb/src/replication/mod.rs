//! Authenticated immutable snapshot transfer and fenced replica activation.
pub(crate) mod activation;
pub(crate) mod assets;
mod receive;
mod role;
mod send;
mod types;
mod wire;
pub use activation::{ApplyOutcome, ApplyReport, bootstrap, bootstrap_with_residency};
pub use receive::receive;
pub(crate) use role::{ReplicaIdentity, load_role, write_role};
pub use send::send_snapshot;
pub use types::*;
