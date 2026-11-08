//! Default-off v7 chunk transport with checked publication and recovery.
//! Engine integration, retained checkpoints and reclamation remain pending.
//! No v5/v6 selector or parser is changed.
pub mod geometry;
pub mod header;
pub(crate) mod protocol;
pub(crate) mod recovery;
#[cfg(test)]
mod recovery_tests;
pub(crate) mod resolver;
pub mod source;
#[cfg(test)]
mod tests;
pub(crate) mod writer;
#[cfg(test)]
mod writer_tests;
