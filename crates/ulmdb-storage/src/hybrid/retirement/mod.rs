//! Read-only retirement primitives. These do not publish a floor or punch data.
mod aliases;
mod lease;
mod seeded;
pub mod selection;
pub use aliases::CheckpointAliases;
pub use lease::ExclusiveLease;
pub(crate) use lease::shared;
pub use seeded::{Boundary, SeededPrefix};
pub use seeded::{inspect, inspect_compound};
pub(crate) use seeded::{inspect_anchor, inspect_compound_anchor};
