//! Logical-to-physical arithmetic for the experimental chunked range store.
//! This proves arithmetic contracts, not file durability or receipt authority.
#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::verus;
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

verus! {
pub const DATA_BYTES: u64 = 1_073_741_824;
pub const HEADER_BYTES: u64 = 65_536;
pub const ALIGN_BYTES: u64 = 65_536;
pub const MAX_PAYLOAD_BYTES: u64 = 256 * 1024 * 1024;

proof fn constants()
    ensures DATA_BYTES == 1_073_741_824, HEADER_BYTES == 65_536,
        ALIGN_BYTES == 65_536, MAX_PAYLOAD_BYTES == 268_435_456,
{
    reveal(DATA_BYTES);
    reveal(HEADER_BYTES);
    reveal(ALIGN_BYTES);
    reveal(MAX_PAYLOAD_BYTES);
}

pub open spec fn fits(start: u64, length: u64) -> bool {
    0 < length <= DATA_BYTES
        && start % ALIGN_BYTES == 0 && length % ALIGN_BYTES == 0
        && (start % DATA_BYTES) as int + length as int <= DATA_BYTES as int
        && start as int + length as int <= u64::MAX as int
}

/// A physical offset fits a positive signed 32-bit file/mapping extent.
#[inline]
pub fn locate(offset: u64) -> (result: (u64, u64))
    ensures
        result.0 == offset / DATA_BYTES,
        result.1 == HEADER_BYTES + offset % DATA_BYTES,
        HEADER_BYTES <= result.1 < HEADER_BYTES + DATA_BYTES,
        result.1 <= 0x7fff_ffff,
        result.0 as int * DATA_BYTES as int
            + result.1 as int - HEADER_BYTES as int == offset as int,
{
    proof { constants(); }
    (offset / DATA_BYTES, HEADER_BYTES + offset % DATA_BYTES)
}

/// Logical zero is a valid first-frame start; an empty extent is invalid.
#[allow(clippy::manual_is_multiple_of)]
#[inline]
pub fn frame_end(start: u64, length: u64) -> (result: Option<u64>)
    ensures match result {
        Some(end) => fits(start, length) && end as int == start as int + length as int,
        None => !fits(start, length),
    },
{
    proof { constants(); }
    if length == 0 || length > DATA_BYTES
        || start % ALIGN_BYTES != 0 || length % ALIGN_BYTES != 0 {
        return None;
    }
    if start % DATA_BYTES > DATA_BYTES - length {
        return None;
    }
    let end = (start as u128) + (length as u128);
    if end > u64::MAX as u128 { None } else { Some(end as u64) }
}

pub open spec fn next_start(parent_start: u64, parent_end: u64, length: u64) -> Option<u64> {
    if parent_end <= parent_start
        || !fits(parent_start, (parent_end - parent_start) as u64) {
        None
    } else if fits(parent_end, length) {
        Some(parent_end)
    } else {
        let next = (parent_start as int / DATA_BYTES as int + 1) * DATA_BYTES as int;
        if next <= u64::MAX as int && fits(next as u64, length) {
            Some(next as u64)
        } else { None }
    }
}

/// Continue in the current chunk or advance exactly one logical chunk. Skipped
/// logical tail space does not authorize reading or physically allocating it.
#[inline]
pub fn next_frame(parent_start: u64, parent_end: u64, length: u64) -> (result: Option<u64>)
    ensures result == next_start(parent_start, parent_end, length),
        match result { Some(start) => fits(start, length), None => true },
{
    proof { constants(); }
    if parent_end <= parent_start { return None; }
    match frame_end(parent_start, parent_end - parent_start) {
        None => return None,
        Some(_) => {},
    }
    match frame_end(parent_end, length) {
        Some(_) => return Some(parent_end),
        None => {},
    }
    let base = parent_start - parent_start % DATA_BYTES;
    let next = (base as u128) + (DATA_BYTES as u128);
    if next > u64::MAX as u128 { return None; }
    let next = next as u64;
    match frame_end(next, length) {
        Some(_) => Some(next),
        None => None,
    }
}

/// The two 4-KiB envelope blocks plus payload round up to a 64-KiB extent.
#[inline]
pub fn frame_length(payload: u64) -> (result: Option<u64>)
    ensures match result {
        Some(length) => payload <= MAX_PAYLOAD_BYTES
            && length as int == ((payload as int + 8192 + ALIGN_BYTES as int - 1)
                / ALIGN_BYTES as int) * ALIGN_BYTES as int
            && ALIGN_BYTES <= length <= DATA_BYTES
            && length % ALIGN_BYTES == 0,
        None => payload > MAX_PAYLOAD_BYTES,
    },
{
    proof { constants(); }
    if payload > MAX_PAYLOAD_BYTES { return None; }
    let blocks = (payload + 8192 + ALIGN_BYTES - 1) / ALIGN_BYTES;
    Some(blocks * ALIGN_BYTES)
}
}

#[cfg(test)]
mod tests;
