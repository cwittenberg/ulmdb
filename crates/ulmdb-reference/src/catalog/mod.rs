//! Facade for typed named databases and their owned transaction oracle.

mod database;
mod transaction;

pub use database::{Catalog, Error, Handle, Kind};
pub use transaction::{Environment, Savepoint, Write};
