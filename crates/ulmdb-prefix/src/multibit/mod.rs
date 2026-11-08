//! Experimental compressed4-bit radix tree. Newtag/version, no legacy-root upgrade.
//! Snapshots require immutable stores; old writer-private roots may be rewritten.
//! A mutating error requires aborting the surrounding writer/store transaction.
mod cursor;
mod read;
mod view;
mod wire;
mod write;
pub use cursor::{Cursor, Selection, ancestors, overlaps, subnets};
pub use read::{Reader, covering, exact, longest_match};
use ulmdb_core::storage::{Error, Result};
pub use write::{delete, delete_with_budget, insert, insert_with_budget};
/// Maximum decoded nodes per operation. Cursor budget is shared across next calls.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub max_nodes: usize,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            max_nodes: 1_000_000,
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Visits {
    remaining: usize,
}
impl Visits {
    pub fn new(b: Budget) -> Self {
        Self {
            remaining: b.max_nodes,
        }
    }
    pub fn take(&mut self) -> Result<()> {
        self.remaining = self.remaining.checked_sub(1).ok_or(Error::LimitExceeded)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
