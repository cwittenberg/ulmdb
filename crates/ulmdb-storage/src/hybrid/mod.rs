//! Experimental materialized image and strict redo authority.
mod arena;
pub mod borrowed;
mod checked;
pub mod checkpoint;
pub mod codec;
mod image;
pub mod journal;
pub mod provenance;
pub mod recovery;
pub mod redo;
pub mod retirement;
mod table;
mod undo;
mod view;
pub use arena::{Allocation, FrozenExtent, ImageArena};
pub use image::{ChunkLimits, ChunkStatus, ChunkWriter, ImageMode, ReadSpan};
pub use table::ImageTable;
pub use undo::ImageCheckpoint;

#[cfg(test)]
mod tests;
