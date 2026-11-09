//! Checked, compressed binary radix indexes over borrowed immutable storage.
//!
//! Each root belongs to one address family. Writes use the store's copy-on-write
//! policy; keep old roots only across immutable snapshots, not private rewrites.
mod bulk;
mod batch;
mod cursor;
#[cfg(any(not(feature = "scalar-family"), test))]
mod lookup;
mod mutate;
mod node;
pub mod object_graph;
#[cfg(any(not(feature = "scalar-family"), test))]
mod reader;
#[cfg(feature = "scalar-family")]
mod scalar_lookup;
#[cfg(feature = "scalar-family")]
mod scalar_reader;

pub use bulk::{BulkBuild, build_sorted};
pub use batch::{BatchUpsert, upsert_sorted};
pub use cursor::{Cursor, SubnetPosition, ancestors, overlaps, subnets};
#[cfg(not(feature = "scalar-family"))]
pub use lookup::{covering, exact, get, longest_match};
pub use mutate::{delete, insert};
#[cfg(not(feature = "scalar-family"))]
pub use reader::Reader;
#[cfg(feature = "scalar-family")]
pub use scalar_lookup::{covering, exact, get, longest_match};
#[cfg(feature = "scalar-family")]
pub use scalar_reader::Reader;
pub const fn scalar_family_enabled() -> bool {
    cfg!(feature = "scalar-family")
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod layout_probe;

#[cfg(all(test, feature = "scalar-family"))]
mod scalar_tests;

/// Experimental checked4-bit compressed radix format; never a binary-root upgrade.
pub mod multibit;
