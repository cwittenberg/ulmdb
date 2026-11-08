//! Private PACK-01/02 protocol, adapted from the tested packed substrate.
pub(super) mod codec;
mod fault;
pub(super) mod io;
pub(super) mod recovery;
#[cfg(feature = "range-lifecycle")]
pub(super) mod snapshot;
mod writer;
use crate::{Error, Result};
pub(crate) use fault::point;
pub(crate) use writer::Writer;
pub(crate) const BLOCK: usize = 4096;
pub(crate) const ALIGNMENT: u64 = 65_536;
pub(crate) const HEAD_OFFSETS: [u64; 2] = [65_536, 131_072];
pub(crate) const DATA_START: u64 = 196_608;
pub(crate) const MAX_PAYLOAD: u64 = 268_435_456;
pub(crate) const MAX_FILE_BYTES: u64 = 1 << 40;

/// An authenticated published frame location. Constructors stay private so an
/// unselected preparation cannot acquire a public mapping through safe APIs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Receipt {
    pub(crate) offset: u64,
    pub(crate) ordinal: u64,
    pub(crate) payload_length: u64,
    pub(crate) hash: [u8; 32],
}
impl Receipt {
    pub fn frame_end(self) -> Result<u64> {
        self.offset
            .checked_add(frame_length(self.payload_length)?)
            .filter(|end| *end <= MAX_FILE_BYTES)
            .ok_or(Error::LimitExceeded)
    }
    pub(crate) fn validate(self) -> Result<()> {
        if self.offset < DATA_START
            || !self.offset.is_multiple_of(ALIGNMENT)
            || (self.ordinal == 0 && (self.offset != DATA_START || self.payload_length != 0))
            || (self.offset == DATA_START && self.ordinal != 0)
            || self.ordinal > (self.offset.saturating_sub(DATA_START)) / ALIGNMENT
        {
            return Err(Error::Corrupt("invalid packed receipt geometry"));
        }
        self.frame_end()?;
        Ok(())
    }
}
pub(crate) fn frame_length(payload: u64) -> Result<u64> {
    if payload > MAX_PAYLOAD {
        return Err(Error::LimitExceeded);
    }
    let bytes = payload
        .checked_add((BLOCK * 2) as u64)
        .and_then(|n| n.checked_add(ALIGNMENT - 1))
        .ok_or(Error::LimitExceeded)?;
    Ok(bytes / ALIGNMENT * ALIGNMENT)
}
