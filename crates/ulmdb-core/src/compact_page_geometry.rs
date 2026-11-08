//! Compact-v2 ordered-page record geometry, shared by production and Verus.
//! Tags, levels, reference bodies, ordering and graph semantics are excluded.
#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::verus;
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

verus! {
pub open spec fn stride(leaf:bool,key_width:u16,value_width:u16)->int {
    key_width as int + if leaf {value_width as int} else {16}
}
pub open spec fn row_start(index:int, width:int)->int {32 + index * width}
pub open spec fn geometry(leaf:bool,count:u16,lower:u16,upper:u16,key_width:u16,value_width:u16)->bool {
    lower == 32 && key_width <= 256 && value_width <= 512
        && (leaf || value_width == 0)
        && upper as int == 32 + count as int * stride(leaf,key_width,value_width)
        && upper <= 4096
        && (stride(leaf,key_width,value_width) != 0 || count <= 1)
}
/// Width checks precede multiplication: even arbitrary raw u16 headers cannot
/// overflow u32 before the geometry guard decides whether to reject the page.
#[inline]
pub fn valid_geometry(leaf:bool,count:u16,lower:u16,upper:u16,key_width:u16,value_width:u16)->(valid:bool)
    ensures valid == geometry(leaf,count,lower,upper,key_width,value_width),
        valid ==> 0 <= stride(leaf,key_width,value_width) <= 768,
        valid ==> forall|index:int| 0 <= index < count as int ==>
            32 <= #[trigger] row_start(index,stride(leaf,key_width,value_width))
            && row_start(index,stride(leaf,key_width,value_width))
                + stride(leaf,key_width,value_width) <= upper as int
            && row_start(index,stride(leaf,key_width,value_width))
                + stride(leaf,key_width,value_width) <= 4096,
{
    if lower != 32 || key_width > 256 || value_width > 512 || (!leaf && value_width != 0) {
        return false;
    }
    let record_width = key_width as u32 + if leaf {value_width as u32} else {16u32};
    assert(0 <= record_width <= 768);
    assert(count as int * record_width as int <= 65535 * 768) by(nonlinear_arith)
        requires 0 <= (count as int) <= 65535, 0 <= (record_width as int) <= 768;
    let end = 32u32 + count as u32 * record_width;
    let valid = (upper as u32 == end) && (upper <= 4096) && (record_width != 0 || count <= 1);
    if valid {
        proof { accepted_rows(leaf,count,lower,upper,key_width,value_width); }
    }
    valid
}
/// Every accepted record extent fits in a 4096-byte page; zero-width records
/// have the empty extent [32,32), with at most one admitted row.
pub proof fn accepted_rows(leaf:bool,count:u16,lower:u16,upper:u16,key_width:u16,value_width:u16)
    requires geometry(leaf,count,lower,upper,key_width,value_width),
    ensures forall|index:int| 0 <= index < count as int ==>
        32 <= #[trigger] row_start(index,stride(leaf,key_width,value_width))
        && row_start(index,stride(leaf,key_width,value_width))
            + stride(leaf,key_width,value_width) <= upper as int
        && row_start(index,stride(leaf,key_width,value_width))
            + stride(leaf,key_width,value_width) <= 4096,
{
    let width = stride(leaf,key_width,value_width);
    assert forall|index:int| 0 <= index < count as int implies
        32 <= #[trigger] row_start(index,width)
        && row_start(index,width) + width <= upper as int
        && row_start(index,width) + width <= 4096 by {
        assert(0 <= width <= 768);
        assert(0 <= index * width && (index + 1) * width <= count as int * width) by(nonlinear_arith)
            requires 0 <= index < count as int, 0 <= width <= 768;
        assert(index * width + width == (index + 1) * width) by(nonlinear_arith);
    }
}
/// Distinct positive-width records cannot overlap. All accepted row arithmetic
/// is <=4096, thus representable on both 32-bit and 64-bit usize targets.
pub proof fn accepted_row_arithmetic(leaf:bool,count:u16,lower:u16,upper:u16,key_width:u16,value_width:u16,index:u16,next:u16)
    requires geometry(leaf,count,lower,upper,key_width,value_width), index < count, index < next, next <= count,
    ensures 0 <= index as int * stride(leaf,key_width,value_width) <= 4064,
        row_start(index as int,stride(leaf,key_width,value_width))
            + stride(leaf,key_width,value_width) <= row_start(next as int,stride(leaf,key_width,value_width)),
        row_start(next as int,stride(leaf,key_width,value_width)) <= upper as int,
        row_start(index as int,stride(leaf,key_width,value_width))
            + stride(leaf,key_width,value_width) <= u32::MAX as int,
        row_start(index as int,stride(leaf,key_width,value_width))
            + stride(leaf,key_width,value_width) <= usize::MAX as int,
        stride(leaf,key_width,value_width) == 0 ==> count <= 1
            && row_start(index as int,stride(leaf,key_width,value_width)) == 32,
{
    let width = stride(leaf,key_width,value_width);
    assert(0 <= width <= 768);
    assert(0 <= index as int * width && (index as int + 1) * width <= next as int * width
        && next as int * width <= count as int * width) by(nonlinear_arith)
        requires 0 <= (index as int) < (next as int) <= (count as int), 0 <= width <= 768;
    assert(index as int * width + width == (index as int + 1) * width) by(nonlinear_arith);
}
}

#[cfg(test)]
mod tests;
