//! Fixed ordered-page header/slot geometry, shared by Page decoding and Verus.
//! This does not validate cell bodies, references, payloads or entire graphs.
#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::verus;
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

verus! {
pub open spec fn slot_start(index:int)->int {32 + 2 * index}
pub open spec fn geometry(count:u16, lower:u16, upper:u16)->bool {
    lower as int == 32 + 2 * (count as int)
        && lower <= upper && upper <= 4096 && upper % 2 == 0
}
/// Exact old Page::decode predicate, with wide intermediates for every raw u16.
// Keep the exact modulo operator in the shared executable proof body.
#[allow(clippy::manual_is_multiple_of)]
#[inline]
pub fn valid_geometry(count:u16, lower:u16, upper:u16)->(valid:bool)
    ensures valid == geometry(count,lower,upper),
        valid ==> count <= 2032 && lower >= 32 && lower % 2 == 0,
        valid ==> forall|index:int| 0 <= index < count as int ==>
            32 <= #[trigger] slot_start(index)
            && slot_start(index) + 2 <= lower as int
            && slot_start(index) + 2 <= upper as int
            && slot_start(index) + 2 <= 4096,
{
    let slots_end = 32u32 + 2u32 * (count as u32);
    (lower as u32) == slots_end
        && ((lower <= upper) & (upper <= 4096) & (upper % 2 == 0))
}
/// Consumer arithmetic after Page::decode accepted the raw header and
/// cell_bytes rejected index>=count. Arithmetic stays bounded on both widths.
pub proof fn accepted_slot_extent(count:u16,lower:u16,upper:u16,index:usize)
    requires geometry(count,lower,upper), index < count as usize,
    ensures (index as int) < 2032,
        32 + 2 * (index as int) + 2 <= lower as int,
        32 + 2 * (index as int) + 2 <= upper as int,
        32 + 2 * (index as int) + 2 <= 4096,
        32 + 2 * (index as int) + 2 <= usize::MAX as int,
{}
/// Slot-table bytes precede the cell-data region even when there is no gap.
pub proof fn regions_disjoint(count:u16,lower:u16,upper:u16,slot_byte:int,cell_byte:int)
    requires geometry(count,lower,upper),
        32 <= slot_byte < lower as int,
        upper as int <= cell_byte < 4096,
    ensures slot_byte < cell_byte,
{}
}

#[cfg(test)]
mod tests;
