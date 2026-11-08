//! Complete checked binary-radix96 wire decoder, shared with its Verus entry.
//! Store access, graph traversal and publication remain outside this component.
use crate::{Address, Prefix};
#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::verus;
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

verus! {
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError { Size, Header, Family, Prefix, Reference, Alignment, FullWidthChildren }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Node {
    pub prefix: Prefix,
    pub children: [(u64,u64);2],
    pub value: (u64,u64),
}
pub open spec fn le64(b:Seq<u8>,at:int)->u64 {
    (b[at]as u64)|((b[at+1]as u64)<<8)|((b[at+2]as u64)<<16)|((b[at+3]as u64)<<24)
    |((b[at+4]as u64)<<32)|((b[at+5]as u64)<<40)|((b[at+6]as u64)<<48)|((b[at+7]as u64)<<56)
}
pub open spec fn be32(b:Seq<u8>,at:int)->u32 {
    ((b[at]as u32)<<24)|((b[at+1]as u32)<<16)|((b[at+2]as u32)<<8)|(b[at+3]as u32)
}
pub open spec fn be128(b:Seq<u8>,at:int)->u128 {
    ((be32(b,at)as u128)<<96)|((be32(b,at+4)as u128)<<64)
    |((be32(b,at+8)as u128)<<32)|(be32(b,at+12)as u128)
}
pub open spec fn reference(b:Seq<u8>,at:int)->(u64,u64) { (le64(b,at),le64(b,at+8)) }
pub open spec fn reference_valid(r:(u64,u64))->bool { r.0!=0 || r.1==0 }
pub open spec fn aligned(r:(u64,u64))->bool { (r.0==0 && r.1==0) || (r.0!=0 && r.1%16==0) }
pub open spec fn null(r:(u64,u64))->bool { r.0==0 && r.1==0 }
pub open spec fn address(b:Seq<u8>)->Address {
    if b[8]==4 { Address::V4(be32(b,16)) } else { Address::V6(be128(b,16)) }
}
pub open spec fn header_ok(b:Seq<u8>)->bool {
    le64(b,0)==0x0000006000000004 && (le64(b,8)>>16)==0 && le64(b,80)==0 && le64(b,88)==0
}
pub open spec fn family_ok(b:Seq<u8>)->bool {
    b[8]==6 || (b[8]==4 && le64(b,20)==0 && be32(b,28)==0)
}
pub open spec fn prefix_ok(b:Seq<u8>)->bool {
    b[9]<=crate::width(address(b)) && address(b)==crate::normalized(address(b),b[9])
}
pub open spec fn error(b:Seq<u8>)->Option<DecodeError> {
    if b.len()!=96 {Some(DecodeError::Size)}
    else if !header_ok(b) {Some(DecodeError::Header)}
    else if !family_ok(b) {Some(DecodeError::Family)}
    else if !prefix_ok(b) {Some(DecodeError::Prefix)}
    else if !reference_valid(reference(b,32)) || !reference_valid(reference(b,48)) || !reference_valid(reference(b,64)) {Some(DecodeError::Reference)}
    else if !aligned(reference(b,32)) || !aligned(reference(b,48)) || !aligned(reference(b,64)) {Some(DecodeError::Alignment)}
    else if b[9]==crate::width(address(b)) && (!null(reference(b,32)) || !null(reference(b,48))) {Some(DecodeError::FullWidthChildren)}
    else {None}
}
pub open spec fn valid(b:Seq<u8>)->bool { error(b).is_none() }
#[inline]
fn read64(b:&[u8],at:usize)->(v:u64)
    requires at as int+8<=b.len(),
    ensures v==le64(b@,at as int),
{
    crate::endian::decode_u64_le([b[at],b[at+1],b[at+2],b[at+3],b[at+4],b[at+5],b[at+6],b[at+7]])
}
#[inline]
fn read_be32(b:&[u8],at:usize)->(v:u32)
    requires at as int+4<=b.len(),
    ensures v==be32(b@,at as int),
{
    ((b[at]as u32)<<24)|((b[at+1]as u32)<<16)|((b[at+2]as u32)<<8)|(b[at+3]as u32)
}
#[inline(always)]
fn read_be128(b:&[u8],at:usize)->(v:u128)
    requires at as int+16<=b.len(),
    ensures v==be128(b@,at as int),
{
    ((read_be32(b,at)as u128)<<96)|((read_be32(b,at+4)as u128)<<64)
    |((read_be32(b,at+8)as u128)<<32)|(read_be32(b,at+12)as u128)
}
#[inline]
fn read_reference(b:&[u8],at:usize)->(r:Result<(u64,u64),DecodeError>)
    requires at as int+16<=b.len(),
    ensures match r {
        Ok(pair)=>pair==reference(b@,at as int) && reference_valid(pair),
        Err(e)=>e==DecodeError::Reference && !reference_valid(reference(b@,at as int)),
    },
{
    let segment=read64(b,at);let offset=read64(b,at+8);
    if segment==0 && offset!=0 {Err(DecodeError::Reference)} else {Ok((segment,offset))}
}
#[inline]
// Keep the actual modulo operation modeled by pinned Verus, without assuming a std specification.
#[allow(clippy::manual_is_multiple_of)]
fn is_aligned(pair:(u64,u64))->(yes:bool)
    ensures yes==aligned(pair),
{
    (pair.0==0 && pair.1==0) || (pair.0!=0 && pair.1%16==0)
}
/// Total for arbitrary byte-slice lengths; success exactly characterizes the96B format.
#[inline]
// Explicit matches keep the all-reference-before-alignment proof branches visible.
#[allow(clippy::question_mark)]
pub fn decode(bytes:&[u8])->(result:Result<Node,DecodeError>)
    ensures result.is_ok()==valid(bytes@),
        match result {
            Ok(node)=>node.prefix.valid() && node.prefix.address()==address(bytes@)
                && node.prefix.depth()==bytes@[9] && node.children[0]==reference(bytes@,32)
                && node.children[1]==reference(bytes@,48) && node.value==reference(bytes@,64),
            Err(e)=>error(bytes@)==Some(e),
        },
{
    if bytes.len()!=96 {return Err(DecodeError::Size);}
    if read64(bytes,0)!=0x0000006000000004 || (read64(bytes,8)>>16)!=0
        || read64(bytes,80)!=0 || read64(bytes,88)!=0 {return Err(DecodeError::Header);}
    let address=match bytes[8] {
        4=>{
            if read64(bytes,20)!=0 || read_be32(bytes,28)!=0 {return Err(DecodeError::Family);}
            Address::V4(read_be32(bytes,16))
        },
        6=>Address::V6(read_be128(bytes,16)),
        _=>return Err(DecodeError::Family),
    };
    let prefix=match Prefix::new(address,bytes[9]) {
        Ok(p)=>p,Err(_)=>return Err(DecodeError::Prefix),
    };
    // Decode ALL references before checking ANY alignment, as the original does.
    let left=match read_reference(bytes,32) {Ok(r)=>r,Err(e)=>return Err(e)};
    let right=match read_reference(bytes,48) {Ok(r)=>r,Err(e)=>return Err(e)};
    let value=match read_reference(bytes,64) {Ok(r)=>r,Err(e)=>return Err(e)};
    if !is_aligned(left) || !is_aligned(right) || !is_aligned(value) {return Err(DecodeError::Alignment);}
    let width=match address {Address::V4(_)=>32,Address::V6(_)=>128};
    if bytes[9]==width && (left!=(0,0) || right!=(0,0)) {return Err(DecodeError::FullWidthChildren);}
    Ok(Node {prefix,children:[left,right],value})
}
}
