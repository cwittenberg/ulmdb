//! Shared executable primitives: Cargo erases contracts; Verus checks this file.

#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::verus;
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

verus! {

/// Tests an extent without overflowing at either end of the u64 address space.
pub fn range_fits(offset: u64, length: u64, total: u64) -> (fits: bool)
    ensures fits == (offset as int + length as int <= total as int),
{
    offset <= total && length <= total - offset
}

/// Prefix equality accepts every u8 depth and rejects depths beyond IPv4.
pub fn prefix_equal32(address: u32, network: u32, depth: u8) -> (equal: bool)
    ensures equal == (if depth > 32 { false }
        else if depth == 0 { true }
        else { (address >> (32 - depth)) == (network >> (32 - depth)) }),
{
    if depth > 32 { false }
    else if depth == 0 { true }
    else { (address >> (32 - depth)) == (network >> (32 - depth)) }
}

/// Prefix equality accepts every u8 depth and rejects depths beyond IPv6.
pub fn prefix_equal128(address: u128, network: u128, depth: u8) -> (equal: bool)
    ensures equal == (if depth > 128 { false }
        else if depth == 0 { true }
        else { (address >> (128 - depth)) == (network >> (128 - depth)) }),
{
    if depth > 128 { false }
    else if depth == 0 { true }
    else { (address >> (128 - depth)) == (network >> (128 - depth)) }
}

}
