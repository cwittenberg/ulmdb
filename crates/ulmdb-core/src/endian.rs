//! Shared executable little-endian/reference codec. Verus checks these bodies.
//! This proves fixed-size values; slice framing and other record decoders remain separate.
#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::verus;
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

verus! {
pub open spec fn u64_bytes(value:u64)->Seq<u8> {
    seq![value as u8,(value>>8) as u8,(value>>16) as u8,(value>>24) as u8,
         (value>>32) as u8,(value>>40) as u8,(value>>48) as u8,(value>>56) as u8]
}
pub open spec fn u64_value(bytes:Seq<u8>)->u64 {
    (bytes[0] as u64)|((bytes[1] as u64)<<8)|((bytes[2] as u64)<<16)|((bytes[3] as u64)<<24)
    |((bytes[4] as u64)<<32)|((bytes[5] as u64)<<40)|((bytes[6] as u64)<<48)|((bytes[7] as u64)<<56)
}
#[inline]
pub fn encode_u64_le(value:u64)->(bytes:[u8;8])
    ensures bytes@==u64_bytes(value),
{
    [value as u8,(value>>8) as u8,(value>>16) as u8,(value>>24) as u8,
     (value>>32) as u8,(value>>40) as u8,(value>>48) as u8,(value>>56) as u8]
}
#[inline]
pub fn decode_u64_le(bytes:[u8;8])->(value:u64)
    ensures value==u64_value(bytes@),
{
    (bytes[0] as u64)|((bytes[1] as u64)<<8)|((bytes[2] as u64)<<16)|((bytes[3] as u64)<<24)
    |((bytes[4] as u64)<<32)|((bytes[5] as u64)<<40)|((bytes[6] as u64)<<48)|((bytes[7] as u64)<<56)
}
pub proof fn u64_roundtrip(value:u64)
    ensures u64_value(u64_bytes(value))==value,
{
    assert((value as u8 as u64)|(((value>>8) as u8 as u64)<<8)|(((value>>16) as u8 as u64)<<16)|(((value>>24) as u8 as u64)<<24)
      |(((value>>32) as u8 as u64)<<32)|(((value>>40) as u8 as u64)<<40)|(((value>>48) as u8 as u64)<<48)|(((value>>56) as u8 as u64)<<56)==value) by(bit_vector);
}
pub proof fn u64_bytes_roundtrip(bytes:Seq<u8>)
    requires bytes.len()==8,
    ensures u64_bytes(u64_value(bytes))==bytes,
{
    let b0=bytes[0];let b1=bytes[1];let b2=bytes[2];let b3=bytes[3];
    let b4=bytes[4];let b5=bytes[5];let b6=bytes[6];let b7=bytes[7];
    let value=(b0 as u64)|((b1 as u64)<<8)|((b2 as u64)<<16)|((b3 as u64)<<24)
      |((b4 as u64)<<32)|((b5 as u64)<<40)|((b6 as u64)<<48)|((b7 as u64)<<56);
    assert(value as u8==b0) by(bit_vector) requires value==((b0 as u64)|((b1 as u64)<<8)|((b2 as u64)<<16)|((b3 as u64)<<24)|((b4 as u64)<<32)|((b5 as u64)<<40)|((b6 as u64)<<48)|((b7 as u64)<<56));
    assert((value>>8) as u8==b1) by(bit_vector) requires value==((b0 as u64)|((b1 as u64)<<8)|((b2 as u64)<<16)|((b3 as u64)<<24)|((b4 as u64)<<32)|((b5 as u64)<<40)|((b6 as u64)<<48)|((b7 as u64)<<56));
    assert((value>>16) as u8==b2) by(bit_vector) requires value==((b0 as u64)|((b1 as u64)<<8)|((b2 as u64)<<16)|((b3 as u64)<<24)|((b4 as u64)<<32)|((b5 as u64)<<40)|((b6 as u64)<<48)|((b7 as u64)<<56));
    assert((value>>24) as u8==b3) by(bit_vector) requires value==((b0 as u64)|((b1 as u64)<<8)|((b2 as u64)<<16)|((b3 as u64)<<24)|((b4 as u64)<<32)|((b5 as u64)<<40)|((b6 as u64)<<48)|((b7 as u64)<<56));
    assert((value>>32) as u8==b4) by(bit_vector) requires value==((b0 as u64)|((b1 as u64)<<8)|((b2 as u64)<<16)|((b3 as u64)<<24)|((b4 as u64)<<32)|((b5 as u64)<<40)|((b6 as u64)<<48)|((b7 as u64)<<56));
    assert((value>>40) as u8==b5) by(bit_vector) requires value==((b0 as u64)|((b1 as u64)<<8)|((b2 as u64)<<16)|((b3 as u64)<<24)|((b4 as u64)<<32)|((b5 as u64)<<40)|((b6 as u64)<<48)|((b7 as u64)<<56));
    assert((value>>48) as u8==b6) by(bit_vector) requires value==((b0 as u64)|((b1 as u64)<<8)|((b2 as u64)<<16)|((b3 as u64)<<24)|((b4 as u64)<<32)|((b5 as u64)<<40)|((b6 as u64)<<48)|((b7 as u64)<<56));
    assert((value>>56) as u8==b7) by(bit_vector) requires value==((b0 as u64)|((b1 as u64)<<8)|((b2 as u64)<<16)|((b3 as u64)<<24)|((b4 as u64)<<32)|((b5 as u64)<<40)|((b6 as u64)<<48)|((b7 as u64)<<56));
    assert forall|i:int| 0<=i<8 implies #[trigger] u64_bytes(value)[i]==bytes[i] by {
        if i==0 {} else if i==1 {} else if i==2 {} else if i==3 {} else if i==4 {} else if i==5 {} else if i==6 {} else {assert(i==7);}
    }
    assert(u64_bytes(value) =~= bytes);
}
pub proof fn u64_encoding_is_injective(a:u64,b:u64)
    requires u64_bytes(a)==u64_bytes(b),
    ensures a==b,
{u64_roundtrip(a);u64_roundtrip(b);}
pub open spec fn reference_bytes(segment:u64,offset:u64)->Seq<u8> {u64_bytes(segment)+u64_bytes(offset)}
pub open spec fn reference_is_valid(segment:u64,offset:u64)->bool {segment!=0 || offset==0}
#[inline]
pub fn encode_reference(segment:u64,offset:u64)->(bytes:[u8;16])
    ensures bytes@==reference_bytes(segment,offset),
{
    let a=encode_u64_le(segment);let b=encode_u64_le(offset);
    let mut bytes=[0u8;16];
    bytes[..8].copy_from_slice(&a);bytes[8..].copy_from_slice(&b);
    bytes
}
#[inline]
pub fn decode_reference(bytes:[u8;16])->(decoded:Option<(u64,u64)>)
    ensures match decoded {
        Some((segment,offset))=>reference_is_valid(segment,offset) && bytes@==reference_bytes(segment,offset),
        None=>u64_value(bytes@.subrange(0,8))==0 && u64_value(bytes@.subrange(8,16))!=0,
    },
{
    let low=[bytes[0],bytes[1],bytes[2],bytes[3],bytes[4],bytes[5],bytes[6],bytes[7]];
    let high=[bytes[8],bytes[9],bytes[10],bytes[11],bytes[12],bytes[13],bytes[14],bytes[15]];
    let segment=decode_u64_le(low);let offset=decode_u64_le(high);
    proof {
        assert(low@ =~= bytes@.subrange(0,8));assert(high@ =~= bytes@.subrange(8,16));
        u64_bytes_roundtrip(low@);u64_bytes_roundtrip(high@);
        assert(bytes@ =~= low@+high@);
    }
    if (segment==0) & (offset!=0) {None}else{Some((segment,offset))}
}
pub proof fn reference_roundtrip(segment:u64,offset:u64)
    ensures u64_value(reference_bytes(segment,offset).subrange(0,8))==segment,
        u64_value(reference_bytes(segment,offset).subrange(8,16))==offset,
{
    u64_roundtrip(segment);u64_roundtrip(offset);
    assert(reference_bytes(segment,offset).subrange(0,8) =~= u64_bytes(segment));
    assert(reference_bytes(segment,offset).subrange(8,16) =~= u64_bytes(offset));
}
pub proof fn reference_encoding_is_injective(a:u64,b:u64,c:u64,d:u64)
    requires reference_bytes(a,b)==reference_bytes(c,d),
    ensures a==c && b==d,
{reference_roundtrip(a,b);reference_roundtrip(c,d);}

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Reference;
    #[test]
    fn fixed_codecs_match_standard_endian_for_boundary_and_seeded_values() {
        let mut value = 0x7e5a_b291_d062_c43fu64;
        for i in 0..10_000 {
            value = value
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let segment = if i < 64 { 1u64 << i } else { value };
            let offset = if i < 64 {
                u64::MAX >> i
            } else {
                value.rotate_left(29)
            };
            assert_eq!(encode_u64_le(segment), segment.to_le_bytes());
            assert_eq!(decode_u64_le(offset.to_le_bytes()), offset);
            let mut expected = [0u8; 16];
            expected[..8].copy_from_slice(&segment.to_le_bytes());
            expected[8..].copy_from_slice(&offset.to_le_bytes());
            let at = Reference { segment, offset };
            assert_eq!(at.encode(), expected);
            assert_eq!(Reference::decode(&expected).unwrap(), at);
        }
        assert_eq!(Reference::decode(&[0; 16]).unwrap(), Reference::NULL);
        let max = Reference {
            segment: u64::MAX,
            offset: u64::MAX,
        };
        assert_eq!(Reference::decode(&max.encode()).unwrap(), max);
    }
    #[test]
    fn every_byte_position_matches_null_validation_and_slice_framing() {
        for length in 0..16 {
            assert!(Reference::decode(&vec![0; length]).is_err());
        }
        for position in 0..16 {
            for byte in 0..=u8::MAX {
                let mut raw = [0u8; 24];
                raw[position] = byte;
                raw[16..].fill(0xa5);
                let segment = u64::from_le_bytes(raw[..8].try_into().unwrap());
                let offset = u64::from_le_bytes(raw[8..16].try_into().unwrap());
                let expected = if segment == 0 && offset != 0 {
                    None
                } else {
                    Some(Reference { segment, offset })
                };
                assert_eq!(Reference::decode(&raw).ok(), expected);
            }
        }
    }
}
