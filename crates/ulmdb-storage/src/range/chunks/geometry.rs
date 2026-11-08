//! Thin executable consumers of Root's shared production/proof arithmetic.
use crate::{Error, Result};
pub use ulmdb_core::chunk_geometry::{
    ALIGN_BYTES, DATA_BYTES, HEADER_BYTES, frame_end, locate, next_frame,
};
pub use ulmdb_core::chunk_geometry::{MAX_PAYLOAD_BYTES as MAX_PAYLOAD, frame_length};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameDescriptor {
    start: u64,
    end: u64,
    ordinal: u64,
    hash: [u8; 32],
}
impl FrameDescriptor {
    /// Checked envelope metadata, NOT a selected-publication authority. A writer
    /// must supply fields from its verified v7 frame/head before continuation.
    pub fn new(start: u64, end: u64, ordinal: u64, hash: [u8; 32]) -> Result<Self> {
        let length = end
            .checked_sub(start)
            .ok_or(Error::Corrupt("chunk parent underflow"))?;
        if frame_end(start, length) != Some(end)
            || length > frame_length(MAX_PAYLOAD).unwrap()
            || (ordinal == 0 && start != 0)
            || (ordinal != 0 && start == 0)
            || ordinal > start / ALIGN_BYTES
        {
            return Err(Error::Corrupt("chunk parent frame geometry"));
        }
        Ok(Self {
            start,
            end,
            ordinal,
            hash,
        })
    }
    pub fn start(self) -> u64 {
        self.start
    }
    pub fn end(self) -> u64 {
        self.end
    }
    pub fn ordinal(self) -> u64 {
        self.ordinal
    }
    pub fn hash(self) -> [u8; 32] {
        self.hash
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppendPlan {
    pub start: u64,
    pub end: u64,
    pub chunk: u64,
    pub ordinal: u64,
    pub rollover: bool,
    pub unallocated_tail: Option<(u64, u64)>,
}
impl AppendPlan {
    pub fn after(parent: FrameDescriptor, payload: u64) -> Result<Self> {
        let length = frame_length(payload).ok_or(Error::LimitExceeded)?;
        let start = next_frame(parent.start, parent.end, length).ok_or(Error::LimitExceeded)?;
        let end = frame_end(start, length).ok_or(Error::LimitExceeded)?;
        let chunk = locate(start).0;
        let prior = locate(parent.start).0;
        let rollover = chunk != prior;
        if rollover && prior.checked_add(1) != Some(chunk) {
            return Err(Error::Corrupt("chunk append skipped index"));
        }
        let unallocated_tail = if start > parent.end {
            Some((parent.end, start))
        } else {
            None
        };
        Ok(Self {
            start,
            end,
            chunk,
            ordinal: parent.ordinal.checked_add(1).ok_or(Error::LimitExceeded)?,
            rollover,
            unallocated_tail,
        })
    }
}
