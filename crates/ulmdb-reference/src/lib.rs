//! Deliberately simple owned oracles, independent of production algorithms.
//! The crate facade exposes model capabilities while modules own their behavior.

mod prefix;
mod state;

pub mod catalog;
pub mod workloads;

pub use prefix::{ModelPrefix, PrefixStore};
pub use state::{Environment, State, Write};
