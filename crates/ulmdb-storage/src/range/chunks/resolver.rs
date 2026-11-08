//! Immutable, cold-admitted chunk directory. Construction is private to the
//! recovery implementation; arbitrary envelope metadata is not an authority.
use super::{
    header::StreamId,
    source::{ChunkLease, ChunkResolve, PackRead},
};
use crate::{Error, Result};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct VerifiedChunks {
    leases: Arc<Vec<ChunkLease>>,
    stream: StreamId,
    selected_end: u64,
}
impl VerifiedChunks {
    /// Call only after the complete selected chain and chunk continuations have
    /// been verified. Leases are contiguous, ordered from genesis to current.
    pub(super) fn from_verified(
        leases: Vec<ChunkLease>,
        stream: StreamId,
        selected_end: u64,
    ) -> Result<Self> {
        if leases.is_empty() || leases.last().map(ChunkLease::visible_end) != Some(selected_end) {
            return Err(Error::Corrupt("v7 resolver selected boundary"));
        }
        for (i, lease) in leases.iter().enumerate() {
            if lease.header().stream() != stream || lease.header().index() != i as u64 {
                return Err(Error::Corrupt("v7 resolver chunk sequence"));
            }
        }
        Ok(Self {
            leases: Arc::new(leases),
            stream,
            selected_end,
        })
    }
    pub(crate) fn reader(&self) -> Result<PackRead<Self>> {
        PackRead::new(self.clone(), self.stream, self.selected_end)
    }
    pub(crate) fn selected_end(&self) -> u64 {
        self.selected_end
    }
    pub(crate) fn chunk_count(&self) -> usize {
        self.leases.len()
    }
}
impl ChunkResolve for VerifiedChunks {
    fn lease(&self, index: u64) -> Result<ChunkLease> {
        let index = usize::try_from(index).map_err(|_| Error::LimitExceeded)?;
        self.leases
            .get(index)
            .cloned()
            .ok_or(Error::Corrupt("v7 unselected chunk"))
    }
}
